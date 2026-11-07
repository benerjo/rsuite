use rsuite::{
    nsm::{Persistent, Values},
    synth::{hardware::HardWare, wavetype::WaveType},
};

///A configuration is user-input defined: it specify
/// the wave type, the amount of overtone, the speed of the attack
/// and the speed of the release
#[derive(Debug, PartialEq, Clone)]
pub struct Configuration {
    /// The wave type used by this configuration
    pub wave: WaveType,
    /// The overtone vector
    pub overtone: Vec<f64>,
    /// The frequency mutliplier for to obtain the overtone
    pub overtone_freq: Vec<f64>,
    /// The speed of the attack
    pub fade_in_duration: f64,
    pub fade_in_shape: u8,
    /// The speed of the release
    pub fade_out_duration: f64,
    pub fade_out_shape: u8,
    /// The gain we appy on the amplitude
    pub gain: f64,
    /// The modulation to apply
    pub modulation: u8,
    /// The intensity of the modulation
    pub mod_intensity: f64,
    /// The speed of the modulation
    pub mod_speed: f64,
}

impl Configuration {
    pub fn new() -> Configuration {
        Self {
            overtone: vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0],
            overtone_freq: vec![1.0, 1.0 / 2.0, 1.0 / 3.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0],
            wave: WaveType::default(),
            fade_in_duration: 0.1,
            fade_in_shape: 64,
            fade_out_duration: 0.1,
            fade_out_shape: 64,
            gain: 1.0,
            modulation: 0,
            mod_intensity: 0.1,
            mod_speed: 8.0,
        }
    }
}

impl Persistent for Configuration {
    fn save(&self, values: &mut Values) {
        values.set("wave", self.wave);
        for (index, overtone) in self.overtone.iter().enumerate() {
            values.set(&format!("overtone.{index}"), overtone);
        }
        values.set("fade_in_duration", self.fade_in_duration);
        values.set("fade_in_shape", self.fade_in_shape);
        values.set("fade_out_duration", self.fade_out_duration);
        values.set("fade_out_shape", self.fade_out_shape);
        values.set("gain", self.gain);
        values.set("modulation", self.modulation);
        values.set("mod_intensity", self.mod_intensity);
        values.set("mod_speed", self.mod_speed);
    }

    fn load(&mut self, values: &Values) {
        values.load("wave", &mut self.wave);
        for (index, overtone) in self.overtone.iter_mut().enumerate() {
            values.load(&format!("overtone.{index}"), overtone);
        }
        values.load("fade_in_duration", &mut self.fade_in_duration);
        values.load("fade_in_shape", &mut self.fade_in_shape);
        values.load("fade_out_duration", &mut self.fade_out_duration);
        values.load("fade_out_shape", &mut self.fade_out_shape);
        values.load("gain", &mut self.gain);
        values.load("modulation", &mut self.modulation);
        values.load("mod_intensity", &mut self.mod_intensity);
        values.load("mod_speed", &mut self.mod_speed);
    }
}

///The state of the synthesizer saved by the session manager
#[derive(Debug, Clone, PartialEq)]
pub struct SynthState {
    pub config: Configuration,
    pub keyboard: HardWare,
}

impl Persistent for SynthState {
    fn save(&self, values: &mut Values) {
        self.config.save(values);
        self.keyboard.save(values);
    }

    fn load(&mut self, values: &Values) {
        self.config.load(values);
        self.keyboard.load(values);
    }
}
