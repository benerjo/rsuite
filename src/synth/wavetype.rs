///The different type of known wave types
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WaveType {
    ///A smooth sinusoidal wave
    Sin,
    ///A wave having either the value 1 or -1
    Square,
    ///A wave that rise linearly
    SawTooth,
    ///A triangle shaped wave
    Triangle,
}
impl WaveType {
    ///Compute the amplitude of the sound after a given time, frequency independent
    pub fn compute(&self, x: f64) -> f64 {
        match self {
            WaveType::Sin => x.sin(),
            WaveType::Square => {
                if x.sin() < 0.0 {
                    1.0
                } else {
                    -1.0
                }
            }
            WaveType::SawTooth => {
                let shift = x + std::f64::consts::PI;
                let pi2 = std::f64::consts::PI * 2.0;
                2.0 * ((shift / pi2) - (shift / pi2).floor()).abs() - 1.0
            }
            WaveType::Triangle => {
                let shift = x - std::f64::consts::PI / 2.0;
                let pi2 = std::f64::consts::PI * 2.0;
                2.0 * (((shift / pi2) - (shift / pi2).floor()) * 2.0 - 1.0).abs() - 1.0
            }
        }
    }

    ///Cycle through the different wave types
    pub fn cycle(&self) -> WaveType {
        match self {
            WaveType::Sin => WaveType::Square,
            WaveType::Square => WaveType::SawTooth,
            WaveType::SawTooth => WaveType::Triangle,
            WaveType::Triangle => WaveType::Sin,
        }
    }
}

impl Default for WaveType {
    fn default() -> Self {
        WaveType::Sin
    }
}

impl std::fmt::Display for WaveType {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WaveType::Sin => write!(f, "Sin"),
            WaveType::Square => write!(f, "Square"),
            WaveType::SawTooth => write!(f, "SawTooth"),
            WaveType::Triangle => write!(f, "Triangle"),
        }
    }
}
