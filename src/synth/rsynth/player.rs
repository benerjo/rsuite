use std::{
    fmt::Display,
    io::{prelude::*, Write},
};

use crate::synth::{
    hardware::{HardWare, KeyBoardKey},
    rsynth::configuration::{Configuration, ConfigurationChange},
};
use crate::{midiinput::MidiInput, utils::KeyBoardKeySetter};

pub const FADE_DURATION_STEP: f64 = 0.025;
pub const MIDI_KEYBOARD_CONF_LOCATION: &str = "rsynth.ron";
pub const GAIN_STEP: f64 = 8.0 / 127.0;
pub const OVERTONE_STEP: f64 = 1.0 / 128.0;

///This enum represent the different elements that can change for the player
#[derive(Debug)]
pub enum PlayerChange {
    Message(String),
    Error(PlayerError),
}

#[derive(Debug)]
pub enum ExternalPlayerInput {
    NewKeyboardKey(KeyBoardKey),
    ClearKeybaordKey(KeyBoardKey),
    ClearAllKeyboardKeys,
    SaveConf,
    LoadConf,
}

impl From<KeyBoardKeySetter> for ExternalPlayerInput {
    fn from(value: KeyBoardKeySetter) -> Self {
        match value {
            KeyBoardKeySetter::Set(k) => ExternalPlayerInput::NewKeyboardKey(k),
            KeyBoardKeySetter::Clear(k) => ExternalPlayerInput::ClearKeybaordKey(k),
        }
    }
}

#[derive(Debug)]
pub enum PlayerError {
    JackError(jack::Error),
    FileError(std::io::Error),
    CommunicationError(std::sync::mpsc::TryRecvError),
    RonError(ron::error::Error),
    RonSpannedError(ron::error::SpannedError),
}

impl From<jack::Error> for PlayerError {
    fn from(value: jack::Error) -> Self {
        PlayerError::JackError(value)
    }
}

impl From<std::io::Error> for PlayerError {
    fn from(value: std::io::Error) -> Self {
        PlayerError::FileError(value)
    }
}

impl From<ron::error::Error> for PlayerError {
    fn from(value: ron::error::Error) -> Self {
        PlayerError::RonError(value)
    }
}

impl From<ron::error::SpannedError> for PlayerError {
    fn from(value: ron::error::SpannedError) -> Self {
        PlayerError::RonSpannedError(value)
    }
}

impl Display for PlayerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayerError::JackError(e) => write!(f, "{e}"),
            PlayerError::FileError(e) => write!(f, "{e}"),
            PlayerError::CommunicationError(e) => write!(f, "{e}"),
            PlayerError::RonError(e) => write!(f, "{e}"),
            PlayerError::RonSpannedError(e) => write!(f, "{e}"),
        }
    }
}

pub struct Player {
    rate: usize,
    /// The duration of a single audio frame
    frame_t: f64,
    /// Time dilation
    time_dilation_factor: f64,
    /// The time that has passed since the beginning
    time: f64,
    /// The input midi port
    midi_in: jack::Port<jack::MidiIn>,
    /// The output audio port
    audio_mono_out: jack::Port<jack::AudioOut>,
    /// Listener to changes in the configuration
    change_listener: Option<std::sync::mpsc::Sender<PlayerChange>>,
    /// Listener to the external configuration changes
    config_listener: Option<std::sync::mpsc::Receiver<ConfigurationChange>>,
    /// The keyboard configuration
    keyboard: HardWare,
    /// The velocity that was used to activate a note
    velocity: Vec<f64>,
    /// Specify for each note if it should be played or not
    play: Vec<bool>,
    fade_in: Vec<f64>,
    fade_out: Vec<f64>,
    config: Configuration,
    ///The channel allowing to receive external commands
    external_commands: std::sync::mpsc::Receiver<ExternalPlayerInput>,
    ///If true, the next control input should be used for mapping
    map_next_contrl: Option<KeyBoardKey>,
}

impl Player {
    pub fn new(
        client: &jack::Client,
        extra_input: std::sync::mpsc::Receiver<ExternalPlayerInput>,
    ) -> Result<Player, PlayerError> {
        let sample_rate = client.sample_rate();

        let nb_notes = 12 * 12;

        let mut velocity_array = Vec::<f64>::with_capacity(nb_notes);
        let mut play_array = Vec::<bool>::with_capacity(nb_notes);
        let mut fin = Vec::<f64>::with_capacity(nb_notes);
        let mut fout = Vec::<f64>::with_capacity(nb_notes);

        for _f in 0..nb_notes {
            velocity_array.push(0.0);
            play_array.push(false);
            fin.push(1.0);
            fout.push(0.0);
        }

        let midi_keyboard = match Self::load_keyboard_conf() {
            Ok(v) => v,
            Err(_e) => HardWare::default(),
        };

        Ok(Player {
            rate: sample_rate,
            frame_t: 1.0 / sample_rate as f64,
            time_dilation_factor: 1.0,
            time: 0.0,
            midi_in: client.register_port("midi_input", jack::MidiIn::default())?,
            audio_mono_out: client.register_port("music_out", jack::AudioOut::default())?,
            change_listener: None,
            config_listener: None,
            keyboard: midi_keyboard,
            velocity: velocity_array,
            play: play_array,
            fade_in: fin,
            fade_out: fout,
            config: Configuration::new(),
            external_commands: extra_input,
            map_next_contrl: None,
        })
    }

    fn get_frequency(note_index: f64) -> f64 {
        let index = note_index + 1.0;
        let mid_a_freq = 440.0;
        let a5_index: f64 = 4.0 * 12.0 + 10.0;
        let b: f64 = (2.0 as f64).powf(1.0 / 12.0);
        let a: f64 = mid_a_freq / ((2.0 as f64).powf(a5_index / 12.0));
        a * b.powf(index)
    }

    /// Send a notification to the change listener
    fn send(
        change_listener: &mut Option<std::sync::mpsc::Sender<PlayerChange>>,
        to_send: PlayerChange,
    ) {
        if change_listener.is_some() {
            match change_listener.as_mut().unwrap().send(to_send) {
                Ok(()) => {}
                Err(e) => println!("Error while trying to send the change value notification: {e}"),
            }
        }
    }

    pub fn get_shape_factor(value: u8) -> f64 {
        if value > 64 {
            1.0 + ((value - 64) as f64) * 3.0 / 4.0
        } else {
            0.1 + (value as f64) / 64.0 * 0.9
        }
    }

    fn save_keyboard_conf(keyboard: &HardWare) -> Result<(), PlayerError> {
        let text = ron::to_string(&keyboard)?;
        let mut f = std::fs::File::create(MIDI_KEYBOARD_CONF_LOCATION)?;
        f.write_all(text.as_bytes())?;
        Ok(())
    }

    fn load_keyboard_conf() -> Result<HardWare, PlayerError> {
        let mut f = std::fs::File::open(MIDI_KEYBOARD_CONF_LOCATION)?;
        let mut buffer = String::new();
        f.read_to_string(&mut buffer)?;
        Ok(ron::from_str(&buffer)?)
    }

    fn read_input(&mut self, ps: &jack::ProcessScope) {
        match &mut self.config_listener {
            Some(config_changes) => {
                while let Ok(m) = config_changes.try_recv() {
                    self.config.apply_dont_send_notification(m)
                }
            }
            None => {}
        }

        match self.external_commands.try_recv() {
            Ok(v) => match v {
                ExternalPlayerInput::NewKeyboardKey(k) => self.map_next_contrl = Some(k),
                ExternalPlayerInput::ClearAllKeyboardKeys => self.keyboard.clear_all(),
                ExternalPlayerInput::SaveConf => {
                    match Self::save_keyboard_conf(&mut self.keyboard) {
                        Ok(()) => {}
                        Err(e) => {
                            Self::send(&mut self.change_listener, PlayerChange::Error(e));
                        }
                    }
                }
                ExternalPlayerInput::LoadConf => match Self::load_keyboard_conf() {
                    Ok(kb) => self.keyboard = kb,
                    Err(e) => Self::send(&mut self.change_listener, PlayerChange::Error(e)),
                },
                ExternalPlayerInput::ClearKeybaordKey(k) => self.keyboard.clear_key(k),
            },
            Err(e) => match e {
                std::sync::mpsc::TryRecvError::Empty => {}
                std::sync::mpsc::TryRecvError::Disconnected => Self::send(
                    &mut self.change_listener,
                    PlayerChange::Error(PlayerError::CommunicationError(e)),
                ),
            },
        };

        let show_p = self.midi_in.iter(ps);
        for e in show_p {
            let midi = e.into();

            match midi {
                MidiInput::NoteStart {
                    note_index,
                    timing: _,
                    velocity,
                    channel: _,
                } => {
                    if !self.play[note_index] {
                        self.velocity[note_index] = velocity;
                        self.play[note_index] = true;
                        //if we play before the fade_out was completed, continue from where we were
                        self.fade_in[note_index] = self.fade_out[note_index];
                    }
                }
                MidiInput::NoteEnd {
                    note_index,
                    channel: _,
                    timing: _,
                    velocity: _,
                } => {
                    self.play[note_index] = false;
                    //start the fade out not higher than the value of the fade_in. Otherwise,
                    //it would create a click
                    self.fade_out[note_index] = 1.0;
                }
                MidiInput::Controller {
                    control,
                    value,
                    channel: _,
                } => {
                    if self.map_next_contrl.is_some() {
                        let k = self.map_next_contrl.take().unwrap();
                        self.keyboard.update_key(k, control);
                    }
                    match self.keyboard.get_keyboard_key(control) {
                        None => {}
                        Some(v) => match v {
                            KeyBoardKey::WaveSelection => {
                                if value > 0 {
                                    self.config.cycle_wave_type();
                                }
                            }
                            KeyBoardKey::Overtone(overtone_index) => {
                                let new_value = (value as f64) * OVERTONE_STEP;
                                self.config
                                    .update_overtone(overtone_index as usize, new_value)
                            }
                            KeyBoardKey::FadeInDuration => self
                                .config
                                .set_fade_in_duration(FADE_DURATION_STEP * (1.0 + value as f64)),
                            KeyBoardKey::FadeInShape => {
                                self.config.set_fade_in_shape(Self::get_shape_factor(value));
                            }
                            KeyBoardKey::FadeOutDuration => {
                                let new_duration = FADE_DURATION_STEP * (1.0 + value as f64);
                                self.config.set_fade_out_duration(new_duration);
                            }
                            KeyBoardKey::FadeOutShape => {
                                self.config
                                    .set_fade_out_shape(Self::get_shape_factor(value));
                            }
                            KeyBoardKey::Gain => {
                                let new_gain = (1 + value) as f64 * GAIN_STEP;
                                self.config.set_gain(new_gain)
                            }
                            _ => {}
                        },
                    }
                }
                MidiInput::PitchBend { value } => {
                    if value < 64 {
                        self.time_dilation_factor = (value as f64) / 64.0
                    } else {
                        self.time_dilation_factor = 1.0 + (value - 64) as f64 / 64.0
                    }
                }
                MidiInput::Unknown {
                    d1: _,
                    d2: _,
                    d3: _,
                } => {}
            }
        }
    }

    fn compute_increment(rate: usize, duration: f64) -> f64 {
        1.0 / ((rate as f64) * duration)
    }

    ///Generate the sound buffer according to the current state
    fn generate_sound(&mut self, ps: &jack::ProcessScope) -> jack::Control {
        // Get output buffer
        let out = self.audio_mono_out.as_mut_slice(ps);

        // Write output
        for v in out.iter_mut() {
            let mut value: f64 = 0.0;
            let mut mute = true;

            let nb_notes = self.velocity.len();

            for note_index in 0..nb_notes {
                let fade = if self.play[note_index] || self.fade_in[note_index] < 1.0 {
                    if self.fade_in[note_index] > 1.0 {
                        1.0
                    } else {
                        let prev = self.fade_in[note_index];
                        self.fade_in[note_index] +=
                            Self::compute_increment(self.rate, self.config.fade_in_duration());
                        prev.powf(self.config.in_shape_factor())
                    }
                } else {
                    if self.fade_out[note_index] < 0.0 {
                        0.0
                    } else {
                        let prev = self.fade_out[note_index];
                        self.fade_out[note_index] -=
                            Self::compute_increment(self.rate, self.config.fade_out_duration());
                        prev.powf(self.config.out_shape_factor())
                    }
                };

                if fade > 0.0 {
                    let overtones_freq = self.config.overtone_freq();
                    let overtones_impact = self.config.overtone();
                    for overtone_index in
                        0..std::cmp::min(overtones_freq.len(), overtones_impact.len())
                    {
                        let x = Self::get_frequency(note_index as f64)
                            * overtones_freq[overtone_index]
                            * self.time
                            * 2.0
                            * std::f64::consts::PI;

                        let y = self.config.wave().compute(x);
                        value +=
                            y * self.velocity[note_index] * overtones_impact[overtone_index] * fade;
                    }
                    mute = false;
                }
            }
            value *= self.config.gain();
            *v = value as f32;
            self.time += self.frame_t * self.time_dilation_factor;
            if mute {
                self.time = 0.0;
            }
        }

        // Continue as normal
        jack::Control::Continue
    }

    pub fn set_change_listener(
        &mut self,
        channel_input: std::sync::mpsc::Sender<PlayerChange>,
        config_change_listener: std::sync::mpsc::Sender<ConfigurationChange>,
        external_config_change: std::sync::mpsc::Receiver<ConfigurationChange>,
    ) {
        self.config.set_change_listener(config_change_listener);
        self.change_listener = Some(channel_input);
        self.config_listener = Some(external_config_change);
    }
}

impl jack::ProcessHandler for Player {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        //update according to the input received
        self.read_input(ps);

        self.generate_sound(ps)
    }
}
