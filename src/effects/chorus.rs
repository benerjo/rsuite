use eframe::egui::{self, ViewportBuilder};

use crate::{
    configuration::{self, ConfigurationValue, FloatValueInRange, UsizeValueInRange},
    midiinput::MidiInput,
    nsm::{self, ConfigurationState, Session, Stateful},
    synth::{
        hardware::{HardWare, KeyBoardKey},
        wavetype::WaveType,
    },
    utils::{CommonError, ConnectionType, KeyBoardKeySetter},
};

///The maximum value of the base delay, in milliseconds
const MAX_DELAY: f64 = 50.0;
///The maximum value of the modulation depth, in milliseconds
const MAX_DEPTH: f64 = 20.0;
///The maximum value of the modulation speed, in hertz
const MAX_SPEED: f64 = 5.0;
///The maximum number of delayed copies of the signal
const MAX_VOICES: usize = 8;

#[derive(Debug, Clone, PartialEq)]
struct Configuration {
    /// The shortest delay of a voice, in milliseconds
    delay: FloatValueInRange,
    /// The amount of time added to the delay by the modulation, in milliseconds
    depth: FloatValueInRange,
    /// The speed of the modulation, in hertz
    speed: FloatValueInRange,
    /// The number of delayed copies of the signal. Without voice, only the input signal is heard
    voices: UsizeValueInRange,
    /// The proportion of delayed signal in the output, between 0 and 1
    mix: FloatValueInRange,
}

impl Configuration {
    pub fn new() -> Configuration {
        Self {
            delay: FloatValueInRange::new(20.0, 0.0, MAX_DELAY, "delay", KeyBoardKey::Duration),
            depth: FloatValueInRange::new(
                5.0,
                0.0,
                MAX_DEPTH,
                "depth",
                KeyBoardKey::ModulationIntensity,
            ),
            speed: FloatValueInRange::new(
                0.5,
                0.0,
                MAX_SPEED,
                "speed",
                KeyBoardKey::ModulationSpeed,
            ),
            voices: UsizeValueInRange::new(2, 0, MAX_VOICES, "voices", KeyBoardKey::Parameter),
            mix: FloatValueInRange::new(0.5, 0.0, 1.0, "mix", KeyBoardKey::ParameterB),
        }
    }
}

impl<'c> configuration::Configuration<'c> for Configuration {
    fn elements(&'c mut self) -> Vec<ConfigurationValue<'c>> {
        vec![
            ConfigurationValue::Float(&mut self.delay),
            ConfigurationValue::Float(&mut self.depth),
            ConfigurationValue::Float(&mut self.speed),
            ConfigurationValue::USize(&mut self.voices),
            ConfigurationValue::Float(&mut self.mix),
        ]
    }
}

///A delay line read by several voices, each one having its own modulated delay
struct DelayLine {
    /// The audio rate (number of audio frames per seconds)
    rate: f64,
    /// The last received samples
    buffer: Vec<f64>,
    /// The position of the last sample written to the buffer
    write_position: usize,
    /// The position in the modulation cycle, between 0 and 1
    phase: f64,
}

impl DelayLine {
    fn new(rate: usize) -> DelayLine {
        let max_delay = (MAX_DELAY + MAX_DEPTH) * rate as f64 / 1000.0;
        DelayLine {
            rate: rate as f64,
            //the additional frames are needed to interpolate at the longest delay
            buffer: vec![0.0; max_delay.ceil() as usize + 2],
            write_position: 0,
            phase: 0.0,
        }
    }

    ///Read the sample that was received a given number of frames ago,
    /// interpolating between the two closest samples
    fn read(&self, delay: f64) -> f64 {
        let len = self.buffer.len();
        let mut position = self.write_position as f64 - delay;
        if position < 0.0 {
            position += len as f64;
        }
        let fraction = position - position.floor();
        let previous = self.buffer[position.floor() as usize % len];
        let next = self.buffer[(position.floor() as usize + 1) % len];
        previous + (next - previous) * fraction
    }

    ///Add a sample to the delay line and compute the corresponding output
    fn process(&mut self, input: f64, conf: &Configuration) -> f64 {
        self.write_position = (self.write_position + 1) % self.buffer.len();
        self.buffer[self.write_position] = input;

        let voices = conf.voices.get_value();
        let mut wet = input;
        if voices > 0 {
            wet = 0.0;
            for voice in 0..voices {
                //the voices are evenly spread over the modulation cycle
                let phase = self.phase + voice as f64 / voices as f64;
                let modulation =
                    (1.0 + WaveType::Sin.compute(phase * 2.0 * std::f64::consts::PI)) / 2.0;
                let delay = conf.delay.get_value() + conf.depth.get_value() * modulation;
                wet += self.read(delay * self.rate / 1000.0);
            }
            wet /= voices as f64;
        }

        self.phase += conf.speed.get_value() / self.rate;
        if self.phase >= 1.0 {
            self.phase -= 1.0;
        }

        let mix = conf.mix.get_value();
        input * (1.0 - mix) + wet * mix
    }
}

struct Chorus {
    /// The midi input to listen to
    midi_in: jack::Port<jack::MidiIn>,
    /// The input audio port
    audio_in: jack::Port<jack::AudioIn>,
    /// The output audio port
    audio_out: jack::Port<jack::AudioOut>,
    ///The incoming messages from the UI
    messages_in: std::sync::mpsc::Receiver<MessageToChorus>,
    ///The outgoing messages to the UI
    messages_out: std::sync::mpsc::Sender<MessageToChorusUI>,
    ///If defined, the next control will be mapped to this key
    key_change: Option<KeyBoardKey>,
    ///The keyboard events we are listening to
    keyboard: HardWare,
    /// The configuration
    conf: Configuration,
    /// The delay line generating the voices
    delay_line: DelayLine,
}

impl Chorus {
    pub fn new(
        client: &jack::Client,
        messages_in: std::sync::mpsc::Receiver<MessageToChorus>,
        mut messages_out: std::sync::mpsc::Sender<MessageToChorusUI>,
        state: ConfigurationState<Configuration>,
    ) -> Result<Chorus, CommonError> {
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

        let current_config = state.conf;

        //initialize the configuration on the UI side
        Self::send_message(
            MessageToChorusUI::NewConfig(current_config.clone()),
            &mut messages_out,
        );

        Ok(Chorus {
            midi_in: m_in,
            audio_in: a_in,
            audio_out: a_out,
            messages_in,
            messages_out,
            key_change: None,
            keyboard: state.keyboard,
            conf: current_config,
            delay_line: DelayLine::new(client.sample_rate() as usize),
        })
    }

    fn send_message(
        msg: MessageToChorusUI,
        messages_out: &mut std::sync::mpsc::Sender<MessageToChorusUI>,
    ) {
        if let Err(e) = messages_out.send(msg) {
            eprintln!("Internal error: {e}");
        }
    }
}

impl Stateful for Chorus {
    type State = ConfigurationState<Configuration>;

    fn publish(&self, state: &mut Self::State) {
        state.update(&self.conf, &self.keyboard);
    }
}

impl jack::ProcessHandler for Chorus {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        if let Ok(message) = self.messages_in.try_recv() {
            match message {
                MessageToChorus::ChangeMidiKey(key) => self.key_change = Some(key),
                MessageToChorus::ClearMidiKey(key) => self.keyboard.clear_key(key),
                MessageToChorus::NewConfig(configuration) => self.conf = configuration,
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
                            MessageToChorusUI::NewConfig(self.conf.clone()),
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
            *output = self.delay_line.process(*input as f64, &self.conf) as f32;
        }

        jack::Control::Continue
    }
}

#[derive(Debug)]
enum MessageToChorus {
    ChangeMidiKey(KeyBoardKey),
    ClearMidiKey(KeyBoardKey),
    NewConfig(Configuration),
}

impl From<KeyBoardKeySetter> for MessageToChorus {
    fn from(value: KeyBoardKeySetter) -> Self {
        match value {
            KeyBoardKeySetter::Set(k) => MessageToChorus::ChangeMidiKey(k),
            KeyBoardKeySetter::Clear(k) => MessageToChorus::ClearMidiKey(k),
        }
    }
}

#[derive(Debug)]
enum MessageToChorusUI {
    NewConfig(Configuration),
}

struct ChorusUI {
    messages_in: std::sync::mpsc::Receiver<MessageToChorusUI>,
    message_out: std::sync::mpsc::Sender<MessageToChorus>,
    messages: Vec<String>,
    current_config: Option<Configuration>,
}

impl ChorusUI {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        messages_in: std::sync::mpsc::Receiver<MessageToChorusUI>,
        messages_out: std::sync::mpsc::Sender<MessageToChorus>,
    ) -> ChorusUI {
        nsm::set_ui_context(&cc.egui_ctx);
        ChorusUI {
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
                    .send(MessageToChorus::NewConfig(conf.clone()))
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
                MessageToChorusUI::NewConfig(cfg) => self.current_config = Some(cfg),
            },
        }
    }
}

impl eframe::App for ChorusUI {
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

pub fn chorus() -> Result<(), CommonError> {
    // open client
    let session = Session::start("Chorus")?;
    let client = session.client("chorus")?;

    //open a message channel for the chorus and the UI
    let (send_to_chorus, rcv_from_ui) = std::sync::mpsc::channel();
    let (send_to_ui, rcv_from_chorus) = std::sync::mpsc::channel();

    let state = session.load(ConfigurationState::new(Configuration::new()));
    let effect = Chorus::new(&client, rcv_from_ui, send_to_ui, state.clone())?;
    let title = session.window_title("Chorus");
    let active_client = session.activate(client, effect, state)?;

    nsm::run_window(
        &title,
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size(egui::vec2(320.0, 640.0)),
            run_and_return: true,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(ChorusUI::new(cc, rcv_from_chorus, send_to_chorus)))),
    )?;

    match active_client.deactivate() {
        Ok(_) => return Ok(()),
        Err(e) => {
            return Err(CommonError::UnableToDeActivateClient(e));
        }
    }
}

#[cfg(test)]
mod test {
    use super::{Configuration, DelayLine, MAX_DELAY, MAX_DEPTH, MAX_SPEED, MAX_VOICES};
    use crate::{
        configuration::{FloatValueInRange, UsizeValueInRange},
        nsm::{ConfigurationState, Persistent, Values},
        synth::hardware::KeyBoardKey,
    };

    #[test]
    fn state_is_restored() {
        let mut state = ConfigurationState::new(Configuration::new());
        state.conf.delay.set_value(12.5);
        state.conf.voices.set_value(5);
        state.keyboard.update_key(KeyBoardKey::Parameter, 20);
        let mut values = Values::new();
        state.save(&mut values);

        let mut loaded = ConfigurationState::new(Configuration::new());
        assert_ne!(loaded, state);
        loaded.load(&values);
        assert_eq!(loaded, state);

        //values outside of the range are limited to the range
        values.set("value.delay", 1000.0);
        loaded.load(&values);
        assert_eq!(loaded.conf.delay.get_value(), MAX_DELAY);
    }

    fn conf(delay: f64, depth: f64, speed: f64, voices: usize, mix: f64) -> Configuration {
        let key = KeyBoardKey::Parameter;
        Configuration {
            delay: FloatValueInRange::new(delay, 0.0, MAX_DELAY, "delay", key),
            depth: FloatValueInRange::new(depth, 0.0, MAX_DEPTH, "depth", key),
            speed: FloatValueInRange::new(speed, 0.0, MAX_SPEED, "speed", key),
            voices: UsizeValueInRange::new(voices, 0, MAX_VOICES, "voices", key),
            mix: FloatValueInRange::new(mix, 0.0, 1.0, "mix", key),
        }
    }

    #[test]
    fn no_mix_keeps_the_input() {
        let conf = conf(20.0, 5.0, 1.0, 4, 0.0);
        let mut line = DelayLine::new(1000);
        for i in 0..200 {
            assert_eq!(line.process(i as f64, &conf), i as f64);
        }
    }

    #[test]
    fn no_voice_keeps_the_input() {
        let conf = conf(20.0, 5.0, 1.0, 0, 1.0);
        let mut line = DelayLine::new(1000);
        for i in 0..200 {
            assert_eq!(line.process(i as f64, &conf), i as f64);
        }
    }

    #[test]
    fn delay_without_modulation() {
        let conf = conf(10.0, 0.0, 0.0, 1, 1.0);
        let mut line = DelayLine::new(1000);
        for i in 0..50 {
            let input = if i == 0 { 1.0 } else { 0.0 };
            let expected = if i == 10 { 1.0 } else { 0.0 };
            assert_eq!(line.process(input, &conf), expected);
        }
    }

    #[test]
    fn modulation_stays_within_the_delay_range() {
        let conf = conf(10.0, 5.0, 5.0, 1, 1.0);
        let mut line = DelayLine::new(1000);
        let mut min_delay = f64::MAX;
        let mut max_delay = f64::MIN;
        for i in 0..2000 {
            let output = line.process(i as f64, &conf);
            if i >= 100 {
                let delay = i as f64 - output;
                min_delay = min_delay.min(delay);
                max_delay = max_delay.max(delay);
            }
        }
        assert!((min_delay - 10.0).abs() < 0.01, "min delay: {min_delay}");
        assert!((max_delay - 15.0).abs() < 0.01, "max delay: {max_delay}");
    }

    #[test]
    fn longest_settings_keep_the_volume() {
        let conf = conf(MAX_DELAY, MAX_DEPTH, MAX_SPEED, MAX_VOICES, 1.0);
        let mut line = DelayLine::new(48000);
        for i in 0..200000 {
            let output = line.process(1.0, &conf);
            if i > 48000 {
                assert!((output - 1.0).abs() < 1e-9, "output: {output}");
            }
        }
    }
}
