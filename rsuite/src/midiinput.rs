use std::convert::From;

const MAX_MIDI: usize = 3;

///The part of a status byte giving the type of a channel message
pub const STATUS_MASK: u8 = 0xF0;
///The part of a status byte giving the channel of a channel message
pub const CHANNEL_MASK: u8 = 0x0F;
pub const NOTE_START: u8 = 0x90;
pub const NOTE_END: u8 = 0x80;
pub const CONTROLLER: u8 = 0xB0;
pub const PITCH_BEND: u8 = 0xE0;

const CLOCK: u8 = 0xF8;
const START: u8 = 0xFA;
const CONTINUE: u8 = 0xFB;
const STOP: u8 = 0xFC;
const SYSEX_START: u8 = 0xF0;
const SYSEX_END: u8 = 0xF7;

///First data byte of the universal real time system exclusive messages
const UNIVERSAL_REAL_TIME: u8 = 0x7F;
///Sub id of the universal real time messages giving notation information
const NOTATION: u8 = 0x03;
///Notation information sub id of the bar marker
const BAR_MARKER: u8 = 0x01;
///Notation information sub id of the time signature applied immediately
const TIME_SIGNATURE: u8 = 0x02;
///Notation information sub id of the time signature applied at the next bar
const TIME_SIGNATURE_DELAYED: u8 = 0x42;
///Number of data bytes following the length in a time signature without additional
/// numerator/denominator pairs
const TIME_SIGNATURE_LEN: u8 = 4;

///Device id of the universal messages sent to every device
pub const ALL_DEVICES: u8 = 0x7F;
///Value of a bar marker when the sequence is not running
pub const BAR_NOT_RUNNING: i16 = -8192;
///Value of a bar marker when the sequence is running but the bar number is unknown
pub const BAR_UNKNOWN: i16 = 8191;

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
    /// System exclusive message. The data starts with the manufacturer id and
    /// contains neither the start (0xF0) nor the end (0xF7) of the message
    SysEx {
        timing: jack::Frames,
        data: Vec<u8>,
    },
    /// Bar marker (universal real time system exclusive message), sent at the start of a bar.
    /// The bar is between -8192 and 8191: BAR_NOT_RUNNING when the sequence is stopped,
    /// a negative value during a count-in and BAR_UNKNOWN when the bar number is unknown
    BarMarker {
        timing: jack::Frames,
        /// The device the message is sent to, 0x7F for every device
        device_id: u8,
        bar: i16,
    },
    /// Time signature (universal real time system exclusive message). Time signatures having
    /// additional numerator/denominator pairs are kept as generic system exclusive messages.
    /// Every value must be lower than 0x80
    TimeSignature {
        timing: jack::Frames,
        /// The device the message is sent to, 0x7F for every device
        device_id: u8,
        /// If true, the time signature applies from the next bar. Otherwise, immediately
        delayed: bool,
        /// The number of beats per bar
        numerator: u8,
        /// The beat unit is 2 to the power of this value (2 for a quarter note)
        denominator_power: u8,
        /// The number of midi clocks per metronome click
        clocks_per_click: u8,
        /// The number of 32nd notes per midi quarter note (24 midi clocks), usually 8
        thirty_seconds_per_quarter: u8,
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
    ///Convert the message to raw midi, using 'bytes' as storage.
    ///'bytes' must be long enough to contain the message: 3 bytes are enough for
    /// every message, except for the system exclusive ones that need 2 more bytes than their data.
    /// A bar marker needs 8 bytes and a time signature 11 bytes
    pub fn to_raw<'data>(&self, bytes: &'data mut [u8]) -> jack::RawMidi<'data> {
        match self {
            MidiInput::NoteStart {
                channel,
                note_index,
                timing,
                velocity,
            } => {
                bytes[0] = NOTE_START | (*channel & CHANNEL_MASK);
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
                bytes[0] = NOTE_END | (*channel & CHANNEL_MASK);
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
                bytes[0] = CONTROLLER | (*channel & CHANNEL_MASK);
                bytes[1] = *control;
                bytes[2] = *value;
                jack::RawMidi { time: 0, bytes }
            }
            MidiInput::PitchBend { value } => {
                bytes[0] = PITCH_BEND;
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
            MidiInput::SysEx { timing, data } => write_sysex(bytes, data, *timing),
            MidiInput::BarMarker {
                timing,
                device_id,
                bar,
            } => {
                //the bar is stored on 14 bits, in two's complement
                let value = (*bar as i32).rem_euclid(1 << 14);
                let data = [
                    UNIVERSAL_REAL_TIME,
                    *device_id,
                    NOTATION,
                    BAR_MARKER,
                    (value & 0x7F) as u8,
                    (value >> 7) as u8,
                ];
                write_sysex(bytes, &data, *timing)
            }
            MidiInput::TimeSignature {
                timing,
                device_id,
                delayed,
                numerator,
                denominator_power,
                clocks_per_click,
                thirty_seconds_per_quarter,
            } => {
                let data = [
                    UNIVERSAL_REAL_TIME,
                    *device_id,
                    NOTATION,
                    if *delayed {
                        TIME_SIGNATURE_DELAYED
                    } else {
                        TIME_SIGNATURE
                    },
                    TIME_SIGNATURE_LEN,
                    *numerator,
                    *denominator_power,
                    *clocks_per_click,
                    *thirty_seconds_per_quarter,
                ];
                write_sysex(bytes, &data, *timing)
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
            MidiInput::SysEx { timing, data } => {
                write!(f, "SysEx {{ timing: {}, data: {:02X?} }}", timing, data)
            }
            MidiInput::BarMarker {
                timing,
                device_id,
                bar,
            } => {
                write!(
                    f,
                    "BarMarker {{ timing: {}, device: {}, bar: {} }}",
                    timing, device_id, bar
                )
            }
            MidiInput::TimeSignature {
                timing,
                device_id,
                delayed,
                numerator,
                denominator_power,
                clocks_per_click,
                thirty_seconds_per_quarter,
            } => {
                write!(
                    f,
                    "TimeSignature {{ timing: {}, device: {}, delayed: {}, signature: {}/2^{}, clocks per click: {}, 32nd per quarter: {} }}",
                    timing,
                    device_id,
                    delayed,
                    numerator,
                    denominator_power,
                    clocks_per_click,
                    thirty_seconds_per_quarter
                )
            }
            MidiInput::Unknown { d1, d2, d3 } => {
                write!(f, "Unknown {{ d1: {}, d2: {}, d3: {} }}", d1, d2, d3)
            }
        }
    }
}

///Check if the bytes form a complete system exclusive message: a start,
/// data bytes (lower than 0x80) and an end
fn is_sysex(bytes: &[u8]) -> bool {
    bytes.len() >= 2
        && bytes[0] == SYSEX_START
        && bytes[bytes.len() - 1] == SYSEX_END
        && bytes[1..bytes.len() - 1].iter().all(|b| *b < 0x80)
}

///Write a system exclusive message containing 'data' in 'bytes'
fn write_sysex<'data>(
    bytes: &'data mut [u8],
    data: &[u8],
    timing: jack::Frames,
) -> jack::RawMidi<'data> {
    let len = data.len() + 2;
    bytes[0] = SYSEX_START;
    bytes[1..len - 1].copy_from_slice(data);
    bytes[len - 1] = SYSEX_END;
    jack::RawMidi {
        time: timing,
        bytes: &bytes[..len],
    }
}

///Retrieve the notation information (bar marker or time signature) contained in the data
/// of a system exclusive message
fn parse_notation(timing: jack::Frames, data: &[u8]) -> Option<MidiInput> {
    if data.len() < 4 || data[0] != UNIVERSAL_REAL_TIME || data[2] != NOTATION {
        return None;
    }
    let device_id = data[1];
    match (data[3], data.len()) {
        (BAR_MARKER, 6) => {
            //the bar is stored on 14 bits, in two's complement
            let value = data[4] as i16 | (data[5] as i16) << 7;
            let bar = if value > BAR_UNKNOWN {
                value - (1 << 14)
            } else {
                value
            };
            Some(MidiInput::BarMarker {
                timing,
                device_id,
                bar,
            })
        }
        (TIME_SIGNATURE | TIME_SIGNATURE_DELAYED, 9) if data[4] == TIME_SIGNATURE_LEN => {
            Some(MidiInput::TimeSignature {
                timing,
                device_id,
                delayed: data[3] == TIME_SIGNATURE_DELAYED,
                numerator: data[5],
                denominator_power: data[6],
                clocks_per_click: data[7],
                thirty_seconds_per_quarter: data[8],
            })
        }
        _ => None,
    }
}

impl From<jack::RawMidi<'_>> for MidiInput {
    fn from(midi: jack::RawMidi<'_>) -> Self {
        let len = std::cmp::min(MAX_MIDI, midi.bytes.len());
        let header_byte = midi.bytes[0];
        if (STATUS_MASK & header_byte) == NOTE_START {
            assert!(len > 2);
            MidiInput::NoteStart {
                channel: CHANNEL_MASK & header_byte,
                note_index: 12 + (midi.bytes[1] as usize),
                timing: midi.time,
                velocity: (midi.bytes[2] as f64) / 256.0,
            }
        } else if (STATUS_MASK & header_byte) == NOTE_END {
            assert!(len > 1);
            MidiInput::NoteEnd {
                channel: CHANNEL_MASK & header_byte,
                note_index: 12 + (midi.bytes[1] as usize),
                timing: midi.time,
                velocity: (midi.bytes[2] as f64) / 256.0,
            }
        } else if (STATUS_MASK & header_byte) == CONTROLLER {
            let channel = CHANNEL_MASK & header_byte;
            let controller = midi.bytes[1];
            MidiInput::Controller {
                channel: channel,
                control: controller,
                value: midi.bytes[2],
            }
        } else if (STATUS_MASK & header_byte) == PITCH_BEND {
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
        } else if is_sysex(midi.bytes) {
            let data = &midi.bytes[1..midi.bytes.len() - 1];
            match parse_notation(midi.time, data) {
                Some(notation) => notation,
                None => MidiInput::SysEx {
                    timing: midi.time,
                    data: data.to_vec(),
                },
            }
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

    use super::{MidiInput, BAR_NOT_RUNNING, BAR_UNKNOWN};

    ///Check that the bytes give the expected message and that the message gives back the bytes
    fn check_round_trip(d: &[u8], expected: MidiInput) {
        let input = MidiInput::from(RawMidi { time: 42, bytes: d });
        assert_eq!(input, expected);

        let mut filled_bytes = vec![0; d.len()];
        let raw = input.to_raw(&mut filled_bytes);
        assert_eq!(raw.time, 42);
        assert_eq!(raw.bytes, d);
    }

    #[test]
    fn bar_markers() {
        let bars: [([u8; 2], i16); 6] = [
            ([0x01, 0x00], 1),
            ([0x0A, 0x01], 138),
            ([0x00, 0x40], BAR_NOT_RUNNING),
            ([0x7F, 0x7F], -1),
            ([0x7E, 0x7F], -2),
            ([0x7F, 0x3F], BAR_UNKNOWN),
        ];
        for ([lsb, msb], bar) in bars {
            let d = [0xF0, 0x7F, 0x7F, 0x03, 0x01, lsb, msb, 0xF7];
            check_round_trip(
                &d,
                MidiInput::BarMarker {
                    timing: 42,
                    device_id: 0x7F,
                    bar,
                },
            );
        }
    }

    #[test]
    fn time_signatures() {
        let d = [
            0xF0, 0x7F, 0x10, 0x03, 0x02, 0x04, 0x06, 0x03, 0x24, 0x08, 0xF7,
        ];
        check_round_trip(
            &d,
            MidiInput::TimeSignature {
                timing: 42,
                device_id: 0x10,
                delayed: false,
                numerator: 6,
                denominator_power: 3,
                clocks_per_click: 36,
                thirty_seconds_per_quarter: 8,
            },
        );

        let d = [
            0xF0, 0x7F, 0x7F, 0x03, 0x42, 0x04, 0x03, 0x02, 0x18, 0x08, 0xF7,
        ];
        check_round_trip(
            &d,
            MidiInput::TimeSignature {
                timing: 42,
                device_id: 0x7F,
                delayed: true,
                numerator: 3,
                denominator_power: 2,
                clocks_per_click: 24,
                thirty_seconds_per_quarter: 8,
            },
        );
    }

    #[test]
    fn other_notations_are_generic_sysex() {
        for d in [
            //compound time signature
            vec![
                0xF0, 0x7F, 0x7F, 0x03, 0x02, 0x06, 0x03, 0x02, 0x18, 0x08, 0x02, 0x03, 0xF7,
            ],
            //bar marker missing a byte
            vec![0xF0, 0x7F, 0x7F, 0x03, 0x01, 0x01, 0xF7],
            //non real time message
            vec![0xF0, 0x7E, 0x7F, 0x03, 0x01, 0x01, 0x00, 0xF7],
        ] {
            check_round_trip(
                &d,
                MidiInput::SysEx {
                    timing: 42,
                    data: d[1..d.len() - 1].to_vec(),
                },
            );
        }
    }

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
    fn sysex_messages() {
        let d = [0xF0, 0x7E, 0x7F, 0x06, 0x01, 0xF7];
        let input = MidiInput::from(RawMidi {
            time: 42,
            bytes: &d,
        });
        assert_eq!(
            input,
            MidiInput::SysEx {
                timing: 42,
                data: vec![0x7E, 0x7F, 0x06, 0x01],
            }
        );

        let mut filled_bytes = vec![0; d.len()];
        let raw = input.to_raw(&mut filled_bytes);
        assert_eq!(raw.time, 42);
        assert_eq!(raw.bytes, &d);
    }

    #[test]
    fn empty_sysex_message() {
        let d = [0xF0, 0xF7];
        let input = MidiInput::from(RawMidi { time: 0, bytes: &d });
        assert_eq!(
            input,
            MidiInput::SysEx {
                timing: 0,
                data: Vec::new(),
            }
        );
    }

    #[test]
    fn incomplete_sysex_messages_are_unknown() {
        for d in [
            vec![0xF0, 0x7E, 0x7F],
            vec![0xF0, 0x7E, 0x90, 0xF7],
            vec![0xF0],
        ] {
            let input = MidiInput::from(RawMidi { time: 0, bytes: &d });
            assert!(
                matches!(input, MidiInput::Unknown { .. }),
                "{d:02X?} gave {input:?}"
            );
        }
    }

    /*#[test]
    fn midi_to_raw() {
        for i in 0..u32::MAX {
            let d: Vec<u8> = i.to_le_bytes().into();
            let raw = RawMidi { time: 0, bytes: &d };
            let input = MidiInput::from(raw);
            let mut filled_bytes = vec![0; 4];
            let new_raw = input.to_raw(&mut filled_bytes);
            debug_assert_eq!(input, MidiInput::from(new_raw));
        }
    }*/
}
