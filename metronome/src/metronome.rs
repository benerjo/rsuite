use std::ops::RangeInclusive;

use eframe::egui::{self, ViewportBuilder};

use rsuite::{
    midiinput::{MidiInput, ALL_DEVICES, BAR_NOT_RUNNING, BAR_UNKNOWN},
    nsm::{self, sync, Persistent, Session, Stateful, Values},
    synth::{
        hardware::{HardWare, KeyBoardKey},
        wavetype::WaveType,
    },
};

use rsuite::utils::{CommonError, ConnectionType, KeyBoardKeySetter};

///The number of midi clock messages per quarter note
const CLOCKS_PER_QUARTER: usize = 24;
///The length of the longest midi message sent by the metronome (the time signature)
const MAX_MESSAGE_LEN: usize = 11;

#[derive(Clone, Debug, PartialEq, Eq)]
struct MetronomeConfiguration {
    ///The number of quarters per minutes
    bpm: usize,
    ///The number of quarter notes per bar
    nb_notes: usize,
    ///Specify if the metronome is active
    active: bool,
}

impl Default for MetronomeConfiguration {
    fn default() -> Self {
        Self {
            bpm: 110,
            nb_notes: 4,
            active: true,
        }
    }
}

///The state saved by the session manager
#[derive(Debug, Clone, PartialEq)]
struct MetronomeState {
    configuration: MetronomeConfiguration,
    keyboard: HardWare,
}

impl Persistent for MetronomeState {
    fn save(&self, values: &mut Values) {
        values.set("bpm", self.configuration.bpm);
        values.set("nb_notes", self.configuration.nb_notes);
        values.set("active", self.configuration.active);
        self.keyboard.save(values);
    }

    fn load(&mut self, values: &Values) {
        values.load("bpm", &mut self.configuration.bpm);
        values.load("nb_notes", &mut self.configuration.nb_notes);
        values.load("active", &mut self.configuration.active);
        self.keyboard.load(values);
    }
}

struct Metronome {
    configuration: MetronomeConfiguration,
    ///The keyboard events we are listening to
    keyboard: HardWare,
    /// The midi input to activate the metronome
    midi_in: jack::Port<jack::MidiIn>,
    /// The midi output sending the clock and the start/stop messages
    midi_out: jack::Port<jack::MidiOut>,
    /// The output audio port
    audio_mono_out: jack::Port<jack::AudioOut>,
    ///The incoming messages from the UI
    messages_in: std::sync::mpsc::Receiver<MessageToMetronome>,
    ///The outgoing messages to the UI
    messages_out: std::sync::mpsc::Sender<MessageToMetronomeUI>,
    ///The audio rate (number of audio frames per seconds)
    rate: usize,
    ///The number of frames left before the next clock message
    frames_to_next_clock: f64,
    ///The position of the next clock message in the current quarter note
    clock_nb: usize,
    ///If the start message was sent without being followed by a stop message
    running: bool,
    ///Duration left of the sound
    sound_left: usize,
    ///The duration of a sound
    sound_duration: usize,
    ///The frequency of the current sound
    sound_frequency: f64,
    /// The beat number
    beat_nb: usize,
    /// The number of the next bar, starting from 1
    bar_nb: i16,
    /// The number of quarters per bar of the last time signature sent
    time_signature_sent: Option<usize>,
    /// The next key to map
    next_key_map: Option<KeyBoardKey>,
}

impl Metronome {
    fn new(
        client: &jack::Client,
        messages_in: std::sync::mpsc::Receiver<MessageToMetronome>,
        messages_out: std::sync::mpsc::Sender<MessageToMetronomeUI>,
        state: MetronomeState,
    ) -> Result<Metronome, CommonError> {
        let a_out = match client.register_port("audio_out", jack::AudioOut::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::AudioOut, e)),
        };
        let m_in = match client.register_port("midi_in", jack::MidiIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiIn, e)),
        };
        let m_out = match client.register_port("midi_out", jack::MidiOut::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiOut, e)),
        };

        let sample_rate = client.sample_rate() as usize;

        Ok(Metronome {
            configuration: state.configuration,
            keyboard: state.keyboard,
            midi_in: m_in,
            midi_out: m_out,
            audio_mono_out: a_out,
            messages_in,
            messages_out,
            rate: sample_rate,
            frames_to_next_clock: 0.0,
            clock_nb: 0,
            running: false,
            sound_left: 0,
            sound_duration: sample_rate / 10,
            sound_frequency: 0.0,
            beat_nb: 0,
            bar_nb: 1,
            time_signature_sent: None,
            next_key_map: None,
        })
    }

    fn send_midi(writer: &mut jack::MidiWriter, message: MidiInput) {
        let mut bytes = [0; MAX_MESSAGE_LEN];
        if let Err(e) = writer.write(&message.to_raw(&mut bytes)) {
            eprintln!("Internal error: {e}");
        }
    }
}

impl Stateful for Metronome {
    type State = MetronomeState;

    fn publish(&self, state: &mut Self::State) {
        sync(&mut state.configuration, &self.configuration);
        sync(&mut state.keyboard, &self.keyboard);
    }
}

impl jack::ProcessHandler for Metronome {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        if let Ok(message) = self.messages_in.try_recv() {
            match message {
                MessageToMetronome::NewConfiguration(conf) => self.configuration = conf,
                MessageToMetronome::Active(active) => self.configuration.active = active,
                MessageToMetronome::SetKey(key_board_key) => {
                    self.next_key_map = Some(key_board_key)
                }
                MessageToMetronome::ClearKey(key_board_key) => {
                    self.keyboard.clear_key(key_board_key)
                }
            }
        }

        let current_conf = self.configuration.clone();

        let show_p = self.midi_in.iter(ps);
        for e in show_p {
            let midi = e.into();

            if let MidiInput::Controller {
                control,
                value,
                channel: _,
            } = midi
            {
                if self.next_key_map.is_some() && value > 0 {
                    self.keyboard
                        .update_key(self.next_key_map.unwrap(), control);
                    self.next_key_map = None;
                } else if value > 0 {
                    match self.keyboard.get_keyboard_key(control) {
                        Some(KeyBoardKey::Activate) => {
                            self.configuration.active = !self.configuration.active;
                        }
                        Some(KeyBoardKey::Tempo) => {
                            self.configuration.bpm = 60 + value as usize * (240 - 60) / 128;
                        }
                        _ => {}
                    }
                }
            }
        }

        let mut midi_out = self.midi_out.writer(ps);

        if self.configuration.active != self.running {
            self.running = self.configuration.active;
            if self.running {
                Metronome::send_midi(&mut midi_out, MidiInput::Start { timing: 0 });
                //the clock following a start message is the first beat of the bar
                self.frames_to_next_clock = 0.0;
                self.clock_nb = 0;
                self.beat_nb = 0;
                self.bar_nb = 1;
                self.time_signature_sent = None;
            } else {
                Metronome::send_midi(&mut midi_out, MidiInput::Stop { timing: 0 });
                Metronome::send_midi(
                    &mut midi_out,
                    MidiInput::BarMarker {
                        timing: 0,
                        device_id: ALL_DEVICES,
                        bar: BAR_NOT_RUNNING,
                    },
                );
            }
        }

        //the time signature is sent when starting and whenever it changes
        let nb_notes = self.configuration.nb_notes;
        if self.running && self.time_signature_sent != Some(nb_notes) {
            Metronome::send_midi(
                &mut midi_out,
                MidiInput::TimeSignature {
                    timing: 0,
                    device_id: ALL_DEVICES,
                    delayed: false,
                    numerator: nb_notes as u8,
                    //quarter notes
                    denominator_power: 2,
                    clocks_per_click: CLOCKS_PER_QUARTER as u8,
                    thirty_seconds_per_quarter: 8,
                },
            );
            self.time_signature_sent = Some(nb_notes);
        }

        // Get output buffer
        let out = self.audio_mono_out.as_mut_slice(ps);

        // Write output
        for (frame, v) in out.iter_mut().enumerate() {
            if self.frames_to_next_clock <= 0.0 {
                //the bar marker comes right before the first clock of the bar
                if self.running && self.clock_nb == 0 && self.beat_nb == 0 {
                    Metronome::send_midi(
                        &mut midi_out,
                        MidiInput::BarMarker {
                            timing: frame as jack::Frames,
                            device_id: ALL_DEVICES,
                            bar: self.bar_nb,
                        },
                    );
                    //once the bar number cannot be represented, it is sent as unknown
                    self.bar_nb = (self.bar_nb + 1).min(BAR_UNKNOWN);
                }
                Metronome::send_midi(
                    &mut midi_out,
                    MidiInput::Clock {
                        timing: frame as jack::Frames,
                    },
                );
                if self.clock_nb == 0 {
                    self.sound_left = self.sound_duration;
                    self.sound_frequency = if self.beat_nb == 0 { 880.0 } else { 220.0 };
                    self.beat_nb = (self.beat_nb + 1) % self.configuration.nb_notes;
                }
                self.clock_nb = (self.clock_nb + 1) % CLOCKS_PER_QUARTER;
                self.frames_to_next_clock +=
                    (self.rate * 60) as f64 / (self.configuration.bpm * CLOCKS_PER_QUARTER) as f64;
            }
            self.frames_to_next_clock -= 1.0;

            let amplitude = if self.sound_left > 0 {
                let fade = if self.sound_left > self.sound_duration / 2 {
                    (self.sound_duration as f64) - (self.sound_left as f64)
                } else {
                    self.sound_left as f64
                };
                let t = (self.sound_duration - self.sound_left) as f64 / (self.rate as f64);
                let result = fade
                    * WaveType::Sin.compute(self.sound_frequency * t * 2.0 * std::f64::consts::PI);
                self.sound_left -= 1;
                result
            } else {
                0.0
            };

            if self.configuration.active {
                *v = amplitude as f32;
            } else {
                *v = 0.0;
            }
        }

        if self.configuration != current_conf {
            if let Err(e) = self
                .messages_out
                .send(MessageToMetronomeUI::NewConfiguration(
                    self.configuration.clone(),
                ))
            {
                eprintln!("Internal error: {e}");
            }
        }

        jack::Control::Continue
    }
}

enum MessageToMetronome {
    Active(bool),
    NewConfiguration(MetronomeConfiguration),
    SetKey(KeyBoardKey),
    ClearKey(KeyBoardKey),
}

impl From<KeyBoardKeySetter> for MessageToMetronome {
    fn from(value: KeyBoardKeySetter) -> Self {
        match value {
            KeyBoardKeySetter::Set(k) => MessageToMetronome::SetKey(k),
            KeyBoardKeySetter::Clear(k) => MessageToMetronome::ClearKey(k),
        }
    }
}

enum MessageToMetronomeUI {
    NewConfiguration(MetronomeConfiguration),
}

struct MetronomeUI {
    messages: Vec<String>,
    conf: MetronomeConfiguration,
    messages_in: std::sync::mpsc::Receiver<MessageToMetronomeUI>,
    messages_out: std::sync::mpsc::Sender<MessageToMetronome>,
}

impl MetronomeUI {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        messages_in: std::sync::mpsc::Receiver<MessageToMetronomeUI>,
        messages_out: std::sync::mpsc::Sender<MessageToMetronome>,
        conf: MetronomeConfiguration,
    ) -> MetronomeUI {
        nsm::set_ui_context(&cc.egui_ctx);
        MetronomeUI {
            messages: Vec::with_capacity(16),
            conf,
            messages_in,
            messages_out,
        }
    }

    fn read_input(&mut self) {
        while let Ok(m) = self.messages_in.try_recv() {
            match m {
                MessageToMetronomeUI::NewConfiguration(c) => self.conf = c,
            }
        }
    }

    fn create_content(&mut self, ui: &mut egui::Ui) {
        let current_conf = self.conf.clone();

        if ui
            .button(if self.conf.active {
                "De-activate"
            } else {
                "Activate"
            })
            .clicked()
        {
            self.conf.active = !self.conf.active;
        }

        rsuite::utils::create_usize_slider(
            ui,
            "Beats per minutes",
            &mut self.conf.bpm,
            RangeInclusive::new(60, 240),
        );

        rsuite::utils::create_usize_slider(
            ui,
            "Nb Quarter notes per bar",
            &mut self.conf.nb_notes,
            RangeInclusive::new(2, 10),
        );

        if self.conf != current_conf {
            self.send_message(MessageToMetronome::NewConfiguration(self.conf.clone()));
        }

        rsuite::utils::show_logs(ui, &mut self.messages);
    }

    fn create_menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Settings", |ui| {
                if ui
                    .toggle_value(&mut self.conf.active, String::from("Active"))
                    .clicked()
                {
                    self.send_message(MessageToMetronome::Active(self.conf.active));
                    ui.close();
                }

                rsuite::utils::create_keyboard_select(
                    ui,
                    "Keyboard activate key",
                    KeyBoardKey::Activate,
                    &mut self.messages_out,
                    &mut self.messages,
                );
                rsuite::utils::create_keyboard_select(
                    ui,
                    "Tempo",
                    KeyBoardKey::Tempo,
                    &mut self.messages_out,
                    &mut self.messages,
                );
            });

            rsuite::utils::common_menu_luncher(ui, &mut self.messages);
        });
    }

    fn send_message(&mut self, msg: MessageToMetronome) {
        if let Err(e) = self.messages_out.send(msg) {
            self.messages.push(format!("Internal error: {e}"));
        }
    }
}

impl eframe::App for MetronomeUI {
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

pub fn metronome() -> Result<(), CommonError> {
    // open client
    let session = Session::start("Metronome")?;
    let client = session.client("metronome")?;

    //open a message channel for the recorder and the UI
    let (send_to_rec, rcv_from_ui) = std::sync::mpsc::channel();
    let (send_to_ui, rcv_from_rec) = std::sync::mpsc::channel();

    let state = session.load(MetronomeState {
        configuration: MetronomeConfiguration::default(),
        keyboard: HardWare::new(),
    });
    let conf = state.configuration.clone();
    let util = Metronome::new(&client, rcv_from_ui, send_to_ui, state.clone())?;
    let title = session.window_title("Metronome");
    let active_client = session.activate(client, util, state)?;

    nsm::run_window(
        &title,
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size(egui::vec2(320.0, 640.0)),
            run_and_return: true,
            ..Default::default()
        },
        Box::new(|cc| {
            Ok(Box::new(MetronomeUI::new(
                cc,
                rcv_from_rec,
                send_to_rec,
                conf,
            )))
        }),
    )?;

    match active_client.deactivate() {
        Ok(_) => return Ok(()),
        Err(e) => {
            return Err(CommonError::UnableToDeActivateClient(e));
        }
    }
}
