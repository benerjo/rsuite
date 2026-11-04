use eframe::egui::{self, ViewportBuilder};

use crate::{
    configuration::{self, ConfigurationValue, FloatValueInRange},
    midiinput::MidiInput,
    synth::hardware::{HardWare, KeyBoardKey},
    utils::{CommonError, ConnectionType, KeyBoardKeySetter},
};

///The maximum amount of amplification before the signal is clipped
const MAX_DRIVE: f64 = 50.0;
///The maximum cutoff frequency of the tone filter, in hertz
const MAX_TONE: f64 = 20000.0;

#[derive(Debug, Clone, PartialEq)]
struct Configuration {
    /// The amount of amplification before the signal is clipped
    drive: FloatValueInRange,
    /// The cutoff frequency of the low pass filter applied after clipping, in hertz.
    /// At 0, no sound goes through
    tone: FloatValueInRange,
    /// The volume of the output, between 0 and 1
    volume: FloatValueInRange,
}

impl Configuration {
    pub fn new() -> Configuration {
        Self {
            drive: FloatValueInRange::new(10.0, 0.0, MAX_DRIVE, "drive", KeyBoardKey::Parameter),
            tone: FloatValueInRange::new(5000.0, 0.0, MAX_TONE, "tone", KeyBoardKey::ParameterB),
            volume: FloatValueInRange::new(0.5, 0.0, 1.0, "volume", KeyBoardKey::Gain),
        }
    }
}

impl<'c> configuration::Configuration<'c> for Configuration {
    fn elements(&'c mut self) -> Vec<ConfigurationValue<'c>> {
        vec![
            ConfigurationValue::Float(&mut self.drive),
            ConfigurationValue::Float(&mut self.tone),
            ConfigurationValue::Float(&mut self.volume),
        ]
    }
}

///The clipping and the filtering applied on the signal
struct Clipper {
    /// The audio rate (number of audio frames per seconds)
    rate: f64,
    /// The last value computed by the tone filter
    filtered: f64,
}

impl Clipper {
    fn new(rate: usize) -> Clipper {
        Clipper {
            rate: rate as f64,
            filtered: 0.0,
        }
    }

    ///Compute the output corresponding to a sample
    fn process(&mut self, input: f64, conf: &Configuration) -> f64 {
        let clipped = (input * (1.0 + conf.drive.get_value())).tanh();

        let cutoff = conf.tone.get_value();
        let alpha = 1.0 - (-2.0 * std::f64::consts::PI * cutoff / self.rate).exp();
        self.filtered += alpha * (clipped - self.filtered);

        self.filtered * conf.volume.get_value()
    }
}

struct Distortion {
    /// The midi input to listen to
    midi_in: jack::Port<jack::MidiIn>,
    /// The input audio port
    audio_in: jack::Port<jack::AudioIn>,
    /// The output audio port
    audio_out: jack::Port<jack::AudioOut>,
    ///The incoming messages from the UI
    messages_in: std::sync::mpsc::Receiver<MessageToDistortion>,
    ///The outgoing messages to the UI
    messages_out: std::sync::mpsc::Sender<MessageToDistortionUI>,
    ///If defined, the next control will be mapped to this key
    key_change: Option<KeyBoardKey>,
    ///The keyboard events we are listening to
    keyboard: HardWare,
    /// The configuration
    conf: Configuration,
    /// The clipper transforming the signal
    clipper: Clipper,
}

impl Distortion {
    pub fn new(
        client: &jack::Client,
        messages_in: std::sync::mpsc::Receiver<MessageToDistortion>,
        mut messages_out: std::sync::mpsc::Sender<MessageToDistortionUI>,
    ) -> Result<Distortion, CommonError> {
        let m_in = match client.register_port("midi_in", jack::MidiIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiIn, e)),
        };
        let a_in = match client.register_port("audio_in", jack::AudioIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::AudioIn, e)),
        };
        let a_out = match client.register_port("audio_out", jack::AudioOut::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::AudioOut, e)),
        };

        let current_config = Configuration::new();

        //initialize the configuration on the UI side
        Self::send_message(
            MessageToDistortionUI::NewConfig(current_config.clone()),
            &mut messages_out,
        );

        Ok(Distortion {
            midi_in: m_in,
            audio_in: a_in,
            audio_out: a_out,
            messages_in,
            messages_out,
            key_change: None,
            keyboard: HardWare::new(),
            conf: current_config,
            clipper: Clipper::new(client.sample_rate() as usize),
        })
    }

    fn send_message(
        msg: MessageToDistortionUI,
        messages_out: &mut std::sync::mpsc::Sender<MessageToDistortionUI>,
    ) {
        if let Err(e) = messages_out.send(msg) {
            eprintln!("Internal error: {e}");
        }
    }
}

impl jack::ProcessHandler for Distortion {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        if let Ok(message) = self.messages_in.try_recv() {
            match message {
                MessageToDistortion::ChangeMidiKey(key) => self.key_change = Some(key),
                MessageToDistortion::ClearMidiKey(key) => self.keyboard.clear_key(key),
                MessageToDistortion::NewConfig(configuration) => self.conf = configuration,
            }
        }

        let show_p = self.midi_in.iter(ps);
        for e in show_p {
            let midi: MidiInput = e.into();
            if let MidiInput::Controller {
                channel: _,
                control,
                value,
            } = midi
            {
                if let Some(key) = self.keyboard.get_keyboard_key(control) {
                    if configuration::Configuration::apply_midi(&mut self.conf, key, value) {
                        Self::send_message(
                            MessageToDistortionUI::NewConfig(self.conf.clone()),
                            &mut self.messages_out,
                        );
                    }
                }

                if let Some(k) = self.key_change {
                    self.keyboard.update_key(k, control);
                    self.key_change = None;
                }
            }
        }

        let audio_in = self.audio_in.as_slice(ps);
        let audio_out = self.audio_out.as_mut_slice(ps);
        for (input, output) in audio_in.iter().zip(audio_out.iter_mut()) {
            *output = self.clipper.process(*input as f64, &self.conf) as f32;
        }

        jack::Control::Continue
    }
}

#[derive(Debug)]
enum MessageToDistortion {
    ChangeMidiKey(KeyBoardKey),
    ClearMidiKey(KeyBoardKey),
    NewConfig(Configuration),
}

impl From<KeyBoardKeySetter> for MessageToDistortion {
    fn from(value: KeyBoardKeySetter) -> Self {
        match value {
            KeyBoardKeySetter::Set(k) => MessageToDistortion::ChangeMidiKey(k),
            KeyBoardKeySetter::Clear(k) => MessageToDistortion::ClearMidiKey(k),
        }
    }
}

#[derive(Debug)]
enum MessageToDistortionUI {
    NewConfig(Configuration),
}

struct DistortionUI {
    messages_in: std::sync::mpsc::Receiver<MessageToDistortionUI>,
    message_out: std::sync::mpsc::Sender<MessageToDistortion>,
    messages: Vec<String>,
    current_config: Option<Configuration>,
}

impl DistortionUI {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        messages_in: std::sync::mpsc::Receiver<MessageToDistortionUI>,
        messages_out: std::sync::mpsc::Sender<MessageToDistortion>,
    ) -> DistortionUI {
        DistortionUI {
            messages_in,
            message_out: messages_out,
            messages: Vec::new(),
            current_config: None,
        }
    }

    fn create_menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Settings", |ui| {
                if let Some(conf) = &mut self.current_config {
                    configuration::Configuration::create_menu_keyboard_settings(
                        conf,
                        ui,
                        &mut self.message_out,
                        &mut self.messages,
                    );
                }
            });
            crate::utils::common_menu_luncher(ui, &mut self.messages);
        });
    }

    fn create_content(&mut self, ui: &mut egui::Ui) {
        if let Some(current_config) = &self.current_config {
            let mut conf = current_config.clone();

            configuration::Configuration::draw(&mut conf, ui);

            if !self.current_config.as_ref().eq(&Some(&conf)) {
                if let Err(e) = self
                    .message_out
                    .send(MessageToDistortion::NewConfig(conf.clone()))
                {
                    self.messages
                        .push(format!("Error while sending new conf: {e}"));
                }
                self.current_config = Some(conf);
            }
        }
        crate::utils::show_logs(ui, &mut self.messages);
    }

    fn read_input(&mut self) {
        //read message queue
        match self.messages_in.try_recv() {
            Err(e) => match e {
                std::sync::mpsc::TryRecvError::Empty => {}
                std::sync::mpsc::TryRecvError::Disconnected => self.messages.push(format!(
                    "Internal error: lost connection between UI and logic"
                )),
            },
            Ok(v) => match v {
                MessageToDistortionUI::NewConfig(cfg) => self.current_config = Some(cfg),
            },
        }
    }
}

impl eframe::App for DistortionUI {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        //the following line should only occur if a repaint is really needed.
        //To do this, we need to revise the architecture to make sure that it
        //can be called whenever there is someting in the queue
        ctx.request_repaint();

        self.read_input();
    }

    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            ui.vertical(|ui| {
                self.create_menu(ui);
                self.create_content(ui);
            });
        });
    }
}

pub fn distortion() -> Result<(), CommonError> {
    // open client
    let (client, _status) =
        match jack::Client::new("distortion", jack::ClientOptions::NO_START_SERVER) {
            Ok(v) => v,
            Err(e) => {
                return Err(CommonError::UnableToStartClient(e));
            }
        };

    //open a message channel for the distortion and the UI
    let (send_to_distortion, rcv_from_ui) = std::sync::mpsc::channel();
    let (send_to_ui, rcv_from_distortion) = std::sync::mpsc::channel();

    let effect = Distortion::new(&client, rcv_from_ui, send_to_ui)?;
    let active_client = match client.activate_async((), effect) {
        Ok(client) => client,
        Err(e) => return Err(CommonError::UnableToActivateTheClient(e)),
    };

    match eframe::run_native(
        "Distortion",
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size(egui::vec2(320.0, 640.0)),
            run_and_return: true,
            ..Default::default()
        },
        Box::new(|cc| {
            Ok(Box::new(DistortionUI::new(
                cc,
                rcv_from_distortion,
                send_to_distortion,
            )))
        }),
    ) {
        Ok(_) => {}
        Err(e) => return Err(CommonError::UnableToStartUserInterface(e)),
    }

    match active_client.deactivate() {
        Ok(_) => return Ok(()),
        Err(e) => {
            return Err(CommonError::UnableToDeActivateClient(e));
        }
    }
}

#[cfg(test)]
mod test {
    use super::{Clipper, Configuration, MAX_DRIVE, MAX_TONE};
    use crate::{configuration::FloatValueInRange, synth::hardware::KeyBoardKey};

    fn conf(drive: f64, tone: f64, volume: f64) -> Configuration {
        let key = KeyBoardKey::Parameter;
        Configuration {
            drive: FloatValueInRange::new(drive, 0.0, MAX_DRIVE, "drive", key),
            tone: FloatValueInRange::new(tone, 0.0, MAX_TONE, "tone", key),
            volume: FloatValueInRange::new(volume, 0.0, 1.0, "volume", key),
        }
    }

    ///Compute the output after the filter has settled on a constant input
    fn settled_output(input: f64, conf: &Configuration) -> f64 {
        let mut clipper = Clipper::new(48000);
        let mut output = 0.0;
        for _ in 0..48000 {
            output = clipper.process(input, conf);
        }
        output
    }

    #[test]
    fn drive_saturates_the_signal() {
        let quiet = settled_output(0.1, &conf(0.0, MAX_TONE, 1.0));
        let driven = settled_output(0.1, &conf(MAX_DRIVE, MAX_TONE, 1.0));
        assert!((quiet - 0.1).abs() < 0.001, "quiet: {quiet}");
        assert!(driven > 0.99, "driven: {driven}");
        assert!(driven <= 1.0, "driven: {driven}");
    }

    #[test]
    fn clipping_is_symmetrical() {
        let conf = conf(10.0, MAX_TONE, 1.0);
        let positive = settled_output(0.3, &conf);
        let negative = settled_output(-0.3, &conf);
        assert!((positive + negative).abs() < 1e-9);
    }

    #[test]
    fn volume_scales_the_output() {
        let full = settled_output(0.5, &conf(10.0, MAX_TONE, 1.0));
        let half = settled_output(0.5, &conf(10.0, MAX_TONE, 0.5));
        assert!((full / 2.0 - half).abs() < 1e-9);
    }

    #[test]
    fn tone_removes_high_frequencies() {
        let conf = conf(0.0, 500.0, 1.0);
        let mut clipper = Clipper::new(48000);
        let mut peak: f64 = 0.0;
        for i in 0..48000 {
            let input = if i % 2 == 0 { 0.5 } else { -0.5 };
            let output = clipper.process(input, &conf);
            if i > 24000 {
                peak = peak.max(output.abs());
            }
        }
        assert!(peak < 0.05, "peak: {peak}");
    }

    #[test]
    fn no_tone_is_silent() {
        assert_eq!(settled_output(0.5, &conf(10.0, 0.0, 1.0)), 0.0);
    }
}
