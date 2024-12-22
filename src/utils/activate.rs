use eframe::egui::{self, ViewportBuilder};

use crate::{
    midiinput::MidiInput,
    synth::hardware::{HardWare, KeyBoardKey},
    utils::{CommonError, ConnectionType},
};

struct Activator {
    /// If false, the recorder will not listen to record events
    active: bool,
    /// The midi input to activate the pass-through and to listen to
    midi_in: jack::Port<jack::MidiIn>,
    /// The midi output
    midi_out: jack::Port<jack::MidiOut>,
    //The incoming messages from the UI
    messages_in: std::sync::mpsc::Receiver<MessageToActivator>,
    ///The outgoing messages to the UI
    messages_out: std::sync::mpsc::Sender<MessageToActivatorUI>,
    ///If true, the next control will be used as key to start/stop the recording
    key_change: bool,
    ///The keyboard events we are listening to
    keyboard: HardWare,
}

impl Activator {
    pub fn new(
        client: &jack::Client,
        messages_in: std::sync::mpsc::Receiver<MessageToActivator>,
        messages_out: std::sync::mpsc::Sender<MessageToActivatorUI>,
    ) -> Result<Activator, CommonError> {
        let m_in = match client.register_port("midi_in", jack::MidiIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiIn, e)),
        };
        let m_out = match client.register_port("midi_out", jack::MidiOut::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiOut, e)),
        };

        Ok(Activator {
            active: true,
            midi_in: m_in,
            midi_out: m_out,
            messages_in,
            messages_out,
            key_change: false,
            keyboard: HardWare::new(),
        })
    }

    fn send_message(
        msg: MessageToActivatorUI,
        messages_out: &mut std::sync::mpsc::Sender<MessageToActivatorUI>,
    ) {
        if let Err(e) = messages_out.send(msg) {
            eprintln!("Internal error: {e}");
        }
    }
}

impl jack::ProcessHandler for Activator {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        if let Ok(message) = self.messages_in.try_recv() {
            match message {
                MessageToActivator::LetMidiThrough => {
                    self.active = true;
                }
                MessageToActivator::BlockMidi => {
                    self.active = false;
                }
                MessageToActivator::ChangeActivationMidiKey => self.key_change = true,
            }
        }

        let show_p = self.midi_in.iter(ps);
        let mut writer = if self.active {
            Some(self.midi_out.writer(ps))
        } else {
            None
        };
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
                    }
                    if self.keyboard.get_keyboard_key(control) == Some(KeyBoardKey::Record)
                        && value > 0
                    {
                        if self.active {
                            Self::send_message(
                                MessageToActivatorUI::ShowMidiBlocked,
                                &mut self.messages_out,
                            );
                            self.active = false;
                        } else {
                            Self::send_message(
                                MessageToActivatorUI::ShowMidiThrough,
                                &mut self.messages_out,
                            );
                            self.active = true;
                        }
                    }
                }
                _ => {}
            }
            if writer.is_some() {
                let w = writer.as_mut().unwrap();
                if let Err(e) = w.write(&e) {
                    println!("Error: {e}");
                }
            }
        }

        jack::Control::Continue
    }
}

#[derive(Debug)]
enum MessageToActivator {
    LetMidiThrough,
    BlockMidi,
    ChangeActivationMidiKey,
}

#[derive(Debug)]
enum MessageToActivatorUI {
    ShowMidiThrough,
    ShowMidiBlocked,
}

struct RecorderUI {
    messages_in: std::sync::mpsc::Receiver<MessageToActivatorUI>,
    message_out: std::sync::mpsc::Sender<MessageToActivator>,
    messages: Vec<String>,
    active_pressed: bool,
}

impl RecorderUI {
    pub fn new(
        _cc: &eframe::CreationContext<'_>,
        messages_in: std::sync::mpsc::Receiver<MessageToActivatorUI>,
        messages_out: std::sync::mpsc::Sender<MessageToActivator>,
    ) -> RecorderUI {
        RecorderUI {
            messages_in,
            message_out: messages_out,
            messages: Vec::new(),
            active_pressed: true,
        }
    }

    fn create_menu(&mut self, ui: &mut egui::Ui) {
        egui::menu::bar(ui, |ui| {
            ui.menu_button("Settings", |ui| {
                if ui.button("Set Activation action").clicked() {
                    self.send_message(MessageToActivator::ChangeActivationMidiKey);
                    ui.close_menu();
                };
            });
            crate::utils::common_menu_luncher(ui, &mut self.messages);
        });
    }

    fn create_content(&mut self, ui: &mut egui::Ui) {
        let rich_text = egui::RichText::new(format!(
            "{}",
            if self.active_pressed { "ACTIVE" } else { "" }
        ))
        .color(egui::Color32::from_rgb(180, 19, 60));
        let _recording = ui.label(rich_text);
        ui.horizontal(|ui| {
            ui.label("Status: ");
            if self.active_pressed {
                if ui.button("Pass-through").clicked() {
                    self.active_pressed = false;
                    self.send_message(MessageToActivator::BlockMidi);
                }
            } else {
                if ui.button("Blocked").clicked() {
                    self.active_pressed = true;
                    self.send_message(MessageToActivator::LetMidiThrough);
                }
            }
        });
        crate::utils::show_logs(ui, &mut self.messages);
    }

    fn send_message(&mut self, msg: MessageToActivator) {
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
                MessageToActivatorUI::ShowMidiThrough => self.active_pressed = true,
                MessageToActivatorUI::ShowMidiBlocked => self.active_pressed = false,
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

pub fn activator() -> Result<(), CommonError> {
    // open client
    let (client, _status) =
        match jack::Client::new("activator", jack::ClientOptions::NO_START_SERVER) {
            Ok(v) => v,
            Err(e) => {
                return Err(CommonError::UnableToStartClient(e));
            }
        };

    //open a message channel for the recorder and the UI
    let (send_to_rec, rcv_from_ui) = std::sync::mpsc::channel();
    let (send_to_ui, rcv_from_rec) = std::sync::mpsc::channel();

    let synth = Activator::new(&client, rcv_from_ui, send_to_ui)?;
    let active_client = match client.activate_async((), synth) {
        Ok(client) => client,
        Err(e) => return Err(CommonError::UnableToActivateTheClient(e)),
    };

    match eframe::run_native(
        "Activator",
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
