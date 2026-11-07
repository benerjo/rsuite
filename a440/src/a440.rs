use eframe::egui::{self, ViewportBuilder};

use rsuite::{
    midiinput::MidiInput,
    nsm::{self, Session},
    utils::{CommonError, ConnectionType},
};

const A440_INDEX: usize = 57;
const VELOCITY: f64 = 100.0 / 256.0;
const MAX_MESSAGE_LEN: usize = 3;

struct A440 {
    midi_out: jack::Port<jack::MidiOut>,
    messages_in: std::sync::mpsc::Receiver<MessageToA440>,
}

impl A440 {
    pub fn new(
        client: &jack::Client,
        messages_in: std::sync::mpsc::Receiver<MessageToA440>,
    ) -> Result<A440, CommonError> {
        let m_out = match client.register_port("midi_out", jack::MidiOut::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiOut, e)),
        };

        Ok(A440 {
            midi_out: m_out,
            messages_in,
        })
    }

    fn send_midi(writer: &mut jack::MidiWriter, message: MidiInput) {
        let mut bytes = [0; MAX_MESSAGE_LEN];
        if let Err(e) = writer.write(&message.to_raw(&mut bytes)) {
            eprintln!("Internal error: {e}");
        }
    }
}

impl jack::ProcessHandler for A440 {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        let mut writer = self.midi_out.writer(ps);
        while let Ok(message) = self.messages_in.try_recv() {
            let midi = match message {
                MessageToA440::Press => MidiInput::NoteStart {
                    channel: 0,
                    note_index: A440_INDEX,
                    timing: 0,
                    velocity: VELOCITY,
                },
                MessageToA440::Release => MidiInput::NoteEnd {
                    channel: 0,
                    note_index: A440_INDEX,
                    timing: 0,
                    velocity: 0.0,
                },
            };
            Self::send_midi(&mut writer, midi);
        }

        jack::Control::Continue
    }
}

#[derive(Debug)]
enum MessageToA440 {
    Press,
    Release,
}

struct A440UI {
    message_out: std::sync::mpsc::Sender<MessageToA440>,
    pressed: bool,
}

impl A440UI {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        messages_out: std::sync::mpsc::Sender<MessageToA440>,
    ) -> A440UI {
        nsm::set_ui_context(&cc.egui_ctx);
        A440UI {
            message_out: messages_out,
            pressed: false,
        }
    }

    fn create_content(&mut self, ui: &mut egui::Ui) {
        let size = ui.available_size();
        let response = ui.add_sized(size, egui::Button::new("A440").selected(self.pressed));
        let pressed = response.is_pointer_button_down_on();
        if pressed != self.pressed {
            self.pressed = pressed;
            self.send_message(if pressed {
                MessageToA440::Press
            } else {
                MessageToA440::Release
            });
        }
    }

    fn send_message(&mut self, msg: MessageToA440) {
        if let Err(e) = self.message_out.send(msg) {
            eprintln!("Internal error: {e}");
        }
    }
}

impl eframe::App for A440UI {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ui, |ui| {
            self.create_content(ui);
        });
    }

    fn on_exit(&mut self) {
        //make sure the note does not keep ringing
        if self.pressed {
            self.pressed = false;
            self.send_message(MessageToA440::Release);
        }
    }
}

pub fn a440() -> Result<(), CommonError> {
    // open client
    let session = Session::start("A440")?;
    let client = session.client("a440")?;

    let (send_to_a440, rcv_from_ui) = std::sync::mpsc::channel();

    let a440 = A440::new(&client, rcv_from_ui)?;
    let title = session.window_title("A440");
    let active_client = session.activate_stateless(client, a440)?;

    nsm::run_window(
        &title,
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size(egui::vec2(200.0, 200.0)),
            run_and_return: true,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(A440UI::new(cc, send_to_a440)))),
    )?;

    match active_client.deactivate() {
        Ok(_) => return Ok(()),
        Err(e) => {
            return Err(CommonError::UnableToDeActivateClient(e));
        }
    }
}
