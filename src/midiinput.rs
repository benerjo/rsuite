use std::convert::From;

const MAX_MIDI: usize = 3;

const CLOCK: u8 = 0xF8;
const START: u8 = 0xFA;
const CONTINUE: u8 = 0xFB;
const STOP: u8 = 0xFC;

#[derive(Clone, PartialEq)]
pub enum MidiInput {
    NoteStart {
        channel: u8,
        note_index: usize,
        timing: jack::Frames,
        velocity: f64,
    },
    NoteEnd {
        channel: u8,
        note_index: usize,
        timing: jack::Frames,
        velocity: f64,
    },
    Controller {
        channel: u8,
        control: u8,
        value: u8,
    },
    PitchBend {
        value: u8,
    },
    /// Timing clock, sent 24 times per quarter note
    Clock {
        timing: jack::Frames,
    },
    /// Start the sequence from the beginning
    Start {
        timing: jack::Frames,
    },
    /// Resume the sequence from where it was stopped
    Continue {
        timing: jack::Frames,
    },
    /// Stop the sequence
    Stop {
        timing: jack::Frames,
    },
    Unknown {
        d1: u8,
        d2: u8,
        d3: u8,
    },
}

/// Function to retrieve the name of a note based on its index
fn index_to_name(index: usize) -> &'static str {
    match index % 12 {
        00 => "C ",
        01 => "C#",
        02 => "D ",
        03 => "D#",
        04 => "E ",
        05 => "F ",
        06 => "F#",
        07 => "G ",
        08 => "G#",
        09 => "A ",
        10 => "A#",
        11 => "B ",
        _ => {
            assert!(false);
            " UNKNOWN NOTE "
        }
    }
}

/// Function to retrieve the octave of a note based on its index
fn index_to_octave(index: usize) -> usize {
    index / 12
}

impl MidiInput {
    pub fn to_raw<'data>(&self, bytes: &'data mut [u8]) -> jack::RawMidi<'data> {
        match self {
            MidiInput::NoteStart {
                channel,
                note_index,
                timing,
                velocity,
            } => {
                bytes[0] = 0x90 | (*channel & 0x0F);
                bytes[1] = (*note_index - 12) as u8;
                bytes[2] = (velocity * 256.0) as u8;
                jack::RawMidi {
                    time: *timing,
                    bytes,
                }
            }
            MidiInput::NoteEnd {
                channel,
                note_index,
                timing,
                velocity,
            } => {
                bytes[0] = 0x80 | (*channel & 0x0F);
                assert!(*note_index >= 12);
                bytes[1] = (*note_index - 12) as u8;
                bytes[2] = (velocity * 256.0) as u8;
                jack::RawMidi {
                    time: *timing,
                    bytes,
                }
            }
            MidiInput::Controller {
                channel,
                control,
                value,
            } => {
                bytes[0] = (0xB0 as u8) | (*channel & 0x0F);
                bytes[1] = *control;
                bytes[2] = *value;
                jack::RawMidi { time: 0, bytes }
            }
            MidiInput::PitchBend { value } => {
                bytes[0] = 0xE0;
                bytes[2] = *value;
                jack::RawMidi { time: 0, bytes }
            }
            MidiInput::Clock { timing } => {
                bytes[0] = CLOCK;
                jack::RawMidi {
                    time: *timing,
                    bytes: &bytes[..1],
                }
            }
            MidiInput::Start { timing } => {
                bytes[0] = START;
                jack::RawMidi {
                    time: *timing,
                    bytes: &bytes[..1],
                }
            }
            MidiInput::Continue { timing } => {
                bytes[0] = CONTINUE;
                jack::RawMidi {
                    time: *timing,
                    bytes: &bytes[..1],
                }
            }
            MidiInput::Stop { timing } => {
                bytes[0] = STOP;
                jack::RawMidi {
                    time: *timing,
                    bytes: &bytes[..1],
                }
            }
            MidiInput::Unknown { d1, d2, d3 } => {
                bytes[0] = *d1;
                bytes[1] = *d2;
                bytes[2] = *d3;
                jack::RawMidi { time: 0, bytes }
            }
        }
    }
}

impl std::fmt::Debug for MidiInput {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match self {
            MidiInput::NoteStart {
                channel,
                note_index,
                timing,
                velocity,
            } => {
                write!(
                    f,
                    "NoteStart {{ channel: {}, note: {}{}, timing: {}, velocity: {} }}",
                    channel,
                    index_to_name(*note_index),
                    index_to_octave(*note_index),
                    timing,
                    velocity,
                )
            }
            MidiInput::NoteEnd {
                channel,
                note_index,
                timing,
                velocity,
            } => {
                write!(
                    f,
                    "NoteEnd   {{ channel: {}, note: {}{}, timing: {} velocity: {} }}",
                    channel,
                    index_to_name(*note_index),
                    index_to_octave(*note_index),
                    timing,
                    velocity,
                )
            }
            MidiInput::Controller {
                channel,
                value,
                control,
            } => {
                write!(
                    f,
                    "Control {{ channel: {}, control: {}, value: {} }}",
                    channel, control, value
                )
            }
            MidiInput::PitchBend { value } => {
                write!(f, "Pitch bend {{ {} }}", value)
            }
            MidiInput::Clock { timing } => {
                write!(f, "Clock {{ timing: {} }}", timing)
            }
            MidiInput::Start { timing } => {
                write!(f, "Start {{ timing: {} }}", timing)
            }
            MidiInput::Continue { timing } => {
                write!(f, "Continue {{ timing: {} }}", timing)
            }
            MidiInput::Stop { timing } => {
                write!(f, "Stop {{ timing: {} }}", timing)
            }
            MidiInput::Unknown { d1, d2, d3 } => {
                write!(f, "Unknown {{ d1: {}, d2: {}, d3: {} }}", d1, d2, d3)
            }
        }
    }
}

impl From<jack::RawMidi<'_>> for MidiInput {
    fn from(midi: jack::RawMidi<'_>) -> Self {
        let len = std::cmp::min(MAX_MIDI, midi.bytes.len());
        let header_byte = midi.bytes[0];
        if (0xF0 & header_byte) == (0x90 as u8) {
            assert!(len > 2);
            MidiInput::NoteStart {
                channel: 0x0F & header_byte,
                note_index: 12 + (midi.bytes[1] as usize),
                timing: midi.time,
                velocity: (midi.bytes[2] as f64) / 256.0,
            }
        } else if (0xF0 & header_byte) == (0x80 as u8) {
            assert!(len > 1);
            MidiInput::NoteEnd {
                channel: 0x0F & header_byte,
                note_index: 12 + (midi.bytes[1] as usize),
                timing: midi.time,
                velocity: (midi.bytes[2] as f64) / 256.0,
            }
        } else if (0xF0 & header_byte) == (0xB0 as u8) {
            let channel = (0x0F & header_byte) as u8;
            let controller = midi.bytes[1];
            MidiInput::Controller {
                channel: channel,
                control: controller,
                value: midi.bytes[2],
            }
        } else if (0xF0 & header_byte) == (0xE0 as u8) {
            assert!(len > 2);
            MidiInput::PitchBend {
                value: midi.bytes[2],
            }
        } else if header_byte == CLOCK {
            MidiInput::Clock { timing: midi.time }
        } else if header_byte == START {
            MidiInput::Start { timing: midi.time }
        } else if header_byte == CONTINUE {
            MidiInput::Continue { timing: midi.time }
        } else if header_byte == STOP {
            MidiInput::Stop { timing: midi.time }
        } else {
            MidiInput::Unknown {
                d1: if len > 0 { midi.bytes[0] } else { 0 as u8 },
                d2: if len > 1 { midi.bytes[1] } else { 0 as u8 },
                d3: if len > 2 { midi.bytes[2] } else { 0 as u8 },
            }
        }
    }
}

#[cfg(test)]
mod test {
    use jack::RawMidi;

    use super::MidiInput;

    #[test]
    fn realtime_messages() {
        let messages = [
            (0xF8, MidiInput::Clock { timing: 42 }),
            (0xFA, MidiInput::Start { timing: 42 }),
            (0xFB, MidiInput::Continue { timing: 42 }),
            (0xFC, MidiInput::Stop { timing: 42 }),
        ];
        for (byte, expected) in messages {
            let d = [byte];
            let input = MidiInput::from(RawMidi {
                time: 42,
                bytes: &d,
            });
            assert_eq!(input, expected);

            let mut filled_bytes = vec![0; 3];
            let raw = input.to_raw(&mut filled_bytes);
            assert_eq!(raw.time, 42);
            assert_eq!(raw.bytes, &[byte]);
        }
    }

    #[test]
    fn midi_to_raw() {
        for i in 0..u32::MAX {
            let d: Vec<u8> = i.to_le_bytes().into();
            let raw = RawMidi { time: 0, bytes: &d };
            let input = MidiInput::from(raw);
            let mut filled_bytes = vec![0; 4];
            let new_raw = input.to_raw(&mut filled_bytes);
            debug_assert_eq!(input, MidiInput::from(new_raw));
        }
    }
}
