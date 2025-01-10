use eframe::egui::{self, ViewportBuilder};
use rand::random;

use crate::{
    configuration::{self, ConfigurationValue, FloatValueInRange, UsizeValueInRange},
    midiinput::MidiInput,
    synth::hardware::{HardWare, KeyBoardKey},
    utils::{CommonError, ConnectionType, KeyBoardKeySetter},
};

#[derive(Debug, Clone, PartialEq)]
struct Configuration {
    /// The number of frames needed to reach full volume
    attack: UsizeValueInRange,
    /// The number of frames to completely stop the sound
    decay: UsizeValueInRange,
    /// The duration of the snare, in frames
    duration: UsizeValueInRange,
    /// The volume of the snare
    volume: FloatValueInRange,
    /// The alpha value for the high pass filter
    alpha: FloatValueInRange,
}

impl Configuration {
    pub fn new(rate: usize) -> Configuration {
        Self {
            attack: UsizeValueInRange::new(50, 0, 128, "attack", KeyBoardKey::FadeInDuration),
            decay: UsizeValueInRange::new(50, 0, 128, "decay", KeyBoardKey::FadeOutDuration),
            duration: UsizeValueInRange::new(
                rate / 20,
                0,
                rate * 5,
                "duration",
                KeyBoardKey::Duration,
            ),
            volume: FloatValueInRange::new(0.5, 0.0, 10.0, "volume", KeyBoardKey::Gain),
            alpha: FloatValueInRange::new(0.2, 0.0, 1.0, "alpha", KeyBoardKey::Parameter),
        }
    }
}

impl<'c> configuration::Configuration<'c> for Configuration {
    fn elements(&'c mut self) -> Vec<ConfigurationValue<'c>> {
        vec![
            ConfigurationValue::USize(&mut self.attack),
            ConfigurationValue::USize(&mut self.decay),
            ConfigurationValue::USize(&mut self.duration),
            ConfigurationValue::Float(&mut self.volume),
            ConfigurationValue::Float(&mut self.alpha),
        ]
    }
}

struct Snare {
    /// The number of frames that still needs to be played
    nb_frames_left: Vec<usize>,
    /// Velocity of the last snare
    velocity: Vec<f64>,
    /// The midi input to listen to
    midi_in: jack::Port<jack::MidiIn>,
    /// The audio output
    audio_out: jack::Port<jack::AudioOut>,
    //The incoming messages from the UI
    messages_in: std::sync::mpsc::Receiver<MessageToSnare>,
    ///The outgoing messages to the UI
    messages_out: std::sync::mpsc::Sender<MessageToSnareUI>,
    ///If true, the next control will be used as key to start/stop the recording
    key_change: Option<KeyBoardKey>,
    ///The keyboard events we are listening to
    keyboard: HardWare,
    /// The configuration
    conf: Configuration,
    /// The last value pushed to the buffer
    last_output: Vec<f64>,
    /// The value before the last value pushed to the buffer
    last_input: Vec<f64>,
}

impl Snare {
    pub fn new(
        client: &jack::Client,
        messages_in: std::sync::mpsc::Receiver<MessageToSnare>,
        mut messages_out: std::sync::mpsc::Sender<MessageToSnareUI>,
    ) -> Result<Snare, CommonError> {
        let m_in = match client.register_port("midi_in", jack::MidiIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiIn, e)),
        };
        let a_out = match client.register_port("audio_out", jack::AudioOut::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiOut, e)),
        };

        let sample_rate = client.sample_rate();

        let current_config = Configuration::new(sample_rate);

        //initialize the configuration on the UI side
        Self::send_message(
            MessageToSnareUI::NewConfig(current_config.clone()),
            &mut messages_out,
        );

        let mut frames = Vec::with_capacity(128);
        let mut vel = Vec::with_capacity(128);
        let mut last = Vec::with_capacity(128);
        for _index in 0..128 {
            frames.push(0);
            vel.push(0.0);
            last.push(0.0);
        }
        Ok(Snare {
            nb_frames_left: frames,
            velocity: vel,
            midi_in: m_in,
            audio_out: a_out,
            messages_in,
            messages_out,
            key_change: None,
            keyboard: HardWare::new(),
            conf: current_config,
            last_output: last.clone(),
            last_input: last,
        })
    }

    fn send_message(
        msg: MessageToSnareUI,
        messages_out: &mut std::sync::mpsc::Sender<MessageToSnareUI>,
    ) {
        if let Err(e) = messages_out.send(msg) {
            eprintln!("Internal error: {e}");
        }
    }
}

impl jack::ProcessHandler for Snare {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        if let Ok(message) = self.messages_in.try_recv() {
            match message {
                MessageToSnare::ChangeActivationMidiKey(key) => self.key_change = Some(key),
                MessageToSnare::ClearActiviationMidiKey(key) => self.keyboard.clear_key(key),
                MessageToSnare::NewConfig(configuration) => self.conf = configuration,
            }
        }

        let total_frames = self.conf.decay.get_value()
            + self.conf.duration.get_value()
            + self.conf.attack.get_value();

        let show_p = self.midi_in.iter(ps);
        for e in show_p {
            let midi: MidiInput = e.into();
            match midi {
                MidiInput::Controller {
                    channel: _,
                    control,
                    value,
                } => {
                    if let Some(key) = self.keyboard.get_keyboard_key(control) {
                        if configuration::Configuration::apply_midi(&mut self.conf, key, value) {
                            Self::send_message(
                                MessageToSnareUI::NewConfig(self.conf.clone()),
                                &mut self.messages_out,
                            );
                        }
                    }

                    if let Some(k) = self.key_change {
                        self.keyboard.update_key(k, control);
                        self.key_change = None;
                    }
                }
                MidiInput::NoteStart {
                    channel: _,
                    note_index: _,
                    timing: _,
                    velocity,
                } => {
                    let mut added = false;
                    for index in 0..self.nb_frames_left.len() {
                        if self.nb_frames_left[index] == 0 {
                            self.nb_frames_left[index] = total_frames;
                            self.velocity[index] = velocity;
                            added = true;
                            break;
                        }
                    }
                    if !added {
                        self.nb_frames_left.push(total_frames);
                        self.velocity.push(velocity);
                    }
                }
                _ => {}
            }
        }

        let out = self.audio_out.as_mut_slice(ps);

        assert_eq!(self.nb_frames_left.len(), self.velocity.len());
        for output in out.iter_mut() {
            let mut v: f64 = 0.0;
            for snare_index in 0..self.nb_frames_left.len() {
                assert!(snare_index < self.nb_frames_left.len());
                assert!(snare_index < self.velocity.len());
                if self.nb_frames_left[snare_index] == 0 {
                    continue;
                }
                let volume = if self.nb_frames_left[snare_index]
                    > self.conf.decay.get_value() + self.conf.duration.get_value()
                {
                    let v = total_frames - self.nb_frames_left[snare_index];
                    (v as f64 / self.conf.attack.get_value() as f64) * self.conf.volume.get_value()
                } else if self.nb_frames_left[snare_index] > self.conf.decay.get_value() {
                    self.conf.volume.get_value()
                } else {
                    let v = self.nb_frames_left[snare_index];
                    (v as f64 / self.conf.decay.get_value() as f64) * self.conf.volume.get_value()
                };

                assert!(total_frames >= self.nb_frames_left[snare_index]);

                let x = 1.0 - (random::<f64>() * 2.0);

                let y = self.conf.alpha.get_value()
                    * (self.last_output[snare_index] + x - self.last_input[snare_index]);

                self.last_input[snare_index] = x;
                self.last_output[snare_index] = y;

                let value = y * self.velocity[snare_index] * volume;

                v += value;

                self.nb_frames_left[snare_index] = self.nb_frames_left[snare_index] - 1;
            }
            *output = v as f32;
        }

        jack::Control::Continue
    }
}

#[derive(Debug)]
enum MessageToSnare {
    ChangeActivationMidiKey(KeyBoardKey),
    ClearActiviationMidiKey(KeyBoardKey),
    NewConfig(Configuration),
}

impl From<KeyBoardKeySetter> for MessageToSnare {
    fn from(value: KeyBoardKeySetter) -> Self {
        match value {
            KeyBoardKeySetter::Set(k) => MessageToSnare::ChangeActivationMidiKey(k),
            KeyBoardKeySetter::Clear(k) => MessageToSnare::ClearActiviationMidiKey(k),
        }
    }
}

#[derive(Debug)]
enum MessageToSnareUI {
    NewConfig(Configuration),
}

struct SnareUI {
    messages_in: std::sync::mpsc::Receiver<MessageToSnareUI>,
    message_out: std::sync::mpsc::Sender<MessageToSnare>,
    messages: Vec<String>,
    current_config: Option<Configuration>,
}

impl SnareUI {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        messages_in: std::sync::mpsc::Receiver<MessageToSnareUI>,
        messages_out: std::sync::mpsc::Sender<MessageToSnare>,
    ) -> SnareUI {
        SnareUI {
            messages_in,
            message_out: messages_out,
            messages: Vec::new(),
            current_config: None,
        }
    }

    fn create_menu(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
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
                    .send(MessageToSnare::NewConfig(conf.clone()))
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
                MessageToSnareUI::NewConfig(cfg) => self.current_config = Some(cfg),
            },
        }
    }
}

impl eframe::App for SnareUI {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        //the following line should only occur if a repaint is really needed.
        //To do this, we need to revise the architecture to make sure that it
        //can be called whenever there is someting in the queue
        ctx.request_repaint();

        self.read_input();

        egui::CentralPanel::default().show(ctx, |ui| {
            ui.vertical(|ui| {
                self.create_menu(ui);
                self.create_content(ui);
            });
        });
    }
}

pub fn snare() -> Result<(), CommonError> {
    // open client
    let (client, _status) = match jack::Client::new("snare", jack::ClientOptions::NO_START_SERVER) {
        Ok(v) => v,
        Err(e) => {
            return Err(CommonError::UnableToStartClient(e));
        }
    };

    //open a message channel for the recorder and the UI
    let (send_to_rec, rcv_from_ui) = std::sync::mpsc::channel();
    let (send_to_ui, rcv_from_rec) = std::sync::mpsc::channel();

    let synth = Snare::new(&client, rcv_from_ui, send_to_ui)?;
    let active_client = match client.activate_async((), synth) {
        Ok(client) => client,
        Err(e) => return Err(CommonError::UnableToActivateTheClient(e)),
    };

    match eframe::run_native(
        "Snare",
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size(egui::vec2(320.0, 640.0)),
            run_and_return: true,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(SnareUI::new(cc, rcv_from_rec, send_to_rec)))),
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
