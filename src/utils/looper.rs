use eframe::egui::{self, ViewportBuilder};

use crate::{
    configuration::{self, ConfigurationValue, UsizeValueInRange},
    midiinput::{MidiInput, BAR_NOT_RUNNING, CHANNEL_MASK, NOTE_END, NOTE_START, STATUS_MASK},
    nsm::{self, ConfigurationState, Session, Stateful},
    synth::hardware::{HardWare, KeyBoardKey},
    utils::{create_keyboard_select, CommonError, ConnectionType, KeyBoardKeySetter},
};

///The number of midi clock messages per quarter note
const CLOCKS_PER_QUARTER: usize = 24;
///The number of midi commands that can be stored without needing a new allocation
const EVENTS_CAPACITY: usize = 4096;
///The longest midi command that can be stored, in bytes
const MAX_EVENT_LEN: usize = 3;
///The part of a clock during which notes still held at the end of the recording are stopped
const NOTE_END_MARGIN: f64 = 0.0001;

#[derive(Debug, Clone, PartialEq)]
struct Configuration {
    /// The number of bars that will be recorded and replayed
    nb_bars: UsizeValueInRange,
    /// The number of quarter notes per bar. This should be the same as the metronome
    nb_quarters: UsizeValueInRange,
}

impl Configuration {
    pub fn new() -> Configuration {
        Self {
            nb_bars: UsizeValueInRange::new(2, 1, 16, "bars", KeyBoardKey::Parameter),
            nb_quarters: UsizeValueInRange::new(
                4,
                1,
                16,
                "quarters per bar",
                KeyBoardKey::ParameterB,
            ),
        }
    }

    ///Retrieve the number of clock messages in a bar
    fn clocks_per_bar(&self) -> usize {
        self.nb_quarters.get_value().max(1) * CLOCKS_PER_QUARTER
    }

    ///Retrieve the number of clock messages in a loop
    fn clocks_per_loop(&self) -> usize {
        self.nb_bars.get_value().max(1) * self.clocks_per_bar()
    }
}

impl<'c> configuration::Configuration<'c> for Configuration {
    fn elements(&'c mut self) -> Vec<ConfigurationValue<'c>> {
        vec![
            ConfigurationValue::USize(&mut self.nb_bars),
            ConfigurationValue::USize(&mut self.nb_quarters),
        ]
    }
}

///The different steps of the life of a loop
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum LoopState {
    ///Nothing is recorded nor replayed
    Inactive,
    ///The recording will start with the next bar
    Waiting,
    ///The received midi commands are being stored
    Recording,
    ///The stored midi commands are being replayed
    Playing,
    ///The stored midi commands are being replayed until the end of the loop, without repeating it
    Stopping,
    ///The stored midi commands are kept but not replayed
    Stopped,
    ///The replay will start again from the beginning of the loop with the next bar
    WaitingToPlay,
}

impl std::fmt::Display for LoopState {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            LoopState::Inactive => write!(f, "Inactive"),
            LoopState::Waiting => write!(f, "Waiting for the next bar"),
            LoopState::Recording => write!(f, "Recording"),
            LoopState::Playing => write!(f, "Playing"),
            LoopState::Stopping => write!(f, "Stopping at the end of the loop"),
            LoopState::Stopped => write!(f, "Stopped"),
            LoopState::WaitingToPlay => write!(f, "Playing from the next bar"),
        }
    }
}

///The commands controlling the replay of a recorded loop
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum ReplayCommand {
    ///Start the replay from the beginning of the loop with the next bar,
    /// or keep repeating the loop if it was stopping
    Play,
    ///Finish the current replay of the loop without repeating it
    Stop,
    ///Stop the replay immediately
    Break,
}

impl ReplayCommand {
    ///Retrieve the command associated with a keyboard key
    fn from_key(key: KeyBoardKey) -> Option<ReplayCommand> {
        match key {
            KeyBoardKey::Play => Some(ReplayCommand::Play),
            KeyBoardKey::Stop => Some(ReplayCommand::Stop),
            KeyBoardKey::Break => Some(ReplayCommand::Break),
            _ => None,
        }
    }
}

///A midi command stored in a loop
#[derive(Debug, Clone, Copy, PartialEq)]
struct LoopEvent {
    /// The position in the loop, in number of clock messages
    position: f64,
    /// The content of the midi command
    bytes: [u8; MAX_EVENT_LEN],
    /// The number of meaningful bytes
    len: usize,
}

///Retrieve the number of quarter notes per bar of a time signature.
/// None is returned if a bar does not contain a whole number of quarter notes
fn quarters_per_bar(numerator: u8, denominator_power: u8) -> Option<usize> {
    //a whole note lasts 4 quarters
    let clocks =
        (numerator as usize * 4 * CLOCKS_PER_QUARTER).checked_shr(denominator_power as u32)?;
    if clocks == 0
        || clocks % CLOCKS_PER_QUARTER != 0
        || (clocks << denominator_power) != numerator as usize * 4 * CLOCKS_PER_QUARTER
    {
        return None;
    }
    Some(clocks / CLOCKS_PER_QUARTER)
}

///Retrieve the channel and the note of a midi command starting or stopping a note.
/// The boolean is true if the note is starting
fn note_change(bytes: &[u8]) -> Option<(usize, usize, bool)> {
    if bytes.len() < 3 {
        return None;
    }
    let channel = (bytes[0] & CHANNEL_MASK) as usize;
    let note = (bytes[1] & 0x7F) as usize;
    match bytes[0] & STATUS_MASK {
        NOTE_START => Some((channel, note, bytes[2] > 0)),
        NOTE_END => Some((channel, note, false)),
        _ => None,
    }
}

///The logic of the recording and of the replay, following the received clock.
/// The frames are counted since the start of the program
struct Looper {
    /// The current step
    state: LoopState,
    /// The stored midi commands, sorted by position
    events: Vec<LoopEvent>,
    /// The index of the next midi command to replay
    next_event: usize,
    /// The duration of the current loop, in number of clock messages
    loop_clocks: usize,
    /// The number of clock messages since the start of the current loop
    loop_position: usize,
    /// The number of clock messages since the last start message
    transport_position: usize,
    /// If the clock is expected to move the position forward
    running: bool,
    /// If the next clock message is the one following a start message during the replay
    restart_pending: bool,
    /// The frame of the last clock message
    last_clock_frame: Option<u64>,
    /// The number of frames between the last two clock messages
    clock_period: f64,
    /// The notes that started and did not stop during the recording
    held: [[bool; 128]; 16],
    /// The notes that started and did not stop during the replay
    sounding: [[bool; 128]; 16],
}

impl Looper {
    fn new() -> Looper {
        Looper {
            state: LoopState::Inactive,
            events: Vec::with_capacity(EVENTS_CAPACITY),
            next_event: 0,
            loop_clocks: 0,
            loop_position: 0,
            transport_position: 0,
            //if the metronome was already started, its start message will never be received
            running: true,
            restart_pending: false,
            last_clock_frame: None,
            clock_period: 0.0,
            held: [[false; 128]; 16],
            sounding: [[false; 128]; 16],
        }
    }

    ///Activate or de-activate the loop. The stored midi commands are lost on de-activation
    fn set_active<F>(&mut self, active: bool, frame: u64, output: &mut F)
    where
        F: FnMut(u64, &[u8]),
    {
        if active {
            if self.state == LoopState::Inactive {
                self.state = LoopState::Waiting;
            }
        } else if self.state != LoopState::Inactive {
            self.play(frame, output);
            self.stop_sounding_notes(frame, output);
            self.events.clear();
            self.restart_pending = false;
            self.state = LoopState::Inactive;
        }
    }

    ///Control the replay of a recorded loop. Commands that do not apply to the current
    /// state are ignored
    fn on_command<F>(&mut self, command: ReplayCommand, frame: u64, output: &mut F)
    where
        F: FnMut(u64, &[u8]),
    {
        match (command, self.state) {
            (ReplayCommand::Play, LoopState::Stopped) => self.state = LoopState::WaitingToPlay,
            (ReplayCommand::Play, LoopState::Stopping) => self.state = LoopState::Playing,
            (ReplayCommand::Stop, LoopState::Playing) => self.state = LoopState::Stopping,
            (ReplayCommand::Stop | ReplayCommand::Break, LoopState::WaitingToPlay) => {
                self.state = LoopState::Stopped
            }
            (ReplayCommand::Break, LoopState::Playing | LoopState::Stopping) => {
                self.play(frame, output);
                self.stop_sounding_notes(frame, output);
                self.restart_pending = false;
                self.state = LoopState::Stopped;
            }
            _ => {}
        }
    }

    ///Handle a start message: the next clock message is the first of the first bar
    fn on_start<F>(&mut self, frame: u64, output: &mut F)
    where
        F: FnMut(u64, &[u8]),
    {
        self.play(frame, output);
        self.running = true;
        self.transport_position = 0;
        match self.state {
            LoopState::Recording => self.state = LoopState::Waiting,
            LoopState::Playing | LoopState::Stopping => {
                self.stop_sounding_notes(frame, output);
                self.restart_pending = true;
            }
            _ => {}
        }
    }

    ///Handle a bar marker: the next clock message is the first of a bar
    fn on_bar_marker(&mut self, bar: i16) {
        if bar != BAR_NOT_RUNNING {
            self.transport_position = 0;
        }
    }

    ///Handle a continue message
    fn on_continue(&mut self) {
        self.running = true;
    }

    ///Handle a stop message: a recording in progress has to be started again
    fn on_stop<F>(&mut self, frame: u64, output: &mut F)
    where
        F: FnMut(u64, &[u8]),
    {
        self.play(frame, output);
        self.running = false;
        match self.state {
            LoopState::Recording => self.state = LoopState::Waiting,
            LoopState::Playing | LoopState::Stopping => self.stop_sounding_notes(frame, output),
            _ => {}
        }
    }

    ///Handle a clock message
    fn on_clock<F>(&mut self, frame: u64, conf: &Configuration, output: &mut F)
    where
        F: FnMut(u64, &[u8]),
    {
        if self.running {
            match self.state {
                LoopState::Inactive | LoopState::Stopped => {}
                LoopState::Waiting => {
                    if self.transport_position % conf.clocks_per_bar() == 0 {
                        self.events.clear();
                        self.held = [[false; 128]; 16];
                        self.loop_clocks = conf.clocks_per_loop();
                        self.loop_position = 0;
                        self.state = LoopState::Recording;
                    }
                }
                LoopState::WaitingToPlay => {
                    if self.transport_position % conf.clocks_per_bar() == 0 {
                        self.loop_position = 0;
                        self.next_event = 0;
                        self.state = LoopState::Playing;
                    }
                }
                LoopState::Recording => {
                    self.loop_position += 1;
                    if self.loop_position == self.loop_clocks {
                        self.stop_held_notes();
                        self.loop_position = 0;
                        self.next_event = 0;
                        self.state = LoopState::Playing;
                    }
                }
                LoopState::Playing | LoopState::Stopping => {
                    if self.restart_pending {
                        self.restart_pending = false;
                        self.loop_position = 0;
                        self.next_event = 0;
                    } else {
                        self.finish_clock(frame, output);
                        self.loop_position += 1;
                        if self.loop_position == self.loop_clocks {
                            self.loop_position = 0;
                            self.next_event = 0;
                            if self.state == LoopState::Stopping {
                                self.stop_sounding_notes(frame, output);
                                self.state = LoopState::Stopped;
                            }
                        }
                    }
                }
            }
            self.transport_position += 1;
        }

        if let Some(last_frame) = self.last_clock_frame {
            self.clock_period = (frame - last_frame) as f64;
        }
        self.last_clock_frame = Some(frame);
    }

    ///Handle a midi command received on the regular input
    fn on_midi(&mut self, frame: u64, bytes: &[u8]) {
        //the system messages are not linked to a position in the loop
        if self.state != LoopState::Recording || bytes.len() > MAX_EVENT_LEN || bytes[0] >= 0xF0 {
            return;
        }

        if let Some((channel, note, starting)) = note_change(bytes) {
            //a note started before the recording cannot be stopped by the loop
            if !starting && !self.held[channel][note] {
                return;
            }
            self.held[channel][note] = starting;
        }

        let mut stored = [0; MAX_EVENT_LEN];
        stored[..bytes.len()].copy_from_slice(bytes);
        let mut fraction = 0.0;
        if let Some(last_frame) = self.last_clock_frame {
            if self.clock_period > 0.0 {
                fraction = (frame - last_frame) as f64 / self.clock_period;
            }
        }
        self.events.push(LoopEvent {
            position: self.loop_position as f64 + fraction.min(1.0 - 2.0 * NOTE_END_MARGIN),
            bytes: stored,
            len: bytes.len(),
        });
    }

    ///Replay the midi commands that are expected before a given frame
    fn play<F>(&mut self, until_frame: u64, output: &mut F)
    where
        F: FnMut(u64, &[u8]),
    {
        let playing = matches!(self.state, LoopState::Playing | LoopState::Stopping);
        if !playing || !self.running || self.restart_pending {
            return;
        }
        let last_clock_frame = match self.last_clock_frame {
            Some(v) => v,
            None => return,
        };
        while self.next_event < self.events.len() {
            let event = self.events[self.next_event];
            let fraction = event.position - self.loop_position as f64;
            if fraction >= 1.0 {
                return;
            }
            let frame = last_clock_frame + (fraction * self.clock_period) as u64;
            if frame >= until_frame {
                return;
            }
            self.send_event(event, frame, output);
        }
    }

    ///Replay the midi commands that are expected before the next clock message
    fn finish_clock<F>(&mut self, clock_frame: u64, output: &mut F)
    where
        F: FnMut(u64, &[u8]),
    {
        self.play(clock_frame, output);
        while self.next_event < self.events.len() {
            let event = self.events[self.next_event];
            if event.position >= (self.loop_position + 1) as f64 {
                return;
            }
            self.send_event(event, clock_frame, output);
        }
    }

    ///Send a stored midi command to the output
    fn send_event<F>(&mut self, event: LoopEvent, frame: u64, output: &mut F)
    where
        F: FnMut(u64, &[u8]),
    {
        let bytes = &event.bytes[..event.len];
        if let Some((channel, note, starting)) = note_change(bytes) {
            self.sounding[channel][note] = starting;
        }
        output(frame, bytes);
        self.next_event += 1;
    }

    ///Add the end of the notes that are still held at the end of the recording.
    /// Otherwise, they would never stop during the replay
    fn stop_held_notes(&mut self) {
        let position = self.loop_clocks as f64 - NOTE_END_MARGIN;
        for channel in 0..self.held.len() {
            for note in 0..self.held[channel].len() {
                if self.held[channel][note] {
                    self.held[channel][note] = false;
                    self.events.push(LoopEvent {
                        position,
                        bytes: [NOTE_END | channel as u8, note as u8, 0],
                        len: MAX_EVENT_LEN,
                    });
                }
            }
        }
    }

    ///Stop every note that is currently being replayed
    fn stop_sounding_notes<F>(&mut self, frame: u64, output: &mut F)
    where
        F: FnMut(u64, &[u8]),
    {
        for channel in 0..self.sounding.len() {
            for note in 0..self.sounding[channel].len() {
                if self.sounding[channel][note] {
                    self.sounding[channel][note] = false;
                    output(frame, &[NOTE_END | channel as u8, note as u8, 0]);
                }
            }
        }
    }
}

struct Loop {
    /// The midi input receiving the clock of the metronome
    clock_in: jack::Port<jack::MidiIn>,
    /// The midi input to record and to listen to for the commands
    midi_in: jack::Port<jack::MidiIn>,
    /// The midi output on which the loop is replayed
    midi_out: jack::Port<jack::MidiOut>,
    ///The incoming messages from the UI
    messages_in: std::sync::mpsc::Receiver<MessageToLoop>,
    ///The outgoing messages to the UI
    messages_out: std::sync::mpsc::Sender<MessageToLoopUI>,
    ///If defined, the next control will be mapped to this key
    key_change: Option<KeyBoardKey>,
    ///The keyboard events we are listening to
    keyboard: HardWare,
    /// The configuration
    conf: Configuration,
    /// The logic of the recording and of the replay
    looper: Looper,
    /// The number of frames processed since the start of the program
    frames_elapsed: u64,
    /// The number of quarters per bar of a time signature that applies from the next bar
    pending_quarters: Option<usize>,
}

impl Loop {
    pub fn new(
        client: &jack::Client,
        messages_in: std::sync::mpsc::Receiver<MessageToLoop>,
        mut messages_out: std::sync::mpsc::Sender<MessageToLoopUI>,
        state: ConfigurationState<Configuration>,
    ) -> Result<Loop, CommonError> {
        let c_in = match client.register_port("clock_in", jack::MidiIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiIn, e)),
        };
        let m_in = match client.register_port("midi_in", jack::MidiIn::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiIn, e)),
        };
        let m_out = match client.register_port("midi_out", jack::MidiOut::default()) {
            Ok(v) => v,
            Err(e) => return Err(CommonError::ConnectionError(ConnectionType::MidiOut, e)),
        };

        let current_config = state.conf;

        //initialize the configuration on the UI side
        Self::send_message(
            MessageToLoopUI::NewConfig(current_config.clone()),
            &mut messages_out,
        );

        Ok(Loop {
            clock_in: c_in,
            midi_in: m_in,
            midi_out: m_out,
            messages_in,
            messages_out,
            key_change: None,
            keyboard: state.keyboard,
            conf: current_config,
            looper: Looper::new(),
            frames_elapsed: 0,
            pending_quarters: None,
        })
    }

    ///Use the number of quarters per bar given by a time signature
    fn apply_quarters(
        conf: &mut Configuration,
        quarters: usize,
        messages_out: &mut std::sync::mpsc::Sender<MessageToLoopUI>,
    ) {
        if conf.nb_quarters.set_value(quarters) {
            Self::send_message(MessageToLoopUI::NewConfig(conf.clone()), messages_out);
        }
    }

    fn send_message(
        msg: MessageToLoopUI,
        messages_out: &mut std::sync::mpsc::Sender<MessageToLoopUI>,
    ) {
        if let Err(e) = messages_out.send(msg) {
            eprintln!("Internal error: {e}");
        }
    }
}

impl Stateful for Loop {
    type State = ConfigurationState<Configuration>;

    fn publish(&self, state: &mut Self::State) {
        state.update(&self.conf, &self.keyboard);
    }
}

impl jack::ProcessHandler for Loop {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        let first_frame = self.frames_elapsed;
        let last_frame = first_frame + ps.n_frames() as u64;
        let initial_state = self.looper.state;

        let mut writer = self.midi_out.writer(ps);
        //the midi commands of a buffer have to be written in chronological order
        let mut last_time = 0;
        let mut output = |frame: u64, bytes: &[u8]| {
            let time = (frame.saturating_sub(first_frame) as jack::Frames).max(last_time);
            last_time = time;
            if let Err(e) = writer.write(&jack::RawMidi { time, bytes }) {
                eprintln!("Internal error: {e}");
            }
        };

        if let Ok(message) = self.messages_in.try_recv() {
            match message {
                MessageToLoop::Active(active) => {
                    self.looper.set_active(active, first_frame, &mut output)
                }
                MessageToLoop::Replay(command) => {
                    self.looper.on_command(command, first_frame, &mut output)
                }
                MessageToLoop::ChangeMidiKey(key) => self.key_change = Some(key),
                MessageToLoop::ClearMidiKey(key) => self.keyboard.clear_key(key),
                MessageToLoop::NewConfig(configuration) => self.conf = configuration,
            }
        }

        //both inputs are read in chronological order. The clock goes first
        //to have a note played on a beat located right after this beat
        let mut clock_events = self.clock_in.iter(ps).peekable();
        let mut midi_events = self.midi_in.iter(ps).peekable();
        loop {
            let clock_time = clock_events.peek().map(|e| e.time);
            let midi_time = midi_events.peek().map(|e| e.time);
            let read_clock = match (clock_time, midi_time) {
                (None, None) => break,
                (Some(_), None) => true,
                (None, Some(_)) => false,
                (Some(clock), Some(midi)) => clock <= midi,
            };

            if read_clock {
                let event = clock_events.next().unwrap();
                let frame = first_frame + event.time as u64;
                let midi: MidiInput = event.into();
                match midi {
                    MidiInput::Clock { timing: _ } => {
                        self.looper.on_clock(frame, &self.conf, &mut output)
                    }
                    MidiInput::Start { timing: _ } => self.looper.on_start(frame, &mut output),
                    MidiInput::Continue { timing: _ } => self.looper.on_continue(),
                    MidiInput::Stop { timing: _ } => self.looper.on_stop(frame, &mut output),
                    MidiInput::BarMarker { bar, .. } => {
                        if let Some(quarters) = self.pending_quarters.take() {
                            Self::apply_quarters(&mut self.conf, quarters, &mut self.messages_out);
                        }
                        self.looper.on_bar_marker(bar);
                    }
                    MidiInput::TimeSignature {
                        delayed,
                        numerator,
                        denominator_power,
                        ..
                    } => {
                        if let Some(quarters) = quarters_per_bar(numerator, denominator_power) {
                            if delayed {
                                self.pending_quarters = Some(quarters);
                            } else {
                                Self::apply_quarters(
                                    &mut self.conf,
                                    quarters,
                                    &mut self.messages_out,
                                );
                            }
                        }
                    }
                    _ => {}
                }
            } else {
                let event = midi_events.next().unwrap();
                let frame = first_frame + event.time as u64;
                self.looper.play(frame, &mut output);

                let midi: MidiInput = event.into();
                let mut is_command = false;
                if let MidiInput::Controller {
                    channel: _,
                    control,
                    value,
                } = midi
                {
                    if let Some(k) = self.key_change {
                        self.keyboard.update_key(k, control);
                        self.key_change = None;
                        is_command = true;
                    } else if let Some(key) = self.keyboard.get_keyboard_key(control) {
                        is_command = true;
                        if key == KeyBoardKey::Activate {
                            if value > 0 {
                                let active = self.looper.state == LoopState::Inactive;
                                self.looper.set_active(active, frame, &mut output);
                            }
                        } else if let Some(command) = ReplayCommand::from_key(key) {
                            if value > 0 {
                                self.looper.on_command(command, frame, &mut output);
                            }
                        } else if configuration::Configuration::apply_midi(
                            &mut self.conf,
                            key,
                            value,
                        ) {
                            Self::send_message(
                                MessageToLoopUI::NewConfig(self.conf.clone()),
                                &mut self.messages_out,
                            );
                        }
                    }
                }

                if !is_command {
                    self.looper.on_midi(frame, event.bytes);
                }
            }
        }

        self.looper.play(last_frame, &mut output);
        self.frames_elapsed = last_frame;

        if self.looper.state != initial_state {
            Self::send_message(
                MessageToLoopUI::NewState(self.looper.state),
                &mut self.messages_out,
            );
        }

        jack::Control::Continue
    }
}

#[derive(Debug)]
enum MessageToLoop {
    Active(bool),
    Replay(ReplayCommand),
    ChangeMidiKey(KeyBoardKey),
    ClearMidiKey(KeyBoardKey),
    NewConfig(Configuration),
}

impl From<KeyBoardKeySetter> for MessageToLoop {
    fn from(value: KeyBoardKeySetter) -> Self {
        match value {
            KeyBoardKeySetter::Set(k) => MessageToLoop::ChangeMidiKey(k),
            KeyBoardKeySetter::Clear(k) => MessageToLoop::ClearMidiKey(k),
        }
    }
}

#[derive(Debug)]
enum MessageToLoopUI {
    NewConfig(Configuration),
    NewState(LoopState),
}

struct LoopUI {
    messages_in: std::sync::mpsc::Receiver<MessageToLoopUI>,
    message_out: std::sync::mpsc::Sender<MessageToLoop>,
    messages: Vec<String>,
    current_config: Option<Configuration>,
    state: LoopState,
}

impl LoopUI {
    pub fn new(
        cc: &eframe::CreationContext<'_>,
        messages_in: std::sync::mpsc::Receiver<MessageToLoopUI>,
        messages_out: std::sync::mpsc::Sender<MessageToLoop>,
    ) -> LoopUI {
        nsm::set_ui_context(&cc.egui_ctx);
        LoopUI {
            messages_in,
            message_out: messages_out,
            messages: Vec::new(),
            current_config: None,
            state: LoopState::Inactive,
        }
    }

    fn create_menu(&mut self, ui: &mut egui::Ui) {
        egui::MenuBar::new().ui(ui, |ui| {
            ui.menu_button("Settings", |ui| {
                create_keyboard_select(
                    ui,
                    "Keyboard activate key",
                    KeyBoardKey::Activate,
                    &mut self.message_out,
                    &mut self.messages,
                );
                for (name, key) in [
                    ("Keyboard play key", KeyBoardKey::Play),
                    ("Keyboard stop key", KeyBoardKey::Stop),
                    ("Keyboard break key", KeyBoardKey::Break),
                ] {
                    create_keyboard_select(
                        ui,
                        name,
                        key,
                        &mut self.message_out,
                        &mut self.messages,
                    );
                }
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
        let active = self.state != LoopState::Inactive;
        if ui
            .button(if active { "De-activate" } else { "Activate" })
            .clicked()
        {
            if let Err(e) = self.message_out.send(MessageToLoop::Active(!active)) {
                self.messages.push(format!("Internal error: {e}"));
            }
        }
        //only the commands that apply to the current state are enabled
        let state = self.state;
        let commands = [
            (
                "Play",
                ReplayCommand::Play,
                matches!(state, LoopState::Stopped | LoopState::Stopping),
            ),
            (
                "Stop",
                ReplayCommand::Stop,
                matches!(state, LoopState::Playing | LoopState::WaitingToPlay),
            ),
            (
                "Break",
                ReplayCommand::Break,
                matches!(
                    state,
                    LoopState::Playing | LoopState::Stopping | LoopState::WaitingToPlay
                ),
            ),
        ];
        ui.horizontal(|ui| {
            for (name, command, enabled) in commands {
                if ui.add_enabled(enabled, egui::Button::new(name)).clicked() {
                    if let Err(e) = self.message_out.send(MessageToLoop::Replay(command)) {
                        self.messages.push(format!("Internal error: {e}"));
                    }
                }
            }
        });
        ui.label(format!("Status: {}", self.state));

        if let Some(current_config) = &self.current_config {
            let mut conf = current_config.clone();

            configuration::Configuration::draw(&mut conf, ui);

            if !self.current_config.as_ref().eq(&Some(&conf)) {
                if let Err(e) = self
                    .message_out
                    .send(MessageToLoop::NewConfig(conf.clone()))
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
                MessageToLoopUI::NewConfig(cfg) => self.current_config = Some(cfg),
                MessageToLoopUI::NewState(state) => self.state = state,
            },
        }
    }
}

impl eframe::App for LoopUI {
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

///Note that 'loop' is a keyword: it cannot be used as the name of this function
pub fn looper() -> Result<(), CommonError> {
    // open client
    let session = Session::start("Loop")?;
    let client = session.client("loop")?;

    //open a message channel for the loop and the UI
    let (send_to_loop, rcv_from_ui) = std::sync::mpsc::channel();
    let (send_to_ui, rcv_from_loop) = std::sync::mpsc::channel();

    let state = session.load(ConfigurationState::new(Configuration::new()));
    let utility = Loop::new(&client, rcv_from_ui, send_to_ui, state.clone())?;
    let title = session.window_title("Loop");
    let active_client = session.activate(client, utility, state)?;

    nsm::run_window(
        &title,
        eframe::NativeOptions {
            viewport: ViewportBuilder::default().with_inner_size(egui::vec2(320.0, 640.0)),
            run_and_return: true,
            ..Default::default()
        },
        Box::new(|cc| Ok(Box::new(LoopUI::new(cc, rcv_from_loop, send_to_loop)))),
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
    use super::{
        quarters_per_bar, Configuration, LoopState, Looper, ReplayCommand, CLOCKS_PER_QUARTER,
    };
    use crate::midiinput::{BAR_NOT_RUNNING, BAR_UNKNOWN};
    use crate::{configuration::UsizeValueInRange, synth::hardware::KeyBoardKey};

    ///The number of frames between two clock messages
    const PERIOD: u64 = 100;
    const NOTE_ON: [u8; 3] = [0x90, 60, 100];
    const NOTE_OFF: [u8; 3] = [0x80, 60, 0];

    type Sent = Vec<(u64, Vec<u8>)>;

    ///A loop of a single bar of a single quarter note
    fn conf() -> Configuration {
        let key = KeyBoardKey::Parameter;
        Configuration {
            nb_bars: UsizeValueInRange::new(1, 1, 16, "bars", key),
            nb_quarters: UsizeValueInRange::new(1, 1, 16, "quarters", key),
        }
    }

    ///Send the clock messages between two clock indexes
    fn clocks(looper: &mut Looper, clocks: std::ops::Range<u64>, sent: &mut Sent) {
        let conf = conf();
        for clock in clocks {
            looper.on_clock(clock * PERIOD, &conf, &mut |frame, bytes: &[u8]| {
                sent.push((frame, bytes.to_vec()))
            });
        }
    }

    fn play(looper: &mut Looper, until: u64, sent: &mut Sent) {
        looper.play(until, &mut |frame, bytes: &[u8]| {
            sent.push((frame, bytes.to_vec()))
        });
    }

    fn activate(looper: &mut Looper) {
        looper.set_active(true, 0, &mut |_, _: &[u8]| panic!("unexpected output"));
    }

    ///Record a note lasting from the clock 2.5 to the clock 4.5
    fn record_note(looper: &mut Looper, sent: &mut Sent) {
        activate(looper);
        clocks(looper, 0..3, sent);
        looper.on_midi(2 * PERIOD + PERIOD / 2, &NOTE_ON);
        clocks(looper, 3..5, sent);
        looper.on_midi(4 * PERIOD + PERIOD / 2, &NOTE_OFF);
        clocks(looper, 5..CLOCKS_PER_QUARTER as u64, sent);
    }

    #[test]
    fn inactive_records_nothing() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        clocks(&mut looper, 0..3, &mut sent);
        looper.on_midi(250, &NOTE_ON);
        clocks(&mut looper, 3..100, &mut sent);
        play(&mut looper, 100 * PERIOD, &mut sent);
        assert_eq!(looper.state, LoopState::Inactive);
        assert!(looper.events.is_empty());
        assert!(sent.is_empty());
    }

    #[test]
    fn recording_starts_with_the_next_bar() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        clocks(&mut looper, 0..10, &mut sent);
        activate(&mut looper);
        looper.on_midi(950, &NOTE_ON);
        assert_eq!(looper.state, LoopState::Waiting);
        assert!(looper.events.is_empty());
        clocks(&mut looper, 10..24, &mut sent);
        assert_eq!(looper.state, LoopState::Waiting);
        clocks(&mut looper, 24..25, &mut sent);
        assert_eq!(looper.state, LoopState::Recording);
    }

    #[test]
    fn bar_marker_gives_the_first_beat() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        clocks(&mut looper, 0..5, &mut sent);
        activate(&mut looper);
        assert_eq!(looper.state, LoopState::Waiting);
        looper.on_bar_marker(3);
        clocks(&mut looper, 5..6, &mut sent);
        assert_eq!(looper.state, LoopState::Recording);
    }

    #[test]
    fn unknown_bar_number_still_gives_the_first_beat() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        clocks(&mut looper, 0..5, &mut sent);
        activate(&mut looper);
        looper.on_bar_marker(BAR_UNKNOWN);
        clocks(&mut looper, 5..6, &mut sent);
        assert_eq!(looper.state, LoopState::Recording);
    }

    #[test]
    fn stopped_bar_marker_is_ignored() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        clocks(&mut looper, 0..5, &mut sent);
        activate(&mut looper);
        looper.on_bar_marker(BAR_NOT_RUNNING);
        clocks(&mut looper, 5..6, &mut sent);
        assert_eq!(looper.state, LoopState::Waiting);
    }

    #[test]
    fn time_signatures_in_quarters() {
        assert_eq!(quarters_per_bar(4, 2), Some(4));
        assert_eq!(quarters_per_bar(3, 2), Some(3));
        assert_eq!(quarters_per_bar(6, 3), Some(3));
        assert_eq!(quarters_per_bar(2, 1), Some(4));
        assert_eq!(quarters_per_bar(7, 3), None);
        assert_eq!(quarters_per_bar(5, 4), None);
        assert_eq!(quarters_per_bar(0, 2), None);
        assert_eq!(quarters_per_bar(4, 100), None);
    }

    fn command(looper: &mut Looper, command: ReplayCommand, frame: u64, sent: &mut Sent) {
        looper.on_command(command, frame, &mut |f, bytes: &[u8]| {
            sent.push((f, bytes.to_vec()))
        });
    }

    #[test]
    fn break_silences_and_keeps_the_loop() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        record_note(&mut looper, &mut sent);
        clocks(&mut looper, 24..27, &mut sent);
        play(&mut looper, 2670, &mut sent);
        command(&mut looper, ReplayCommand::Break, 2670, &mut sent);
        assert_eq!(looper.state, LoopState::Stopped);
        assert_eq!(
            sent,
            vec![(2650, NOTE_ON.to_vec()), (2670, NOTE_OFF.to_vec())]
        );

        clocks(&mut looper, 27..100, &mut sent);
        play(&mut looper, 100 * PERIOD, &mut sent);
        assert_eq!(sent.len(), 2);
        assert!(!looper.events.is_empty());
    }

    #[test]
    fn stop_finishes_the_loop_without_repeating_it() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        record_note(&mut looper, &mut sent);
        clocks(&mut looper, 24..27, &mut sent);
        play(&mut looper, 2670, &mut sent);
        command(&mut looper, ReplayCommand::Stop, 2670, &mut sent);
        assert_eq!(looper.state, LoopState::Stopping);
        assert_eq!(sent, vec![(2650, NOTE_ON.to_vec())]);

        clocks(&mut looper, 27..48, &mut sent);
        assert_eq!(looper.state, LoopState::Stopping);
        clocks(&mut looper, 48..100, &mut sent);
        play(&mut looper, 100 * PERIOD, &mut sent);
        assert_eq!(looper.state, LoopState::Stopped);
        assert_eq!(
            sent,
            vec![(2650, NOTE_ON.to_vec()), (2850, NOTE_OFF.to_vec())]
        );
    }

    #[test]
    fn stop_ends_the_notes_held_until_the_end_of_the_loop() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        activate(&mut looper);
        clocks(&mut looper, 0..3, &mut sent);
        looper.on_midi(250, &NOTE_ON);
        clocks(&mut looper, 3..27, &mut sent);
        play(&mut looper, 2700, &mut sent);
        command(&mut looper, ReplayCommand::Stop, 2700, &mut sent);
        clocks(&mut looper, 27..100, &mut sent);
        assert_eq!(looper.state, LoopState::Stopped);
        assert_eq!(sent.len(), 2);
        assert_eq!(sent[1].1, NOTE_OFF.to_vec());
        assert!(sent[1].0 <= 4800);
    }

    #[test]
    fn play_cancels_the_stop() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        record_note(&mut looper, &mut sent);
        clocks(&mut looper, 24..27, &mut sent);
        command(&mut looper, ReplayCommand::Stop, 2700, &mut sent);
        command(&mut looper, ReplayCommand::Play, 2700, &mut sent);
        assert_eq!(looper.state, LoopState::Playing);
        clocks(&mut looper, 27..51, &mut sent);
        play(&mut looper, 51 * PERIOD, &mut sent);
        assert_eq!(
            sent,
            vec![
                (2650, NOTE_ON.to_vec()),
                (2850, NOTE_OFF.to_vec()),
                (5050, NOTE_ON.to_vec()),
            ]
        );
    }

    #[test]
    fn break_while_stopping() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        record_note(&mut looper, &mut sent);
        clocks(&mut looper, 24..27, &mut sent);
        play(&mut looper, 2670, &mut sent);
        command(&mut looper, ReplayCommand::Stop, 2670, &mut sent);
        command(&mut looper, ReplayCommand::Break, 2680, &mut sent);
        assert_eq!(looper.state, LoopState::Stopped);
        assert_eq!(sent[1], (2680, NOTE_OFF.to_vec()));
        clocks(&mut looper, 27..100, &mut sent);
        assert_eq!(sent.len(), 2);
    }

    #[test]
    fn replay_starts_again_with_the_next_bar() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        record_note(&mut looper, &mut sent);
        clocks(&mut looper, 24..27, &mut sent);
        command(&mut looper, ReplayCommand::Break, 2700, &mut sent);
        clocks(&mut looper, 27..40, &mut sent);
        sent.clear();

        command(&mut looper, ReplayCommand::Play, 4000, &mut sent);
        assert_eq!(looper.state, LoopState::WaitingToPlay);
        clocks(&mut looper, 40..48, &mut sent);
        play(&mut looper, 48 * PERIOD, &mut sent);
        assert!(sent.is_empty());

        clocks(&mut looper, 48..51, &mut sent);
        play(&mut looper, 51 * PERIOD, &mut sent);
        assert_eq!(looper.state, LoopState::Playing);
        assert_eq!(sent, vec![(5050, NOTE_ON.to_vec())]);
    }

    #[test]
    fn stop_or_break_before_the_replay_starts_again() {
        for cancel in [ReplayCommand::Stop, ReplayCommand::Break] {
            let mut looper = Looper::new();
            let mut sent = Vec::new();
            record_note(&mut looper, &mut sent);
            clocks(&mut looper, 24..25, &mut sent);
            command(&mut looper, ReplayCommand::Break, 2400, &mut sent);
            command(&mut looper, ReplayCommand::Play, 2400, &mut sent);
            command(&mut looper, cancel, 2400, &mut sent);
            clocks(&mut looper, 25..100, &mut sent);
            assert_eq!(looper.state, LoopState::Stopped);
            assert!(sent.is_empty());
        }
    }

    #[test]
    fn commands_are_ignored_without_loop() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        command(&mut looper, ReplayCommand::Play, 0, &mut sent);
        assert_eq!(looper.state, LoopState::Inactive);
        activate(&mut looper);
        clocks(&mut looper, 0..3, &mut sent);
        for c in [
            ReplayCommand::Stop,
            ReplayCommand::Break,
            ReplayCommand::Play,
        ] {
            command(&mut looper, c, 300, &mut sent);
            assert_eq!(looper.state, LoopState::Recording);
        }
    }

    #[test]
    fn de_activation_forgets_a_stopped_loop() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        record_note(&mut looper, &mut sent);
        clocks(&mut looper, 24..25, &mut sent);
        command(&mut looper, ReplayCommand::Break, 2400, &mut sent);
        looper.set_active(false, 2400, &mut |_, _: &[u8]| panic!("unexpected output"));
        assert_eq!(looper.state, LoopState::Inactive);
        assert!(looper.events.is_empty());
    }

    #[test]
    fn nothing_is_sent_before_the_end_of_the_recording() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        record_note(&mut looper, &mut sent);
        assert_eq!(looper.state, LoopState::Recording);
        assert!(sent.is_empty());
    }

    #[test]
    fn notes_are_replayed_at_every_loop() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        record_note(&mut looper, &mut sent);
        clocks(&mut looper, 24..72, &mut sent);
        play(&mut looper, 72 * PERIOD, &mut sent);
        assert_eq!(looper.state, LoopState::Playing);
        assert_eq!(
            sent,
            vec![
                (2650, NOTE_ON.to_vec()),
                (2850, NOTE_OFF.to_vec()),
                (5050, NOTE_ON.to_vec()),
                (5250, NOTE_OFF.to_vec()),
            ]
        );
    }

    #[test]
    fn replay_follows_the_tempo() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        record_note(&mut looper, &mut sent);
        clocks(&mut looper, 24..25, &mut sent);
        //the clock is now twice as slow
        let conf = conf();
        let mut frame = 24 * PERIOD;
        for _ in 0..5 {
            frame += 2 * PERIOD;
            looper.on_clock(frame, &conf, &mut |f, bytes: &[u8]| {
                sent.push((f, bytes.to_vec()))
            });
        }
        play(&mut looper, frame, &mut sent);
        assert_eq!(
            sent,
            vec![(2900, NOTE_ON.to_vec()), (3300, NOTE_OFF.to_vec())]
        );
    }

    #[test]
    fn held_notes_stop_at_the_end_of_the_loop() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        activate(&mut looper);
        clocks(&mut looper, 0..3, &mut sent);
        looper.on_midi(250, &NOTE_ON);
        clocks(&mut looper, 3..49, &mut sent);
        assert_eq!(sent[1].1, NOTE_OFF.to_vec());
        assert!(sent[1].0 <= 4800);
        assert!(sent[0].0 < sent[1].0);
    }

    #[test]
    fn notes_started_before_the_recording_are_not_stopped() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        activate(&mut looper);
        clocks(&mut looper, 0..3, &mut sent);
        looper.on_midi(250, &NOTE_OFF);
        assert!(looper.events.is_empty());
    }

    #[test]
    fn other_commands_are_replayed_and_system_messages_ignored() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        activate(&mut looper);
        clocks(&mut looper, 0..3, &mut sent);
        looper.on_midi(250, &[0xB0, 1, 64]);
        looper.on_midi(250, &[0xF8]);
        looper.on_midi(250, &[0xF0, 1, 2, 3, 0xF7]);
        clocks(&mut looper, 3..27, &mut sent);
        play(&mut looper, 27 * PERIOD, &mut sent);
        assert_eq!(sent, vec![(2650, vec![0xB0, 1, 64])]);
    }

    #[test]
    fn de_activation_stops_the_notes_and_forgets_the_loop() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        record_note(&mut looper, &mut sent);
        clocks(&mut looper, 24..28, &mut sent);
        play(&mut looper, 27 * PERIOD + 70, &mut sent);
        looper.set_active(false, 2770, &mut |frame, bytes: &[u8]| {
            sent.push((frame, bytes.to_vec()))
        });
        assert_eq!(looper.state, LoopState::Inactive);
        assert_eq!(sent.last().unwrap(), &(2770, NOTE_OFF.to_vec()));
        assert!(looper.events.is_empty());
    }

    #[test]
    fn stop_silences_and_continue_resumes() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        record_note(&mut looper, &mut sent);
        clocks(&mut looper, 24..27, &mut sent);
        play(&mut looper, 2670, &mut sent);
        assert_eq!(sent, vec![(2650, NOTE_ON.to_vec())]);

        looper.on_stop(2670, &mut |frame, bytes: &[u8]| {
            sent.push((frame, bytes.to_vec()))
        });
        assert_eq!(sent[1], (2670, NOTE_OFF.to_vec()));

        //the metronome keeps sending its clock while being stopped
        clocks(&mut looper, 27..40, &mut sent);
        assert_eq!(sent.len(), 2);

        looper.on_continue();
        clocks(&mut looper, 40..43, &mut sent);
        assert_eq!(sent.len(), 3);
        assert_eq!(sent[2], (4150, NOTE_OFF.to_vec()));
    }

    #[test]
    fn stop_cancels_the_recording() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        activate(&mut looper);
        clocks(&mut looper, 0..3, &mut sent);
        looper.on_stop(300, &mut |_, _: &[u8]| panic!("unexpected output"));
        assert_eq!(looper.state, LoopState::Waiting);
    }

    #[test]
    fn start_restarts_the_replay_from_the_beginning() {
        let mut looper = Looper::new();
        let mut sent = Vec::new();
        record_note(&mut looper, &mut sent);
        clocks(&mut looper, 24..30, &mut sent);
        sent.clear();

        looper.on_start(3000, &mut |frame, bytes: &[u8]| {
            sent.push((frame, bytes.to_vec()))
        });
        assert!(sent.is_empty());
        clocks(&mut looper, 30..33, &mut sent);
        play(&mut looper, 33 * PERIOD, &mut sent);
        assert_eq!(sent, vec![(3250, NOTE_ON.to_vec())]);
    }
}
