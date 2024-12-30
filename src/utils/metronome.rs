use std::ops::RangeInclusive;

use eframe::egui::{self, ViewportBuilder};

use crate::{
    midiinput::MidiInput,
    synth::{
        hardware::{HardWare, KeyBoardKey},
        wavetype::WaveType,
    },
};

use super::{CommonError, ConnectionType, KeyBoardKeySetter};

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

struct Metronome {
    configuration: MetronomeConfiguration,
    ///The keyboard events we are listening to
    keyboard: HardWare,
    /// The midi input to activate the metronome
    midi_in: jack::Port<jack::MidiIn>,
    /// The output audio port
    audio_mono_out: jack::Port<jack::AudioOut>,
    ///The incoming messages from the UI
    messages_in: std::sync::mpsc::Receiver<MessageToMetronome>,
    ///The outgoing messages to the UI
    messages_out: std::sync::mpsc::Sender<MessageToMetronomeUI>,
    ///The audio rate (number of audio frames per seconds)
    rate: usize,
    ///The number of frames passed since the start of the first beat
    time: usize,
    ///Duration left of the sound
    sound_left: usize,
    ///The duration of a sound
    sound_duration: usize,
    /// The beat number
    beat_nb: usize,
    /// The next key to map
    next_key_map: Option<KeyBoardKey>,
}

impl Metronome {
    fn new(
        client: &jack::Client,
        messages_in: std::sync::mpsc::Receiver<MessageToMetronome>,
        messages_out: std::sync::mpsc::Sender<MessageToMetronomeUI>,
    ) -> Result<Metronome, CommonError> {
        let a_out = match client.register_port("audio_out", jack::AudioOut::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::AudioOut, e)),
        };
        let m_in = match client.register_port("midi_in", jack::MidiIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiIn, e)),
        };

        Ok(Metronome {
            configuration: MetronomeConfiguration::default(),
            keyboard: HardWare::new(),
            midi_in: m_in,
            audio_mono_out: a_out,
            messages_in,
            messages_out,
            rate: client.sample_rate(),
            time: 0,
            sound_left: client.sample_rate() / 10,
            sound_duration: client.sample_rate() / 10,
            beat_nb: 0,
            next_key_map: None,
        })
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

        // Get output buffer
        let out = self.audio_mono_out.as_mut_slice(ps);

        // Write output
        for v in out.iter_mut() {
            let amplitude = if self.sound_left > 0 {
                let fade = if self.sound_left > self.sound_duration / 2 {
                    (self.sound_duration as f64) - (self.sound_left as f64)
                } else {
                    self.sound_left as f64
                };
                let result = if self.time + self.sound_left <= self.sound_duration {
                    //first beat
                    let t = (self.time as f64) / (self.rate as f64);
                    fade * WaveType::Sin.compute(880.0 * t * 2.0 * std::f64::consts::PI)
                } else {
                    let t = (self.sound_duration - self.sound_left) as f64 / (self.rate as f64);
                    fade * WaveType::Sin.compute(220.0 * t * 2.0 * std::f64::consts::PI)
                };
                self.time += 1;
                if self.sound_left > 0 {
                    self.sound_left -= 1;
                }
                result
            } else {
                if self.time % (self.rate * 60 / self.configuration.bpm) == 0 {
                    self.sound_left = self.sound_duration;
                    self.beat_nb = (self.beat_nb + 1) % self.configuration.nb_notes;
                    if self.beat_nb == 0 {
                        self.time = 0;
                    } else {
                        self.time += 1;
                    }
                } else {
                    self.time += 1;
                }

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
        _cc: &eframe::CreationContext<'_>,
        messages_in: std::sync::mpsc::Receiver<MessageToMetronomeUI>,
        messages_out: std::sync::mpsc::Sender<MessageToMetronome>,
    ) -> MetronomeUI {
        MetronomeUI {
            messages: Vec::with_capacity(16),
            conf: MetronomeConfiguration::default(),
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

        crate::utils::create_usize_slider(
            ui,
            "Beats per minutes",
            &mut self.conf.bpm,
            RangeInclusive::new(60, 240),
        );

        crate::utils::create_usize_slider(
            ui,
            "Nb Quarter notes per bar",
            &mut self.conf.nb_notes,
            RangeInclusive::new(2, 10),
        );

        if self.conf != current_conf {
            self.send_message(MessageToMetronome::NewConfiguration(self.conf.clone()));
        }

        crate::utils::show_logs(ui, &mut self.messages);
    }

    fn create_menu(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("Settings", |ui| {
                if ui
                    .toggle_value(&mut self.conf.active, String::from("Active"))
                    .clicked()
                {
                    self.send_message(MessageToMetronome::Active(self.conf.active));
                    ui.close_menu();
                }

                crate::utils::create_keyboard_select(
                    ui,
                    "Keyboard activate key",
                    KeyBoardKey::Activate,
                    &mut self.messages_out,
                    &mut self.messages,
                );
                crate::utils::create_keyboard_select(
                    ui,
                    "Tempo",
                    KeyBoardKey::Tempo,
                    &mut self.messages_out,
                    &mut self.messages,
                );
            });

            crate::utils::common_menu_luncher(ui, &mut self.messages);
        });
    }

    fn send_message(&mut self, msg: MessageToMetronome) {
        if let Err(e) = self.messages_out.send(msg) {
            self.messages.push(format!("Internal error: {e}"));
        }
    }
}

impl eframe::App for MetronomeUI {
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

pub fn metronome() -> Result<(), CommonError> {
    // open client
    let (client, _status) =
        match jack::Client::new("metronome", jack::ClientOptions::NO_START_SERVER) {
            Ok(v) => v,
            Err(e) => {
                return Err(CommonError::UnableToStartClient(e));
            }
        };

    //open a message channel for the recorder and the UI
    let (send_to_rec, rcv_from_ui) = std::sync::mpsc::channel();
    let (send_to_ui, rcv_from_rec) = std::sync::mpsc::channel();

    let util = Metronome::new(&client, rcv_from_ui, send_to_ui)?;
    let active_client = match client.activate_async((), util) {
        Ok(client) => client,
        Err(e) => return Err(CommonError::UnableToActivateTheClient(e)),
    };

    match eframe::run_native(
        "Recorder",
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size(egui::vec2(320.0, 640.0)),
            run_and_return: true,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(MetronomeUI::new(cc, rcv_from_rec, send_to_rec)))),
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
