use serde::{Deserialize, Serialize};
use std::{collections::HashMap, fmt::Display};

///This enum represent all the dials/button from the midi device
#[derive(Debug, PartialEq, Eq, Hash, PartialOrd, Ord, Clone, Serialize, Deserialize, Copy)]
pub enum KeyBoardKey {
    WaveSelection,
    Overtone(u8),
    FadeInDuration,
    FadeInShape,
    FadeOutDuration,
    FadeOutShape,
    Gain,
    Record,
    Play,
    Stop,
    TransposeUp,
    TransposeDown,
    Parameter,
    Modulation,
    ModulationSpeed,
    ModulationIntensity,
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
            KeyBoardKey::TransposeUp => write!(f, "Transpose up half a step"),
            KeyBoardKey::TransposeDown => write!(f, "Transpose down half a step"),
            KeyBoardKey::Parameter => write!(f, "Effect parameter"),
            KeyBoardKey::Modulation => write!(f, "Modulation"),
            KeyBoardKey::ModulationSpeed => write!(f, "Modulation Speed"),
            KeyBoardKey::ModulationIntensity => write!(f, "Modulation Intensity"),
        }
    }
}

#[derive(Debug, PartialEq, Serialize, Deserialize)]
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
    pub fn new() -> Self {
        Self {
            midi_keys_mapping: HashMap::new(),
            kb_keys_mapping: HashMap::new(),
        }
    }

    pub fn get_keyboard_key(&self, midi_control: u8) -> Option<KeyBoardKey> {
        match self.midi_keys_mapping.get(&midi_control) {
            Some(k) => Some(k.clone()),
            None => None,
        }
    }

    pub fn clear_all(&mut self) {
        self.midi_keys_mapping.clear();
        self.kb_keys_mapping.clear();
    }

    pub fn clear_key(&mut self, key: KeyBoardKey) {
        match self.kb_keys_mapping.get(&key) {
            Some(v) => {
                self.midi_keys_mapping.remove_entry(v);
            }
            None => return,
        }
        self.kb_keys_mapping.remove_entry(&key);
    }

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
