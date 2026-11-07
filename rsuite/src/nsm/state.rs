//! The state of a program, as it is saved by the session manager

use std::{collections::BTreeMap, fmt::Display, str::FromStr};

use crate::{
    configuration::Configuration,
    synth::hardware::{HardWare, KeyBoardKey},
};

///The prefix of the values storing the midi controller of a keyboard key
const KEY_PREFIX: &str = "key.";

///A list of named values, stored in a text file as one 'name=value' per line
#[derive(Debug, Clone, Default, PartialEq)]
pub struct Values {
    values: BTreeMap<String, String>,
}

impl Values {
    pub fn new() -> Values {
        Values::default()
    }

    ///Store a value. The name must neither contain '=' nor line breaks
    pub fn set<T: Display>(&mut self, name: &str, value: T) {
        self.values.insert(String::from(name), value.to_string());
    }

    ///Retrieve a value. None is returned if the value is missing or cannot be read
    pub fn get<T: FromStr>(&self, name: &str) -> Option<T> {
        self.values.get(name).and_then(|v| v.parse().ok())
    }

    ///Change the given value only if it is stored
    pub fn load<T: FromStr>(&self, name: &str, value: &mut T) {
        if let Some(v) = self.get(name) {
            *value = v;
        }
    }

    ///Retrieve all the values whose name start with the given prefix.
    /// The prefix is removed from the returned names
    pub fn with_prefix<'v>(&'v self, prefix: &'v str) -> impl Iterator<Item = (&'v str, &'v str)> {
        self.values
            .iter()
            .filter_map(move |(k, v)| k.strip_prefix(prefix).map(|k| (k, v.as_str())))
    }

    ///Write the values in the format of the state file
    pub fn to_text(&self) -> String {
        let mut out = String::new();
        for (name, value) in &self.values {
            out.push_str(name);
            out.push('=');
            for c in value.chars() {
                match c {
                    '\\' => out.push_str("\\\\"),
                    '\n' => out.push_str("\\n"),
                    '\r' => out.push_str("\\r"),
                    c => out.push(c),
                }
            }
            out.push('\n');
        }
        out
    }

    ///Read the values written by 'to_text'. Invalid lines are ignored
    pub fn from_text(text: &str) -> Values {
        let mut values = Values::new();
        for line in text.lines() {
            let Some((name, raw)) = line.split_once('=') else {
                continue;
            };
            let mut value = String::with_capacity(raw.len());
            let mut chars = raw.chars();
            while let Some(c) = chars.next() {
                if c == '\\' {
                    match chars.next() {
                        Some('n') => value.push('\n'),
                        Some('r') => value.push('\r'),
                        Some(c) => value.push(c),
                        None => {}
                    }
                } else {
                    value.push(c);
                }
            }
            values.values.insert(String::from(name), value);
        }
        values
    }
}

///Something that can be saved in, and restored from, a list of values
pub trait Persistent {
    ///Store the state in the values
    fn save(&self, values: &mut Values);
    ///Restore the state from the values. Missing values are left unchanged
    fn load(&mut self, values: &Values);
}

///Nothing to save
impl Persistent for () {
    fn save(&self, _values: &mut Values) {}
    fn load(&mut self, _values: &Values) {}
}

impl Persistent for HardWare {
    fn save(&self, values: &mut Values) {
        for (key, control) in self.mappings() {
            values.set(&format!("{KEY_PREFIX}{}", key.id()), control);
        }
    }

    ///The mapping is replaced by the saved one
    fn load(&mut self, values: &Values) {
        self.clear_all();
        for (id, control) in values.with_prefix(KEY_PREFIX) {
            if let (Some(key), Ok(control)) = (KeyBoardKey::from_id(id), control.parse()) {
                self.update_key(key, control);
            }
        }
    }
}

///A processing unit whose state is kept by the session manager.
pub trait Stateful {
    type State: Persistent + Send + 'static;

    ///Copy the current state of the unit into 'state'. This is called after each
    /// cycle of the real time thread, so only what changed should be copied (see 'sync')
    fn publish(&self, state: &mut Self::State);
}

///Copy 'value' into 'target' only if they differ, to avoid allocations
pub fn sync<T: Clone + PartialEq>(target: &mut T, value: &T) {
    if target != value {
        *target = value.clone();
    }
}

///The state of a program defined by a configuration and the controllers mapped to it
#[derive(Debug, Clone, PartialEq)]
pub struct ConfigurationState<C> {
    pub conf: C,
    pub keyboard: HardWare,
}

impl<C: Clone + PartialEq> ConfigurationState<C> {
    pub fn new(conf: C) -> Self {
        Self {
            conf,
            keyboard: HardWare::new(),
        }
    }

    ///Copy the configuration and the controllers if they changed
    pub fn update(&mut self, conf: &C, keyboard: &HardWare) {
        sync(&mut self.conf, conf);
        sync(&mut self.keyboard, keyboard);
    }
}

impl<C> Persistent for ConfigurationState<C>
where
    C: for<'c> Configuration<'c>,
{
    fn save(&self, values: &mut Values) {
        //the elements of a configuration can only be retrieved from a mutable reference
        self.conf.clone().save_values(values);
        self.keyboard.save(values);
    }

    fn load(&mut self, values: &Values) {
        self.conf.load_values(values);
        self.keyboard.load(values);
    }
}

#[cfg(test)]
mod test {
    use super::{Persistent, Values};
    use crate::synth::hardware::{HardWare, KeyBoardKey};

    #[test]
    fn text_round_trip() {
        let mut values = Values::new();
        values.set("a", 1.5);
        values.set("b", "line\nother=\\line\r");
        values.set("value.start freq", 350);
        let text = values.to_text();
        assert_eq!(text.lines().count(), 3);
        assert_eq!(Values::from_text(&text), values);
        assert_eq!(values.get::<f64>("a"), Some(1.5));
        assert_eq!(values.get::<f64>("b"), None);
        assert_eq!(values.get::<usize>("missing"), None);
    }

    #[test]
    fn invalid_lines_are_ignored() {
        let values = Values::from_text("no separator\na=1\n\n");
        assert_eq!(values.get::<u8>("a"), Some(1));
        assert_eq!(values.with_prefix("").count(), 1);
    }

    #[test]
    fn keyboard_round_trip() {
        let mut keyboard = HardWare::new();
        keyboard.update_key(KeyBoardKey::Overtone(3), 12);
        keyboard.update_key(KeyBoardKey::TransposeDown, 13);
        let mut values = Values::new();
        keyboard.save(&mut values);

        let mut loaded = HardWare::default();
        loaded.load(&values);
        assert_eq!(loaded, keyboard);
    }
}
