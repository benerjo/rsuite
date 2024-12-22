use eframe::egui::{self, ViewportBuilder};

use crate::{
    midiinput::MidiInput,
    synth::hardware::{HardWare, KeyBoardKey},
    utils::{CommonError, ConnectionType},
};

use super::KeyBoardKeySetter;

struct Recorder {
    /// If false, the recorder will not listen to record events
    active: bool,
    /// The midi input to activate the recording
    midi_in: jack::Port<jack::MidiIn>,
    /// The input audio port
    audio_mono_in: jack::Port<jack::AudioIn>,
    ///The sample rate of the audio
    rate: usize,
    ///The buffer containing the current recording
    record_buffer: Vec<i16>,
    ///If true, we are currently recording
    recording: bool,
    ///The incoming messages from the UI
    messages_in: std::sync::mpsc::Receiver<MessageToRecorder>,
    ///The outgoing messages to the UI
    messages_out: std::sync::mpsc::Sender<MessageToRecorderUI>,
    ///The prefix of the audio file
    audio_prefix: String,
    ///If true, the next control will be used as key to start/stop the recording
    key_change: bool,
    ///The keyboard events we are listening to
    keyboard: HardWare,
}

impl Recorder {
    pub fn new(
        client: &jack::Client,
        messages_in: std::sync::mpsc::Receiver<MessageToRecorder>,
        messages_out: std::sync::mpsc::Sender<MessageToRecorderUI>,
    ) -> Result<Recorder, CommonError> {
        let sample_rate = client.sample_rate();
        let a_in = match client.register_port("music_in", jack::AudioIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::AudioIn, e)),
        };
        let m_in = match client.register_port("midi_in", jack::MidiIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiIn, e)),
        };

        Ok(Recorder {
            active: true,
            rate: sample_rate,
            midi_in: m_in,
            audio_mono_in: a_in,
            record_buffer: Vec::with_capacity(sample_rate * 60 * 4), //have a buffer for 4 min of music
            recording: false,
            messages_in,
            messages_out,
            audio_prefix: String::from("Rec"),
            key_change: false,
            keyboard: HardWare::new(),
        })
    }
}

impl jack::ProcessHandler for Recorder {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        if let Ok(message) = self.messages_in.try_recv() {
            match message {
                MessageToRecorder::StartRecording => {
                    if !self.recording && self.active {
                        self.recording = true
                    }
                }
                MessageToRecorder::StopRecordeing => {
                    if self.recording && self.active {
                        self.recording = false;
                        let mut tmp_buf = Vec::<i16>::with_capacity(self.record_buffer.capacity());
                        std::mem::swap(&mut self.record_buffer, &mut tmp_buf);
                        if let Err(e) = crate::wavwriter::save_wav(
                            tmp_buf,
                            self.rate as u32,
                            Some(&self.audio_prefix),
                        ) {
                            println!("Error while saving the wav file: {e}");
                        }
                    }
                }
                MessageToRecorder::NewPrefix(prefix) => self.audio_prefix = prefix,
                MessageToRecorder::ChangeRecord => self.key_change = true,
                MessageToRecorder::DiscardRecordKey => {
                    self.key_change = false;
                    self.keyboard.clear_key(KeyBoardKey::Record);
                }
                MessageToRecorder::Active(value) => self.active = value,
            }
        }

        let show_p = self.midi_in.iter(ps);
        for e in show_p {
            let midi: MidiInput = e.into();
            match midi {
                MidiInput::Controller {
                    channel: _,
                    control,
                    value,
                } => {
                    if self.key_change {
                        self.keyboard.update_key(KeyBoardKey::Record, control);
                        self.key_change = false;
                    } else {
                        if self.keyboard.get_keyboard_key(control) == Some(KeyBoardKey::Record)
                            && value > 0
                            && self.active
                        {
                            if self.recording {
                                self.recording = false;
                                if let Err(e) = self
                                    .messages_out
                                    .send(MessageToRecorderUI::ShowRecordingStopped)
                                {
                                    println!("Error: {e}");
                                }
                                let mut tmp_buf =
                                    Vec::<i16>::with_capacity(self.record_buffer.capacity());
                                std::mem::swap(&mut self.record_buffer, &mut tmp_buf);
                                if let Err(e) = crate::wavwriter::save_wav(
                                    tmp_buf,
                                    self.rate as u32,
                                    Some(&self.audio_prefix),
                                ) {
                                    println!("Error while saving the wav file: {e}");
                                }
                            } else {
                                self.recording = true;
                                if let Err(e) = self
                                    .messages_out
                                    .send(MessageToRecorderUI::ShowRecordingStarted)
                                {
                                    println!("Error: {e}");
                                }
                            }
                        }
                    }
                }
                _ => {}
            }
        }

        if self.recording && self.active {
            let audio_in = self.audio_mono_in.as_slice(ps);
            for value in audio_in {
                let sample = ((value) * 32768.0) as i16;
                self.record_buffer.push(sample);
            }
        }
        jack::Control::Continue
    }
}

#[derive(Debug)]
enum MessageToRecorder {
    StartRecording,
    StopRecordeing,
    NewPrefix(String),
    ChangeRecord,
    DiscardRecordKey,
    Active(bool),
}

impl From<KeyBoardKeySetter> for MessageToRecorder {
    fn from(value: KeyBoardKeySetter) -> Self {
        match value {
            KeyBoardKeySetter::Set(_k) => MessageToRecorder::ChangeRecord,
            KeyBoardKeySetter::Clear(_k) => MessageToRecorder::DiscardRecordKey,
        }
    }
}

#[derive(Debug)]
enum MessageToRecorderUI {
    ShowRecordingStarted,
    ShowRecordingStopped,
}

struct RecorderUI {
    messages_in: std::sync::mpsc::Receiver<MessageToRecorderUI>,
    message_out: std::sync::mpsc::Sender<MessageToRecorder>,
    messages: Vec<String>,
    record_pressed: bool,
    current_prefix: String,
    active: bool,
}

impl RecorderUI {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        messages_in: std::sync::mpsc::Receiver<MessageToRecorderUI>,
        messages_out: std::sync::mpsc::Sender<MessageToRecorder>,
    ) -> RecorderUI {
        RecorderUI {
            messages_in,
            message_out: messages_out,
            messages: Vec::new(),
            record_pressed: false,
            current_prefix: String::from(""),
            active: true,
        }
    }

    fn create_menu(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("Settings", |ui| {
                if ui
                    .toggle_value(&mut self.active, String::from("Active"))
                    .clicked()
                {
                    self.send_message(MessageToRecorder::Active(self.active));
                    ui.close_menu();
                }
                crate::utils::create_keyboard_select(
                    ui,
                    "Keyboard record key",
                    KeyBoardKey::Record,
                    &mut self.message_out,
                    &mut self.messages,
                );
            });
            crate::utils::common_menu_luncher(ui, &mut self.messages);
        });
    }

    fn create_content(&mut self, ui: &mut egui::Ui) {
        let rich_text =
            egui::RichText::new(format!("{}", if self.record_pressed { "REC" } else { "" }))
                .color(egui::Color32::from_rgb(180, 19, 60));
        let _recording = ui.label(rich_text);
        ui.horizontal(|ui| {
            ui.label("Recording: ");
            if self.record_pressed {
                if ui.button("In progress").clicked() {
                    self.record_pressed = false;
                    self.send_message(MessageToRecorder::StopRecordeing);
                }
            } else {
                if ui.button("Waiting").clicked() {
                    self.record_pressed = true;
                    self.send_message(MessageToRecorder::StartRecording);
                }
            }
        });
        ui.horizontal(|ui| {
            ui.label("Audio file prefix: ");
            ui.text_edit_singleline(&mut self.current_prefix);
        });
        if ui.button("Change prefix").clicked() {
            self.send_message(MessageToRecorder::NewPrefix(self.current_prefix.clone()));
        }
        crate::utils::show_logs(ui, &mut self.messages);
    }

    fn send_message(&mut self, msg: MessageToRecorder) {
        if let Err(e) = self.message_out.send(msg) {
            self.messages.push(format!("Internal error: {e}"));
        }
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
                MessageToRecorderUI::ShowRecordingStarted => self.record_pressed = true,
                MessageToRecorderUI::ShowRecordingStopped => self.record_pressed = false,
            },
        }
    }
}

impl eframe::App for RecorderUI {
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

pub fn record() -> Result<(), CommonError> {
    // open client
    let (client, _status) =
        match jack::Client::new("recorder", jack::ClientOptions::NO_START_SERVER) {
            Ok(v) => v,
            Err(e) => {
                return Err(CommonError::UnableToStartClient(e));
            }
        };

    //open a message channel for the recorder and the UI
    let (send_to_rec, rcv_from_ui) = std::sync::mpsc::channel();
    let (send_to_ui, rcv_from_rec) = std::sync::mpsc::channel();

    let synth = Recorder::new(&client, rcv_from_ui, send_to_ui)?;
    let active_client = match client.activate_async((), synth) {
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
        Box::new(|cc| Ok(Box::new(RecorderUI::new(cc, rcv_from_rec, send_to_rec)))),
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
