use crate::synth::wavetype::WaveType;

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
        }
    }
}
