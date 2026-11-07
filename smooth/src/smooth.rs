use eframe::egui::{self, ViewportBuilder};

use rsuite::{
    nsm::{self, Persistent, Session, Stateful, Values},
    utils::{CommonError, ConnectionType},
};

///Messages passed from the UI to the smooth
enum SmoothMessages {
    NewAlpha(f64),
}

struct Smooth {
    alpha: f64,
    beta: f64,
    avg: f64,
    /// The input midi port
    _midi_in: jack::Port<jack::MidiIn>,
    /// The input audio port
    audio_mono_in: jack::Port<jack::AudioIn>,
    /// The output audio port
    audio_mono_out: jack::Port<jack::AudioOut>,
    /// The incoming messages
    messages_in: std::sync::mpsc::Receiver<SmoothMessages>,
}

const MIN_ALPHA: f64 = 0.000001;
const MAX_ALPHA: f64 = 0.01;
const ALPHA_DEFAULT: f64 = 0.004;

impl Smooth {
    pub fn new(
        alpha: f64,
        client: &jack::Client,
        messages: std::sync::mpsc::Receiver<SmoothMessages>,
    ) -> Result<Smooth, CommonError> {
        let m_in = match client.register_port("midi_input", jack::MidiIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiIn, e)),
        };
        let a_in = match client.register_port("music_in", jack::AudioIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::AudioIn, e)),
        };
        let a_out = match client.register_port("music_out", jack::AudioOut::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::AudioOut, e)),
        };
        let a = if alpha < MIN_ALPHA {
            MIN_ALPHA
        } else if alpha > MAX_ALPHA {
            MAX_ALPHA
        } else {
            alpha
        };
        Ok(Smooth {
            alpha: a,
            beta: 1.0 - a,
            avg: 0.0,
            _midi_in: m_in,
            audio_mono_in: a_in,
            audio_mono_out: a_out,
            messages_in: messages,
        })
    }
}

///The state saved by the session manager
#[derive(Debug, Clone, PartialEq)]
struct SmoothState {
    alpha: f64,
}

impl Persistent for SmoothState {
    fn save(&self, values: &mut Values) {
        values.set("alpha", self.alpha);
    }

    fn load(&mut self, values: &Values) {
        values.load("alpha", &mut self.alpha);
    }
}

impl Stateful for Smooth {
    type State = SmoothState;

    fn publish(&self, state: &mut Self::State) {
        state.alpha = self.alpha;
    }
}

impl jack::ProcessHandler for Smooth {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        if let Ok(msg) = self.messages_in.try_recv() {
            match msg {
                SmoothMessages::NewAlpha(alpha) => {
                    self.alpha = alpha;
                    self.beta = 1.0 - alpha;
                }
            }
        }
        let audio_in = self.audio_mono_in.as_slice(ps);
        let audio_out = self.audio_mono_out.as_mut_slice(ps);
        audio_out.copy_from_slice(audio_in);
        for v in audio_out {
            self.avg = (*v) as f64 * self.alpha + self.avg * self.beta;
            *v = self.avg as f32;
        }
        jack::Control::Continue
    }
}

struct SmoothUI {
    message_out: std::sync::mpsc::Sender<SmoothMessages>,
    current_alpha: f64,
    sent_alpha: f64,
    /// The log messages
    messages: Vec<String>,
}

impl SmoothUI {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        messages: std::sync::mpsc::Sender<SmoothMessages>,
        alpha: f64,
    ) -> SmoothUI {
        nsm::set_ui_context(&cc.egui_ctx);
        SmoothUI {
            message_out: messages,
            current_alpha: alpha,
            sent_alpha: -1.0,
            messages: Vec::new(),
        }
    }
    fn create_menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            rsuite::utils::common_menu_luncher(ui, &mut self.messages);
        });
    }

    fn create_content(&mut self, ui: &mut egui::Ui) {
        if self.sent_alpha < self.current_alpha || self.sent_alpha > self.current_alpha {
            //value has changed
            if self.send_message(SmoothMessages::NewAlpha(self.current_alpha)) {
                self.sent_alpha = self.current_alpha;
            }
        }
        let range = std::ops::RangeInclusive::new(MIN_ALPHA, MAX_ALPHA);
        rsuite::utils::create_f64_slider(ui, "Alpha", &mut self.current_alpha, range);

        rsuite::utils::show_logs(ui, &mut self.messages);
    }

    fn send_message(&mut self, msg: SmoothMessages) -> bool {
        if let Err(e) = self.message_out.send(msg) {
            self.messages.push(format!("Internal error: {e}"));
            return false;
        } else {
            return true;
        }
    }
}

impl eframe::App for SmoothUI {
    fn logic(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        //the following line should only occur if a repaint is really needed.
        //To do this, we need to revise the architecture to make sure that it
        //can be called whenever there is someting in the queue
        ctx.request_repaint();
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

pub fn smooth() -> Result<(), CommonError> {
    // open client
    let session = Session::start("Smooth")?;
    let client = session.client("smooth")?;

    //open a message channel for the recorder and the UI
    let (send, rcv) = std::sync::mpsc::channel();

    let state = session.load(SmoothState {
        alpha: ALPHA_DEFAULT,
    });
    let synth = Smooth::new(state.alpha, &client, rcv)?;
    //the value used by the effect, which may differ from the saved one
    let alpha = synth.alpha;
    let title = session.window_title("Smooth");
    let active_client = session.activate(client, synth, state)?;

    nsm::run_window(
        &title,
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size(egui::vec2(320.0, 640.0)),
            run_and_return: true,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(SmoothUI::new(cc, send, alpha)))),
    )?;

    match active_client.deactivate() {
        Ok(_) => return Ok(()),
        Err(e) => {
            return Err(CommonError::UnableToDeActivateClient(e));
        }
    }
}
