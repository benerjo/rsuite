use eframe::egui::{self, ViewportBuilder};
use jack::{MidiWriter, RawMidi};

use crate::{
    midiinput::MidiInput,
    nsm::{self, sync, Persistent, Session, Stateful, Values},
    synth::hardware::{HardWare, KeyBoardKey},
    utils::{CommonError, ConnectionType},
};

use super::KeyBoardKeySetter;

const MAX_TRANSPOSE: usize = 13;

struct Transposer {
    /// The number of half-step we have to transpose the input
    transpose: usize,
    /// The midi input to activate the pass-through and to listen to
    midi_in: jack::Port<jack::MidiIn>,
    /// The midi output
    midi_out: jack::Port<jack::MidiOut>,
    //The incoming messages from the UI
    messages_in: std::sync::mpsc::Receiver<MessageToTransposer>,
    ///The outgoing messages to the UI
    messages_out: std::sync::mpsc::Sender<MessageToTransposerUI>,
    ///If true, the next control will be used as key to start/stop the recording
    key_change: Option<KeyBoardKey>,
    ///The keyboard events we are listening to
    keyboard: HardWare,
}

impl Transposer {
    pub fn new(
        client: &jack::Client,
        messages_in: std::sync::mpsc::Receiver<MessageToTransposer>,
        messages_out: std::sync::mpsc::Sender<MessageToTransposerUI>,
        state: TransposerState,
    ) -> Result<Transposer, CommonError> {
        let m_in = match client.register_port("midi_in", jack::MidiIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiIn, e)),
        };
        let m_out = match client.register_port("midi_out", jack::MidiOut::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiOut, e)),
        };

        Ok(Transposer {
            transpose: state.transpose % MAX_TRANSPOSE,
            midi_in: m_in,
            midi_out: m_out,
            messages_in,
            messages_out,
            key_change: None,
            keyboard: state.keyboard,
        })
    }

    fn send_message(
        msg: MessageToTransposerUI,
        messages_out: &mut std::sync::mpsc::Sender<MessageToTransposerUI>,
    ) {
        if let Err(e) = messages_out.send(msg) {
            eprintln!("Internal error: {e}");
        }
    }

    fn write(
        writer: &mut MidiWriter<'_>,
        initial: &MidiInput,
        raw: &RawMidi<'_>,
        messages_out: &mut std::sync::mpsc::Sender<MessageToTransposerUI>,
    ) {
        if let Err(e) = writer.write(raw) {
            Self::send_message(
                MessageToTransposerUI::Message(format!(
                    "Unable to write message {:?}({:?}): {e}",
                    raw, initial
                )),
                messages_out,
            );
        }
    }
}

///The state saved by the session manager
#[derive(Debug, Clone, PartialEq)]
struct TransposerState {
    transpose: usize,
    keyboard: HardWare,
}

impl Persistent for TransposerState {
    fn save(&self, values: &mut Values) {
        values.set("transpose", self.transpose);
        self.keyboard.save(values);
    }

    fn load(&mut self, values: &Values) {
        values.load("transpose", &mut self.transpose);
        self.keyboard.load(values);
    }
}

impl Stateful for Transposer {
    type State = TransposerState;

    fn publish(&self, state: &mut Self::State) {
        state.transpose = self.transpose;
        sync(&mut state.keyboard, &self.keyboard);
    }
}

impl jack::ProcessHandler for Transposer {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        if let Ok(message) = self.messages_in.try_recv() {
            match message {
                MessageToTransposer::ChangeActivationMidiKey(key) => self.key_change = Some(key),
                MessageToTransposer::TransposeLevel(lvl) => self.transpose = lvl % MAX_TRANSPOSE,
                MessageToTransposer::ClearActivationMidiKey(key) => self.keyboard.clear_key(key),
            }
        }

        let show_p = self.midi_in.iter(ps);
        let mut writer = self.midi_out.writer(ps);
        for e in show_p {
            let midi: MidiInput = e.into();
            match midi {
                MidiInput::Controller {
                    channel: _,
                    control,
                    value,
                } => {
                    if let Some(key) = self.key_change {
                        self.keyboard.update_key(key, control);
                        self.key_change = None;
                    }
                    if self.keyboard.get_keyboard_key(control) == Some(KeyBoardKey::TransposeUp)
                        && value > 0
                    {
                        self.transpose = (self.transpose + 1) % MAX_TRANSPOSE;
                        Self::send_message(
                            MessageToTransposerUI::TransposeLevel(self.transpose),
                            &mut self.messages_out,
                        );
                    } else if self.keyboard.get_keyboard_key(control)
                        == Some(KeyBoardKey::TransposeDown)
                        && value > 0
                    {
                        self.transpose = (self.transpose + 11) % MAX_TRANSPOSE;
                        Self::send_message(
                            MessageToTransposerUI::TransposeLevel(self.transpose),
                            &mut self.messages_out,
                        );
                    }
                }
                _ => {}
            }
            if let MidiInput::NoteStart {
                channel,
                note_index,
                timing,
                velocity,
            } = midi
            {
                let mut bytes = vec![0; 3];
                let raw = MidiInput::NoteStart {
                    channel: channel,
                    note_index: note_index + self.transpose,
                    timing: timing,
                    velocity: velocity,
                }
                .to_raw(&mut bytes);
                Self::write(&mut writer, &midi, &raw, &mut self.messages_out);
            } else if let MidiInput::NoteEnd {
                channel,
                note_index,
                timing,
                velocity,
            } = midi
            {
                let mut bytes = vec![0; 4];
                let raw = MidiInput::NoteEnd {
                    channel: channel,
                    note_index: note_index + self.transpose,
                    timing: timing,
                    velocity: velocity,
                }
                .to_raw(&mut bytes);
                Self::write(&mut writer, &midi, &raw, &mut self.messages_out);
            } else {
                Self::write(&mut writer, &midi, &e, &mut self.messages_out);
            }
        }

        jack::Control::Continue
    }
}

#[derive(Debug)]
enum MessageToTransposer {
    ChangeActivationMidiKey(KeyBoardKey),
    ClearActivationMidiKey(KeyBoardKey),
    TransposeLevel(usize),
}

impl From<KeyBoardKeySetter> for MessageToTransposer {
    fn from(value: KeyBoardKeySetter) -> Self {
        match value {
            KeyBoardKeySetter::Set(k) => MessageToTransposer::ChangeActivationMidiKey(k),
            KeyBoardKeySetter::Clear(k) => MessageToTransposer::ClearActivationMidiKey(k),
        }
    }
}

#[derive(Debug)]
enum MessageToTransposerUI {
    Message(String),
    TransposeLevel(usize),
}

struct TransposerUI {
    messages_in: std::sync::mpsc::Receiver<MessageToTransposerUI>,
    message_out: std::sync::mpsc::Sender<MessageToTransposer>,
    messages: Vec<String>,
    transpose_amount: usize,
}

impl TransposerUI {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        messages_in: std::sync::mpsc::Receiver<MessageToTransposerUI>,
        messages_out: std::sync::mpsc::Sender<MessageToTransposer>,
        transpose_amount: usize,
    ) -> TransposerUI {
        nsm::set_ui_context(&cc.egui_ctx);
        TransposerUI {
            messages_in,
            message_out: messages_out,
            messages: Vec::new(),
            transpose_amount,
        }
    }

    fn create_menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Settings", |ui| {
                crate::utils::create_keyboard_select(
                    ui,
                    "Transpose up",
                    KeyBoardKey::TransposeUp,
                    &mut self.message_out,
                    &mut self.messages,
                );
                crate::utils::create_keyboard_select(
                    ui,
                    "Transpose down",
                    KeyBoardKey::TransposeUp,
                    &mut self.message_out,
                    &mut self.messages,
                );
            });
            crate::utils::common_menu_luncher(ui, &mut self.messages);
        });
    }

    fn create_content(&mut self, ui: &mut egui::Ui) {
        ui.horizontal(|ui| {
            let initial_lvl = self.transpose_amount;
            crate::utils::create_usize_slider(
                ui,
                "Transpose: ",
                &mut self.transpose_amount,
                std::ops::RangeInclusive::new(0usize, MAX_TRANSPOSE - 1),
            );
            if initial_lvl != self.transpose_amount {
                self.send_message(MessageToTransposer::TransposeLevel(self.transpose_amount));
            }
        });
        crate::utils::show_logs(ui, &mut self.messages);
    }

    fn send_message(&mut self, msg: MessageToTransposer) {
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
                MessageToTransposerUI::Message(msg) => self.messages.push(msg),
                MessageToTransposerUI::TransposeLevel(lvl) => self.transpose_amount = lvl,
            },
        }
    }
}

impl eframe::App for TransposerUI {
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

pub fn transposer() -> Result<(), CommonError> {
    // open client
    let session = Session::start("Transposer")?;
    let client = session.client("transposer")?;

    let client_name = session.window_title(client.name());

    //open a message channel for the recorder and the UI
    let (send_to_rec, rcv_from_ui) = std::sync::mpsc::channel();
    let (send_to_ui, rcv_from_rec) = std::sync::mpsc::channel();

    let state = session.load(TransposerState {
        transpose: 0,
        keyboard: HardWare::new(),
    });
    let transpose = state.transpose % MAX_TRANSPOSE;
    let synth = Transposer::new(&client, rcv_from_ui, send_to_ui, state.clone())?;
    let active_client = session.activate(client, synth, state)?;

    nsm::run_window(
        &client_name,
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size(egui::vec2(320.0, 640.0)),
            run_and_return: true,
            ..Default::default()
        },
        Box::new(|cc| {
            Ok(Box::new(TransposerUI::new(
                cc,
                rcv_from_rec,
                send_to_rec,
                transpose,
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
