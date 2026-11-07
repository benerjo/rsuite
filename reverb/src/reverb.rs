use eframe::egui::{self, ViewportBuilder};

use rsuite::{
    configuration::{self, ConfigurationValue, FloatValueInRange},
    midiinput::MidiInput,
    nsm::{self, ConfigurationState, Session, Stateful},
    synth::hardware::{HardWare, KeyBoardKey},
    utils::{CommonError, ConnectionType, KeyBoardKeySetter},
};

///The maximum value of the pre-delay, in milliseconds
const MAX_PRE_DELAY: f64 = 100.0;
///The audio rate for which the lengths of the filters are given
const REFERENCE_RATE: f64 = 44100.0;
///The lengths of the comb filters, in frames at the reference rate.
/// They have no common divisor so that their echoes do not pile up
const COMB_LENGTHS: [usize; 8] = [1116, 1188, 1277, 1356, 1422, 1491, 1557, 1617];
///The lengths of the all pass filters, in frames at the reference rate
const ALL_PASS_LENGTHS: [usize; 4] = [556, 441, 341, 225];
///The feedback of the comb filters for the smallest room
const MIN_FEEDBACK: f64 = 0.7;
///The feedback added to the comb filters for the biggest room. The sum
/// has to stay below 1, otherwise the reverberation would never stop
const SIZE_FEEDBACK: f64 = 0.28;
///The highest proportion of the high frequencies absorbed at each echo
const MAX_DAMPING: f64 = 0.4;
///The feedback of the all pass filters
const ALL_PASS_FEEDBACK: f64 = 0.5;
///The gain applied to the input of the comb filters, to compensate for their feedback
const INPUT_GAIN: f64 = 0.015;
///The gain applied to the reverberated signal
const WET_GAIN: f64 = 3.0;
///Values smaller than this one are replaced by 0. Otherwise, the end of the
/// reverberation would produce numbers so small that computing them is very slow
const SILENCE: f64 = 1e-20;

#[derive(Debug, Clone, PartialEq)]
struct Configuration {
    /// The size of the room, between 0 and 1. The bigger the room, the longer the reverberation
    size: FloatValueInRange,
    /// How much the high frequencies are absorbed by the walls, between 0 and 1
    damping: FloatValueInRange,
    /// The time before the first echo is heard, in milliseconds
    pre_delay: FloatValueInRange,
    /// The proportion of reverberated signal in the output, between 0 and 1
    mix: FloatValueInRange,
}

impl Configuration {
    pub fn new() -> Configuration {
        Self {
            size: FloatValueInRange::new(0.5, 0.0, 1.0, "size", KeyBoardKey::Duration),
            damping: FloatValueInRange::new(0.5, 0.0, 1.0, "damping", KeyBoardKey::Parameter),
            pre_delay: FloatValueInRange::new(
                20.0,
                0.0,
                MAX_PRE_DELAY,
                "pre-delay",
                KeyBoardKey::ParameterB,
            ),
            mix: FloatValueInRange::new(0.3, 0.0, 1.0, "mix", KeyBoardKey::Gain),
        }
    }
}

impl<'c> configuration::Configuration<'c> for Configuration {
    fn elements(&'c mut self) -> Vec<ConfigurationValue<'c>> {
        vec![
            ConfigurationValue::Float(&mut self.size),
            ConfigurationValue::Float(&mut self.damping),
            ConfigurationValue::Float(&mut self.pre_delay),
            ConfigurationValue::Float(&mut self.mix),
        ]
    }
}

///Convert a length given at the reference rate to the given audio rate
fn frames(length: usize, rate: usize) -> usize {
    ((length as f64 * rate as f64 / REFERENCE_RATE).round() as usize).max(1)
}

///Replace the values too small to be heard by 0
fn flush(value: f64) -> f64 {
    if value.abs() < SILENCE {
        0.0
    } else {
        value
    }
}

///A delay whose output is fed back to its input, through a low pass filter.
/// This simulates the echoes between two walls, whose high frequencies fade faster
struct Comb {
    /// The last samples, delayed by the length of the buffer
    buffer: Vec<f64>,
    /// The position of the next sample to read and write
    position: usize,
    /// The last output of the low pass filter
    filtered: f64,
}

impl Comb {
    fn new(length: usize) -> Comb {
        Comb {
            buffer: vec![0.0; length],
            position: 0,
            filtered: 0.0,
        }
    }

    fn process(&mut self, input: f64, feedback: f64, damping: f64) -> f64 {
        let output = self.buffer[self.position];
        self.filtered = flush(output * (1.0 - damping) + self.filtered * damping);
        self.buffer[self.position] = input + self.filtered * feedback;
        self.position = (self.position + 1) % self.buffer.len();
        output
    }
}

///A delay that keeps the volume of every frequency but shifts their phases.
/// This spreads the echoes produced by the comb filters, so that they are not heard
/// one by one
struct AllPass {
    /// The last samples, delayed by the length of the buffer
    buffer: Vec<f64>,
    /// The position of the next sample to read and write
    position: usize,
}

impl AllPass {
    fn new(length: usize) -> AllPass {
        AllPass {
            buffer: vec![0.0; length],
            position: 0,
        }
    }

    fn process(&mut self, input: f64) -> f64 {
        let delayed = self.buffer[self.position];
        self.buffer[self.position] = flush(input + delayed * ALL_PASS_FEEDBACK);
        self.position = (self.position + 1) % self.buffer.len();
        delayed - input
    }
}

///Simulate the reflections of the sound in a room: the input is delayed by the
/// pre-delay, sent to comb filters in parallel, and their sum goes through all pass
/// filters one after the other
struct Room {
    /// The audio rate (number of audio frames per seconds)
    rate: f64,
    /// The last received samples, used for the pre-delay
    pre_delay: Vec<f64>,
    /// The position of the last sample written to the pre-delay buffer
    write_position: usize,
    /// The comb filters, producing the echoes
    combs: Vec<Comb>,
    /// The all pass filters, spreading the echoes
    all_passes: Vec<AllPass>,
}

impl Room {
    fn new(rate: usize) -> Room {
        let max_pre_delay = MAX_PRE_DELAY * rate as f64 / 1000.0;
        Room {
            rate: rate as f64,
            //one additional frame is needed to read the sample with the longest delay
            pre_delay: vec![0.0; max_pre_delay.ceil() as usize + 1],
            write_position: 0,
            combs: COMB_LENGTHS
                .iter()
                .map(|l| Comb::new(frames(*l, rate)))
                .collect(),
            all_passes: ALL_PASS_LENGTHS
                .iter()
                .map(|l| AllPass::new(frames(*l, rate)))
                .collect(),
        }
    }

    ///Add a sample to the room and compute the corresponding output
    fn process(&mut self, input: f64, conf: &Configuration) -> f64 {
        let len = self.pre_delay.len();
        self.write_position = (self.write_position + 1) % len;
        self.pre_delay[self.write_position] = input;
        let delay = (conf.pre_delay.get_value() * self.rate / 1000.0).round() as usize;
        let delayed = self.pre_delay[(self.write_position + len - delay.min(len - 1)) % len];

        let feedback = MIN_FEEDBACK + SIZE_FEEDBACK * conf.size.get_value();
        let damping = MAX_DAMPING * conf.damping.get_value();
        let mut wet = 0.0;
        for comb in &mut self.combs {
            wet += comb.process(delayed * INPUT_GAIN, feedback, damping);
        }
        for all_pass in &mut self.all_passes {
            wet = all_pass.process(wet);
        }

        let mix = conf.mix.get_value();
        input * (1.0 - mix) + wet * WET_GAIN * mix
    }
}

struct Reverb {
    /// The midi input to listen to
    midi_in: jack::Port<jack::MidiIn>,
    /// The input audio port
    audio_in: jack::Port<jack::AudioIn>,
    /// The output audio port
    audio_out: jack::Port<jack::AudioOut>,
    ///The incoming messages from the UI
    messages_in: std::sync::mpsc::Receiver<MessageToReverb>,
    ///The outgoing messages to the UI
    messages_out: std::sync::mpsc::Sender<MessageToReverbUI>,
    ///If defined, the next control will be mapped to this key
    key_change: Option<KeyBoardKey>,
    ///The keyboard events we are listening to
    keyboard: HardWare,
    /// The configuration
    conf: Configuration,
    /// The room generating the reverberation
    room: Room,
}

impl Reverb {
    pub fn new(
        client: &jack::Client,
        messages_in: std::sync::mpsc::Receiver<MessageToReverb>,
        mut messages_out: std::sync::mpsc::Sender<MessageToReverbUI>,
        state: ConfigurationState<Configuration>,
    ) -> Result<Reverb, CommonError> {
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
            MessageToReverbUI::NewConfig(current_config.clone()),
            &mut messages_out,
        );

        Ok(Reverb {
            midi_in: m_in,
            audio_in: a_in,
            audio_out: a_out,
            messages_in,
            messages_out,
            key_change: None,
            keyboard: state.keyboard,
            conf: current_config,
            room: Room::new(client.sample_rate() as usize),
        })
    }

    fn send_message(
        msg: MessageToReverbUI,
        messages_out: &mut std::sync::mpsc::Sender<MessageToReverbUI>,
    ) {
        if let Err(e) = messages_out.send(msg) {
            eprintln!("Internal error: {e}");
        }
    }
}

impl Stateful for Reverb {
    type State = ConfigurationState<Configuration>;

    fn publish(&self, state: &mut Self::State) {
        state.update(&self.conf, &self.keyboard);
    }
}

impl jack::ProcessHandler for Reverb {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        if let Ok(message) = self.messages_in.try_recv() {
            match message {
                MessageToReverb::ChangeMidiKey(key) => self.key_change = Some(key),
                MessageToReverb::ClearMidiKey(key) => self.keyboard.clear_key(key),
                MessageToReverb::NewConfig(configuration) => self.conf = configuration,
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
                            MessageToReverbUI::NewConfig(self.conf.clone()),
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
            *output = self.room.process(*input as f64, &self.conf) as f32;
        }

        jack::Control::Continue
    }
}

#[derive(Debug)]
enum MessageToReverb {
    ChangeMidiKey(KeyBoardKey),
    ClearMidiKey(KeyBoardKey),
    NewConfig(Configuration),
}

impl From<KeyBoardKeySetter> for MessageToReverb {
    fn from(value: KeyBoardKeySetter) -> Self {
        match value {
            KeyBoardKeySetter::Set(k) => MessageToReverb::ChangeMidiKey(k),
            KeyBoardKeySetter::Clear(k) => MessageToReverb::ClearMidiKey(k),
        }
    }
}

#[derive(Debug)]
enum MessageToReverbUI {
    NewConfig(Configuration),
}

struct ReverbUI {
    messages_in: std::sync::mpsc::Receiver<MessageToReverbUI>,
    message_out: std::sync::mpsc::Sender<MessageToReverb>,
    messages: Vec<String>,
    current_config: Option<Configuration>,
}

impl ReverbUI {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        messages_in: std::sync::mpsc::Receiver<MessageToReverbUI>,
        messages_out: std::sync::mpsc::Sender<MessageToReverb>,
    ) -> ReverbUI {
        nsm::set_ui_context(&cc.egui_ctx);
        ReverbUI {
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
            rsuite::utils::common_menu_luncher(ui, &mut self.messages);
        });
    }

    fn create_content(&mut self, ui: &mut egui::Ui) {
        if let Some(current_config) = &self.current_config {
            let mut conf = current_config.clone();

            configuration::Configuration::draw(&mut conf, ui);

            if !self.current_config.as_ref().eq(&Some(&conf)) {
                if let Err(e) = self
                    .message_out
                    .send(MessageToReverb::NewConfig(conf.clone()))
                {
                    self.messages
                        .push(format!("Error while sending new conf: {e}"));
                }
                self.current_config = Some(conf);
            }
        }
        rsuite::utils::show_logs(ui, &mut self.messages);
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
                MessageToReverbUI::NewConfig(cfg) => self.current_config = Some(cfg),
            },
        }
    }
}

impl eframe::App for ReverbUI {
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

pub fn reverb() -> Result<(), CommonError> {
    // open client
    let session = Session::start("Reverb")?;
    let client = session.client("reverb")?;

    //open a message channel for the reverb and the UI
    let (send_to_reverb, rcv_from_ui) = std::sync::mpsc::channel();
    let (send_to_ui, rcv_from_reverb) = std::sync::mpsc::channel();

    let state = session.load(ConfigurationState::new(Configuration::new()));
    let effect = Reverb::new(&client, rcv_from_ui, send_to_ui, state.clone())?;
    let title = session.window_title("Reverb");
    let active_client = session.activate(client, effect, state)?;

    nsm::run_window(
        &title,
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size(egui::vec2(320.0, 640.0)),
            run_and_return: true,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(ReverbUI::new(cc, rcv_from_reverb, send_to_reverb)))),
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
    use super::{Configuration, Room, COMB_LENGTHS, MAX_PRE_DELAY, REFERENCE_RATE};
    use rsuite::{
        configuration::FloatValueInRange,
        nsm::{ConfigurationState, Persistent, Values},
        synth::hardware::KeyBoardKey,
    };

    ///The audio rate used by the tests
    const RATE: usize = REFERENCE_RATE as usize;

    #[test]
    fn state_is_restored() {
        let mut state = ConfigurationState::new(Configuration::new());
        state.conf.size.set_value(0.8);
        state.conf.pre_delay.set_value(42.0);
        state.keyboard.update_key(KeyBoardKey::Parameter, 20);
        let mut values = Values::new();
        state.save(&mut values);

        let mut loaded = ConfigurationState::new(Configuration::new());
        assert_ne!(loaded, state);
        loaded.load(&values);
        assert_eq!(loaded, state);

        //values outside of the range are limited to the range
        values.set("value.pre-delay", 1000.0);
        loaded.load(&values);
        assert_eq!(loaded.conf.pre_delay.get_value(), MAX_PRE_DELAY);
    }

    fn conf(size: f64, damping: f64, pre_delay: f64, mix: f64) -> Configuration {
        let key = KeyBoardKey::Parameter;
        Configuration {
            size: FloatValueInRange::new(size, 0.0, 1.0, "size", key),
            damping: FloatValueInRange::new(damping, 0.0, 1.0, "damping", key),
            pre_delay: FloatValueInRange::new(pre_delay, 0.0, MAX_PRE_DELAY, "pre-delay", key),
            mix: FloatValueInRange::new(mix, 0.0, 1.0, "mix", key),
        }
    }

    ///Send an impulse to the room and return the output
    fn impulse_response(conf: &Configuration, length: usize) -> Vec<f64> {
        let mut room = Room::new(RATE);
        (0..length)
            .map(|i| room.process(if i == 0 { 1.0 } else { 0.0 }, conf))
            .collect()
    }

    ///The energy of the signal between two frames
    fn energy(signal: &[f64], from: usize, to: usize) -> f64 {
        signal[from..to].iter().map(|s| s * s).sum()
    }

    #[test]
    fn no_mix_keeps_the_input() {
        let conf = conf(0.8, 0.5, 20.0, 0.0);
        let mut room = Room::new(RATE);
        for i in 0..5000 {
            assert_eq!(room.process(i as f64, &conf), i as f64);
        }
    }

    #[test]
    fn first_echo_after_the_pre_delay() {
        let pre_delay = 50.0;
        let response = impulse_response(&conf(0.5, 0.5, pre_delay, 1.0), RATE);
        let first_echo = (pre_delay as usize * RATE / 1000) + COMB_LENGTHS[0];
        assert!(response[..first_echo].iter().all(|s| *s == 0.0));
        assert!(response[first_echo..first_echo + 1000]
            .iter()
            .any(|s| *s != 0.0));
    }

    #[test]
    fn bigger_rooms_reverberate_longer() {
        let small = impulse_response(&conf(0.2, 0.5, 0.0, 1.0), 2 * RATE);
        let big = impulse_response(&conf(0.9, 0.5, 0.0, 1.0), 2 * RATE);
        assert!(energy(&big, RATE, 2 * RATE) > 100.0 * energy(&small, RATE, 2 * RATE));
    }

    #[test]
    fn reverberation_stops() {
        let response = impulse_response(&conf(0.5, 0.5, 0.0, 1.0), 5 * RATE);
        let start = energy(&response, 0, RATE);
        assert!(start > 0.0);
        let end = energy(&response, 4 * RATE, 5 * RATE);
        assert!(end < start * 1e-12, "start: {start}, end: {end}");
    }

    #[test]
    fn damping_removes_the_high_frequencies() {
        //the difference between two frames is bigger when high frequencies are present
        let variation = |signal: &[f64]| -> f64 {
            signal[RATE / 2..RATE]
                .windows(2)
                .map(|w| (w[1] - w[0]).powi(2))
                .sum::<f64>()
                / energy(signal, RATE / 2, RATE)
        };
        let bright = impulse_response(&conf(0.8, 0.0, 0.0, 1.0), RATE);
        let dark = impulse_response(&conf(0.8, 1.0, 0.0, 1.0), RATE);
        assert!(variation(&dark) < variation(&bright));
    }

    #[test]
    fn biggest_room_stays_stable() {
        let conf = conf(1.0, 0.0, MAX_PRE_DELAY, 1.0);
        let mut room = Room::new(48000);
        let mut loudest: f64 = 0.0;
        for i in 0..10 * 48000 {
            //a loud square wave at 100Hz
            let input = if (i / 240) % 2 == 0 { 1.0 } else { -1.0 };
            loudest = loudest.max(room.process(input, &conf).abs());
        }
        assert!(loudest.is_finite() && loudest < 10.0, "loudest: {loudest}");
    }
}
