use std::fmt::Display;

use crate::synth::{
    hardware::{HardWare, KeyBoardKey},
    rsynth::configuration::Configuration,
};
use crate::{midiinput::MidiInput, utils::KeyBoardKeySetter};

pub const FADE_DURATION_STEP: f64 = 0.025;
pub const GAIN_STEP: f64 = 8.0 / 127.0;
pub const OVERTONE_STEP: f64 = 1.0 / 128.0;

///This enum represent the different elements that can change for the player
#[derive(Debug)]
pub enum MessageToUI {
    NewConfiguration(Configuration),
    Error(PlayerError),
}

#[derive(Debug)]
pub enum MessageToPlayer {
    NewKeyboardKey(KeyBoardKey),
    ClearKeybaordKey(KeyBoardKey),
    NewConfiguration(Configuration),
    ClearAllKeyboardKeys,
    SaveConf,
    LoadConf,
}

impl From<KeyBoardKeySetter> for MessageToPlayer {
    fn from(value: KeyBoardKeySetter) -> Self {
        match value {
            KeyBoardKeySetter::Set(k) => MessageToPlayer::NewKeyboardKey(k),
            KeyBoardKeySetter::Clear(k) => MessageToPlayer::ClearKeybaordKey(k),
        }
    }
}

#[derive(Debug)]
pub enum PlayerError {
    JackError(jack::Error),
    FileError(std::io::Error),
    CommunicationError(std::sync::mpsc::TryRecvError),
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

impl Display for PlayerError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            PlayerError::JackError(e) => write!(f, "{e}"),
            PlayerError::FileError(e) => write!(f, "{e}"),
            PlayerError::CommunicationError(e) => write!(f, "{e}"),
        }
    }
}

pub struct Player {
    rate: usize,
    /// The duration of a single audio frame
    frame_t: f64,
    /// Time dilation
    time_dilation_factor: f64,
    /// The dilated time that has passed since the beginning
    time: f64,
    /// The real time that has passed since the synth is playing
    real_time: f64,
    /// The input midi port
    midi_in: jack::Port<jack::MidiIn>,
    /// The output audio port
    audio_mono_out: jack::Port<jack::AudioOut>,
    /// Listener to changes in the configuration
    change_listener: std::sync::mpsc::Sender<MessageToUI>,
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
    external_commands: std::sync::mpsc::Receiver<MessageToPlayer>,
    ///If true, the next control input should be used for mapping
    map_next_contrl: Option<KeyBoardKey>,
}

impl Player {
    pub fn new(
        client: &jack::Client,
        extra_input: std::sync::mpsc::Receiver<MessageToPlayer>,
        channel_input: std::sync::mpsc::Sender<MessageToUI>,
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
            real_time: 0.0,
            midi_in: client.register_port("midi_input", jack::MidiIn::default())?,
            audio_mono_out: client.register_port("music_out", jack::AudioOut::default())?,
            change_listener: channel_input,
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
    fn send(change_listener: &mut std::sync::mpsc::Sender<MessageToUI>, to_send: MessageToUI) {
        if let Err(e) = change_listener.send(to_send) {
            eprintln!("Internal error: {e}");
        }
    }

    pub fn get_shape_factor(value: u8) -> f64 {
        if value > 64 {
            1.0 + ((value - 64) as f64) * 3.0 / 4.0
        } else {
            0.1 + (value as f64) / 64.0 * 0.9
        }
    }

    fn save_keyboard_conf(_keyboard: &HardWare) -> Result<(), PlayerError> {
        todo!("implement the saving of the keyboard configuration");
    }

    fn load_keyboard_conf() -> Result<HardWare, PlayerError> {
        todo!("implement the loading of the keyboard configuration");
    }

    fn read_input(&mut self, ps: &jack::ProcessScope) {
        match self.external_commands.try_recv() {
            Ok(v) => match v {
                MessageToPlayer::NewKeyboardKey(k) => self.map_next_contrl = Some(k),
                MessageToPlayer::ClearAllKeyboardKeys => self.keyboard.clear_all(),
                MessageToPlayer::SaveConf => match Self::save_keyboard_conf(&mut self.keyboard) {
                    Ok(()) => {}
                    Err(e) => {
                        Self::send(&mut self.change_listener, MessageToUI::Error(e));
                    }
                },
                MessageToPlayer::LoadConf => match Self::load_keyboard_conf() {
                    Ok(kb) => self.keyboard = kb,
                    Err(e) => Self::send(&mut self.change_listener, MessageToUI::Error(e)),
                },
                MessageToPlayer::ClearKeybaordKey(k) => self.keyboard.clear_key(k),
                MessageToPlayer::NewConfiguration(conf) => self.config = conf,
            },
            Err(e) => match e {
                std::sync::mpsc::TryRecvError::Empty => {}
                std::sync::mpsc::TryRecvError::Disconnected => Self::send(
                    &mut self.change_listener,
                    MessageToUI::Error(PlayerError::CommunicationError(e)),
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
                    let current_conf = self.config.clone();
                    match self.keyboard.get_keyboard_key(control) {
                        None => {}
                        Some(v) => match v {
                            KeyBoardKey::WaveSelection => {
                                if value > 0 {
                                    self.config.wave = self.config.wave.cycle();
                                }
                            }
                            KeyBoardKey::Overtone(overtone_index) => {
                                let new_value = (value as f64) * OVERTONE_STEP;
                                self.config.overtone[overtone_index as usize] = new_value;
                            }
                            KeyBoardKey::FadeInDuration => {
                                self.config.fade_in_duration =
                                    FADE_DURATION_STEP * (1.0 + value as f64)
                            }
                            KeyBoardKey::FadeInShape => {
                                self.config.fade_in_shape = value;
                            }
                            KeyBoardKey::FadeOutDuration => {
                                let new_duration = FADE_DURATION_STEP * (1.0 + value as f64);
                                self.config.fade_out_duration = new_duration;
                            }
                            KeyBoardKey::FadeOutShape => {
                                self.config.fade_out_shape = value;
                            }
                            KeyBoardKey::Gain => {
                                let new_gain = (1 + value) as f64 * GAIN_STEP;
                                self.config.gain = new_gain;
                            }
                            KeyBoardKey::Modulation => {
                                self.config.modulation = value;
                            }
                            KeyBoardKey::ModulationSpeed => {
                                self.config.mod_speed = (value as f64) / 4.0;
                            }
                            KeyBoardKey::ModulationIntensity => {
                                self.config.mod_intensity = (value as f64) / 128.0;
                            }
                            _ => {}
                        },
                    }
                    if self.config != current_conf {
                        Player::send(
                            &mut self.change_listener,
                            MessageToUI::NewConfiguration(self.config.clone()),
                        )
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
                            Self::compute_increment(self.rate, self.config.fade_in_duration);
                        let factor = Player::get_shape_factor(self.config.fade_in_shape);
                        prev.powf(factor)
                    }
                } else {
                    if self.fade_out[note_index] < 0.0 {
                        0.0
                    } else {
                        let prev = self.fade_out[note_index];
                        self.fade_out[note_index] -=
                            Self::compute_increment(self.rate, self.config.fade_out_duration);
                        let factor = Player::get_shape_factor(self.config.fade_out_shape);
                        prev.powf(factor)
                    }
                };

                if fade > 0.0 {
                    let overtones_freq = &self.config.overtone_freq;
                    let overtones_impact = &self.config.overtone;
                    for overtone_index in
                        0..std::cmp::min(overtones_freq.len(), overtones_impact.len())
                    {
                        let x = Self::get_frequency(note_index as f64)
                            * overtones_freq[overtone_index]
                            * self.time
                            * 2.0
                            * std::f64::consts::PI;

                        let y = self.config.wave.compute(x);
                        value +=
                            y * self.velocity[note_index] * overtones_impact[overtone_index] * fade;
                    }
                    mute = false;
                }
            }
            value *= self.config.gain;
            *v = value as f32;

            let modulation_aux =
                (self.config.modulation as f64) * self.real_time * std::f64::consts::PI
                    / self.config.mod_speed;
            let modulation = self.config.mod_intensity * modulation_aux.sin() + 1.0;
            self.time += self.frame_t * self.time_dilation_factor * modulation;
            self.real_time += self.frame_t;
            if mute {
                self.time = 0.0;
                self.real_time = 0.0;
            }
        }

        // Continue as normal
        jack::Control::Continue
    }
}

impl jack::ProcessHandler for Player {
    fn process(&mut self, _: &jack::Client, ps: &jack::ProcessScope) -> jack::Control {
        //update according to the input received
        self.read_input(ps);

        self.generate_sound(ps)
    }
}
