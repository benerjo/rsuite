use crate::synth::wavetype::WaveType;

///This enum represent the different elements that can change for the player
#[derive(Debug, Clone)]
pub enum ConfigurationChange {
    Wave(WaveType),
    Overtone { index: usize, value: f64 },
    FadeInDuration(f64),
    ShapeFactorFadeIn(f64),
    ShapeFactorFadeOut(f64),
    FadeOutDuration(f64),
    Gain(f64),
}

///A configuration is user-input defined: it specify
/// the wave type, the amount of overtone, the speed of the attack
/// and the speed of the release
pub struct Configuration {
    /// The wave type used by this configuration
    wave: WaveType,
    /// The overtone vector
    overtone: Vec<f64>,
    /// The frequency mutliplier for to obtain the overtone
    overtone_freq: Vec<f64>,
    /// The speed of the attack
    fade_in_duration: f64,
    in_shape_factor: f64,
    /// The speed of the release
    fade_out_duration: f64,
    out_shape_factor: f64,
    /// The gain we appy on the amplitude
    gain: f64,
    change_listener: Option<std::sync::mpsc::Sender<ConfigurationChange>>,
}

impl Configuration {
    ///Retrieve the default overtone frequency array
    pub fn default_overtone_frequencies() -> Vec<f64> {
        vec![1.0, 1.0 / 2.0, 1.0 / 3.0, 2.0, 3.0, 4.0, 5.0, 6.0, 8.0]
    }

    ///Retrieve the default overtone impact array
    pub fn default_overtone_impact() -> Vec<f64> {
        vec![1.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0]
    }

    pub fn new() -> Configuration {
        Self {
            overtone: Self::default_overtone_impact(),
            overtone_freq: Self::default_overtone_frequencies(),
            wave: WaveType::default(),
            fade_in_duration: 0.1,
            in_shape_factor: 1.0,
            fade_out_duration: 0.1,
            out_shape_factor: 1.0,
            gain: 1.0,
            change_listener: None,
        }
    }

    /// Apply the changes given in the configuration change.
    pub fn apply_and_send_notifications(&mut self, change: ConfigurationChange) {
        match change {
            ConfigurationChange::Wave(w) => {
                self.wave = w;
                self.send_notification(ConfigurationChange::Wave(self.wave))
            }
            ConfigurationChange::Overtone { index, value } => self.update_overtone(index, value),
            ConfigurationChange::FadeInDuration(d) => self.set_fade_in_duration(d),
            ConfigurationChange::ShapeFactorFadeIn(s) => self.set_fade_in_shape(s),
            ConfigurationChange::ShapeFactorFadeOut(s) => self.set_fade_out_shape(s),
            ConfigurationChange::FadeOutDuration(d) => self.set_fade_out_duration(d),
            ConfigurationChange::Gain(g) => self.set_gain(g),
        }
    }

    pub fn apply_dont_send_notification(&mut self, change: ConfigurationChange) {
        match change {
            ConfigurationChange::Wave(w) => self.wave = w,
            ConfigurationChange::Overtone { index, value } => self.overtone[index] = value,
            ConfigurationChange::FadeInDuration(d) => self.fade_in_duration = d,
            ConfigurationChange::ShapeFactorFadeIn(s) => self.in_shape_factor = s,
            ConfigurationChange::ShapeFactorFadeOut(s) => self.out_shape_factor = s,
            ConfigurationChange::FadeOutDuration(d) => self.fade_out_duration = d,
            ConfigurationChange::Gain(g) => self.gain = g,
        }
    }

    pub fn set_change_listener(
        &mut self,
        change_listener: std::sync::mpsc::Sender<ConfigurationChange>,
    ) {
        self.change_listener = Some(change_listener)
    }

    pub fn cycle_wave_type(&mut self) {
        self.wave = self.wave.cycle();
        self.send_notification(ConfigurationChange::Wave(self.wave))
    }

    ///update the impact of an overtone, given its index and send the related notification
    pub fn update_overtone(&mut self, index: usize, new_value: f64) {
        if self.overtone[index] == new_value {
            return;
        }
        //change the value of the overtones
        self.overtone[index] = new_value;

        //notify the listener of the change
        self.send_notification(ConfigurationChange::Overtone {
            index: index,
            value: new_value,
        });
    }

    ///Specify the new duration of the fade in
    pub fn set_fade_in_duration(&mut self, new_duration: f64) {
        if self.fade_in_duration == new_duration {
            return;
        }
        self.fade_in_duration = new_duration;
        self.send_notification(ConfigurationChange::FadeInDuration(new_duration));
    }

    ///Specify the new shape factor for the fade in
    pub fn set_fade_in_shape(&mut self, shape: f64) {
        if self.in_shape_factor == shape {
            return;
        }
        self.in_shape_factor = shape;
        self.send_notification(ConfigurationChange::ShapeFactorFadeIn(self.in_shape_factor));
    }

    ///Specify the new duration of the fade out
    pub fn set_fade_out_duration(&mut self, new_duration: f64) {
        if self.fade_out_duration == new_duration {
            return;
        }
        self.fade_out_duration = new_duration;
        self.send_notification(ConfigurationChange::FadeOutDuration(new_duration));
    }

    ///Specify the new shape factor for the fade out
    pub fn set_fade_out_shape(&mut self, shape: f64) {
        if self.out_shape_factor == shape {
            return;
        }
        self.out_shape_factor = shape;
        self.send_notification(ConfigurationChange::ShapeFactorFadeOut(
            self.out_shape_factor,
        ));
    }

    pub fn set_gain(&mut self, gain: f64) {
        if self.gain == gain {
            return;
        }
        self.gain = gain;
        self.send_notification(ConfigurationChange::Gain(self.gain));
    }

    fn send_notification(&mut self, notification: ConfigurationChange) {
        match &mut self.change_listener {
            Some(l) => match l.send(notification) {
                Ok(()) => (),
                Err(e) => eprint!("Unable to send change notification: {e}"),
            },
            None => {}
        }
    }

    pub fn gain(&self) -> f64 {
        self.gain
    }

    pub fn out_shape_factor(&self) -> f64 {
        self.out_shape_factor
    }

    pub fn in_shape_factor(&self) -> f64 {
        self.in_shape_factor
    }

    pub fn fade_out_duration(&self) -> f64 {
        self.fade_out_duration
    }

    pub fn fade_in_duration(&self) -> f64 {
        self.fade_in_duration
    }

    pub fn wave(&self) -> WaveType {
        self.wave
    }

    pub fn overtone_freq(&self) -> &[f64] {
        self.overtone_freq.as_ref()
    }

    pub fn overtone(&self) -> &[f64] {
        self.overtone.as_ref()
    }
}
