use crate::{
    synth::{hardware::KeyBoardKey, wavetype::WaveType},
    utils::{create_keyboard_select, KeyBoardKeySetter},
};

/// Represent a float value that must be whithin a range
#[derive(Debug, Clone, PartialEq)]
pub struct FloatValueInRange {
    ///The current value
    value: f64,
    ///The allowed range
    range: std::ops::RangeInclusive<f64>,
    ///The name of the value
    name: String,
    ///The keyboard key that we want to map to the value
    key: KeyBoardKey,
}

impl FloatValueInRange {
    ///Create a new float value whithin a range
    pub fn new(
        value: f64,
        range: std::ops::RangeInclusive<f64>,
        name: &str,
        key: KeyBoardKey,
    ) -> FloatValueInRange {
        FloatValueInRange {
            value,
            range,
            name: String::from(name),
            key,
        }
    }

    ///Change the value based on the value retrieved by the midi key
    pub fn from_midi_value(&mut self, value: u8) -> bool {
        let nv = value as f64 * (self.range.end() - self.range.start()) / 128.0;
        if nv != self.value {
            self.value = value as f64 * (self.range.end() - self.range.start()) / 128.0;
            return true;
        }
        false
    }

    ///Retrieve the value
    pub fn get_value(&self) -> f64 {
        self.value
    }

    ///Retrieve the keyboard key that is expected to this value
    fn get_keyboard_key(&self) -> KeyBoardKey {
        self.key
    }

    ///Draw the value
    fn draw(&mut self, ui: &mut eframe::egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(&self.name);
            ui.add_enabled(
                true,
                eframe::egui::Slider::new(&mut self.value, self.range.clone()).show_value(false),
            );
            let speed = self.range.end() - self.range.start();
            ui.add_enabled(
                true,
                eframe::egui::DragValue::new(&mut self.value)
                    .range(self.range.clone())
                    .speed(speed / 128.0),
            );
        });
    }
}

/// Represent a value that must be whithin a range
#[derive(Debug, Clone, PartialEq)]
pub struct UsizeValueInRange {
    ///The current value
    value: usize,
    ///The allowed range
    range: std::ops::RangeInclusive<usize>,
    ///The name of the value
    name: String,
    ///The keyboard key that we want to map to the value
    key: KeyBoardKey,
}

impl UsizeValueInRange {
    pub fn new(
        value: usize,
        range: std::ops::RangeInclusive<usize>,
        name: &str,
        key: KeyBoardKey,
    ) -> UsizeValueInRange {
        UsizeValueInRange {
            value,
            range,
            name: String::from(name),
            key,
        }
    }

    ///Change the value based on the value retrieved by the midi key
    pub fn from_midi_value(&mut self, value: u8) -> bool {
        let nv = value as usize * (self.range.end() - self.range.start()) / 128;
        if nv != self.value {
            self.value = nv;
            return true;
        }
        false
    }

    pub fn get_value(&self) -> usize {
        self.value
    }

    fn get_keyboard_key(&self) -> KeyBoardKey {
        self.key
    }

    fn draw(&mut self, ui: &mut eframe::egui::Ui) {
        ui.horizontal(|ui| {
            ui.label(&self.name);
            ui.add_enabled(
                true,
                eframe::egui::Slider::new(&mut self.value, self.range.clone()).show_value(false),
            );
            let speed = self.range.end() - self.range.start();
            ui.add_enabled(
                true,
                eframe::egui::DragValue::new(&mut self.value)
                    .range(self.range.clone())
                    .speed(speed as f64 / 128.0),
            );
        });
    }
}

/// Represent a value that must be whithin a range
#[derive(Debug, Clone, PartialEq)]
pub struct WaveTypeValue {
    ///The current value
    value: WaveType,
    ///The name of the value
    name: String,
    ///The keyboard key that we want to map to the value
    key: KeyBoardKey,
}

impl WaveTypeValue {
    pub fn new(name: &str, key: KeyBoardKey) -> WaveTypeValue {
        WaveTypeValue {
            value: WaveType::Sin,
            name: String::from(name),
            key,
        }
    }

    ///Change the value based on the value retrieved by the midi key
    pub fn from_midi_value(&mut self, value: u8) -> bool {
        if value > 0 {
            self.value = self.value.cycle();
            return true;
        }
        false
    }

    pub fn get_value(&self) -> WaveType {
        self.value
    }

    fn get_keyboard_key(&self) -> KeyBoardKey {
        self.key
    }

    fn draw(&mut self, ui: &mut eframe::egui::Ui) {
        ui.horizontal(|ui| {
            ui.label("Wave type:");
            if ui.button(format!("{}", self.value)).clicked() {
                self.value = self.value.cycle();
            }
        });
        let line = crate::utils::create_plot_line(&self.value);
        egui_plot::Plot::new(format!("Wave type: {}", self.value))
            .view_aspect(21.0 / 9.0)
            .show(ui, |plot_ui| plot_ui.line(line));
    }
}

///A value stored in a configuration
pub enum ConfigurationValue<'conf> {
    Float(&'conf mut FloatValueInRange),
    USize(&'conf mut UsizeValueInRange),
    WaveType(&'conf mut WaveTypeValue),
}

impl<'conf> ConfigurationValue<'conf> {
    ///Draw the configurable value
    fn draw(&mut self, ui: &mut eframe::egui::Ui) {
        match self {
            ConfigurationValue::Float(value) => value.draw(ui),
            ConfigurationValue::USize(value) => value.draw(ui),
            ConfigurationValue::WaveType(value) => value.draw(ui),
        }
    }

    ///Retrive the keyboard key associated with the configurable value
    fn key(&self) -> KeyBoardKey {
        match self {
            ConfigurationValue::Float(value) => value.get_keyboard_key(),
            ConfigurationValue::USize(value) => value.get_keyboard_key(),
            ConfigurationValue::WaveType(value) => value.get_keyboard_key(),
        }
    }

    ///retrieve the name of the configurable value
    fn name(&self) -> &str {
        match self {
            ConfigurationValue::Float(value) => &value.name,
            ConfigurationValue::USize(value) => &value.name,
            ConfigurationValue::WaveType(value) => &value.name,
        }
    }

    ///Change the value based on a midi controller value
    fn apply_midi_value(&mut self, midi_value: u8) -> bool {
        match self {
            ConfigurationValue::Float(value) => value.from_midi_value(midi_value),
            ConfigurationValue::USize(value) => value.from_midi_value(midi_value),
            ConfigurationValue::WaveType(value) => value.from_midi_value(midi_value),
        }
    }
}

///Trait for configurations of programs
/// Configurations should contain every single parameter of a program
pub trait Configuration<'c>
where
    Self: std::fmt::Debug + Clone + PartialEq,
{
    ///Retrieve the list of values that can be configured
    fn elements(&'c mut self) -> Vec<ConfigurationValue<'c>>;

    ///Draw the configuration on the user interface
    fn draw(&'c mut self, ui: &mut eframe::egui::Ui) {
        for mut e in self.elements() {
            e.draw(ui);
        }
    }

    ///Apply a midi control key to the configuration. If the configuration
    /// changes, true will be returned. False otherwise
    fn apply_midi(&'c mut self, key: KeyBoardKey, value: u8) -> bool {
        for mut e in self.elements() {
            if e.key() == key {
                return e.apply_midi_value(value);
            }
        }
        return false;
    }

    ///Create the menu entries to change the controller key
    /// mapped to a configuration element
    fn create_menu_keyboard_settings<T>(
        &'c mut self,
        ui: &mut eframe::egui::Ui,
        sender: &mut std::sync::mpsc::Sender<T>,
        messages: &mut Vec<String>,
    ) where
        T: From<KeyBoardKeySetter>,
    {
        for gen_val in self.elements() {
            create_keyboard_select(ui, gen_val.name(), gen_val.key(), sender, messages);
        }
    }
}
