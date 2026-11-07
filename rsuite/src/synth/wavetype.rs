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

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
pub enum WaveTypeConversionError {
    UnknownType(String),
}

impl std::fmt::Display for WaveTypeConversionError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            WaveTypeConversionError::UnknownType(w) => write!(f, "Unknown wave type: {w}"),
        }
    }
}

impl std::error::Error for WaveTypeConversionError {}

impl std::str::FromStr for WaveType {
    type Err = WaveTypeConversionError;

    ///Read the name written by 'Display'
    fn from_str(s: &str) -> Result<Self, Self::Err> {
        [
            WaveType::Sin,
            WaveType::Square,
            WaveType::SawTooth,
            WaveType::Triangle,
        ]
        .into_iter()
        .find(|w| w.to_string() == s)
        .ok_or_else(|| WaveTypeConversionError::UnknownType(s.to_string()))
    }
}

#[cfg(test)]
mod test {
    use super::WaveType;

    #[test]
    fn names() {
        let mut wave = WaveType::Sin;
        for _ in 0..4 {
            assert_eq!(wave.to_string().parse::<WaveType>(), Ok(wave));
            wave = wave.cycle();
        }
        assert!("sin".parse::<WaveType>().is_err());
    }
}
