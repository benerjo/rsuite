use std::{collections::HashMap, fmt::Display};

///This enum represent all the dials/button from the midi device
#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Clone, Copy)]
pub enum KeyBoardKey {
    WaveSelection,
    Overtone(u8),
    FadeInDuration,
    FadeInShape,
    FadeOutDuration,
    FadeOutShape,
    Duration,
    Gain,
    Record,
    Play,
    Stop,
    Break,
    TransposeUp,
    TransposeDown,
    Parameter,
    ParameterB,
    Modulation,
    ModulationSpeed,
    ModulationIntensity,
    Activate,
    Tempo,
}

impl KeyBoardKey {
    ///The keys without parameter. Any new key has to be added here to be restored
    /// by 'from_id'
    const SIMPLE_KEYS: [KeyBoardKey; 20] = [
        KeyBoardKey::WaveSelection,
        KeyBoardKey::FadeInDuration,
        KeyBoardKey::FadeInShape,
        KeyBoardKey::FadeOutDuration,
        KeyBoardKey::FadeOutShape,
        KeyBoardKey::Duration,
        KeyBoardKey::Gain,
        KeyBoardKey::Record,
        KeyBoardKey::Play,
        KeyBoardKey::Stop,
        KeyBoardKey::Break,
        KeyBoardKey::TransposeUp,
        KeyBoardKey::TransposeDown,
        KeyBoardKey::Parameter,
        KeyBoardKey::ParameterB,
        KeyBoardKey::Modulation,
        KeyBoardKey::ModulationSpeed,
        KeyBoardKey::ModulationIntensity,
        KeyBoardKey::Activate,
        KeyBoardKey::Tempo,
    ];

    ///A name identifying the key, that does not change with the displayed text
    pub fn id(&self) -> String {
        format!("{self:?}")
    }

    ///Retrieve the key identified by the result of 'id'
    pub fn from_id(id: &str) -> Option<KeyBoardKey> {
        if let Some(index) = id
            .strip_prefix("Overtone(")
            .and_then(|i| i.strip_suffix(')'))
        {
            return index.parse().ok().map(KeyBoardKey::Overtone);
        }
        Self::SIMPLE_KEYS.into_iter().find(|k| k.id() == id)
    }
}

impl Display for KeyBoardKey {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            KeyBoardKey::WaveSelection => write!(f, "Wave Selection"),
            KeyBoardKey::Overtone(v) => write!(f, "Overtone {}", v + 1),
            KeyBoardKey::FadeInDuration => write!(f, "Fade In Duration"),
            KeyBoardKey::FadeInShape => write!(f, "Fade In Shape"),
            KeyBoardKey::FadeOutDuration => write!(f, "Fade Out Duration"),
            KeyBoardKey::FadeOutShape => write!(f, "Fade Out Shape"),
            KeyBoardKey::Gain => write!(f, "Gain"),
            KeyBoardKey::Record => write!(f, "Record"),
            KeyBoardKey::Play => write!(f, "Play"),
            KeyBoardKey::Stop => write!(f, "Stop"),
            KeyBoardKey::Break => write!(f, "Break"),
            KeyBoardKey::TransposeUp => write!(f, "Transpose up half a step"),
            KeyBoardKey::TransposeDown => write!(f, "Transpose down half a step"),
            KeyBoardKey::Parameter => write!(f, "Effect parameter A"),
            KeyBoardKey::ParameterB => write!(f, "Effect parameter B"),
            KeyBoardKey::Modulation => write!(f, "Modulation"),
            KeyBoardKey::ModulationSpeed => write!(f, "Modulation Speed"),
            KeyBoardKey::ModulationIntensity => write!(f, "Modulation Intensity"),
            KeyBoardKey::Activate => write!(f, "Activate"),
            KeyBoardKey::Tempo => write!(f, "Tempo"),
            KeyBoardKey::Duration => write!(f, "Duration"),
        }
    }
}

///The hardware structure is used to abstract
/// the midi control message and map them to a KeyBoardKey value
#[derive(Debug, Clone, PartialEq)]
pub struct HardWare {
    midi_keys_mapping: HashMap<u8, KeyBoardKey>,
    kb_keys_mapping: HashMap<KeyBoardKey, u8>,
}

impl Default for HardWare {
    fn default() -> Self {
        let map = HashMap::from([
            (113, KeyBoardKey::WaveSelection),
            (74, KeyBoardKey::Overtone(0)),
            (71, KeyBoardKey::Overtone(1)),
            (91, KeyBoardKey::Overtone(2)),
            (93, KeyBoardKey::Overtone(3)),
            (73, KeyBoardKey::Overtone(4)),
            (72, KeyBoardKey::Overtone(5)),
            (5, KeyBoardKey::Overtone(6)),
            (84, KeyBoardKey::Overtone(7)),
            (7, KeyBoardKey::Overtone(8)),
            (10, KeyBoardKey::FadeInDuration),
            (2, KeyBoardKey::FadeInShape),
            (75, KeyBoardKey::FadeOutDuration),
            (76, KeyBoardKey::FadeOutShape),
            (95, KeyBoardKey::Gain),
            (118, KeyBoardKey::Record),
            (117, KeyBoardKey::Play),
            (116, KeyBoardKey::Stop),
        ]);
        let mut revert_map = HashMap::<KeyBoardKey, u8>::with_capacity(map.capacity());
        for (k, v) in &map {
            revert_map.insert(v.clone(), *k);
        }
        Self {
            midi_keys_mapping: map,
            kb_keys_mapping: revert_map,
        }
    }
}

impl HardWare {
    ///Create a new representation of Hardware
    pub fn new() -> Self {
        Self {
            midi_keys_mapping: HashMap::new(),
            kb_keys_mapping: HashMap::new(),
        }
    }

    ///Retreive the KeyBoardKey represented by the given midi control
    pub fn get_keyboard_key(&self, midi_control: u8) -> Option<KeyBoardKey> {
        match self.midi_keys_mapping.get(&midi_control) {
            Some(k) => Some(k.clone()),
            None => None,
        }
    }

    ///Retrieve all the keys that are mapped, with their midi control
    pub fn mappings(&self) -> impl Iterator<Item = (KeyBoardKey, u8)> + '_ {
        self.kb_keys_mapping.iter().map(|(k, m)| (*k, *m))
    }

    ///Clear all mapping
    pub fn clear_all(&mut self) {
        self.midi_keys_mapping.clear();
        self.kb_keys_mapping.clear();
    }

    ///Clear the mapping between a key and it's midi control representation
    pub fn clear_key(&mut self, key: KeyBoardKey) {
        match self.kb_keys_mapping.get(&key) {
            Some(v) => {
                self.midi_keys_mapping.remove_entry(v);
            }
            None => return,
        }
        self.kb_keys_mapping.remove_entry(&key);
    }

    ///Specify the midi control key that should be mapped to a given keyboard key
    pub fn update_key(&mut self, key: KeyBoardKey, midi_key: u8) {
        match self.kb_keys_mapping.get(&key) {
            Some(midi_key) => {
                self.midi_keys_mapping.remove(midi_key);
            }
            None => {}
        }
        self.midi_keys_mapping.insert(midi_key, key.clone());
        self.kb_keys_mapping.insert(key, midi_key);
    }
}

#[cfg(test)]
mod test {
    use super::KeyBoardKey;

    #[test]
    fn key_ids() {
        let keys = KeyBoardKey::SIMPLE_KEYS
            .into_iter()
            .chain((0..9).map(KeyBoardKey::Overtone));
        for key in keys {
            assert_eq!(KeyBoardKey::from_id(&key.id()), Some(key));
        }
        assert_eq!(KeyBoardKey::from_id("Unknown"), None);
        assert_eq!(KeyBoardKey::from_id("Overtone(x)"), None);
    }
}
