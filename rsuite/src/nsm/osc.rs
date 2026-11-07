//! Minimal implementation of the Open Sound Control (OSC) messages, as needed
//! to talk with a session manager. Bundles are not supported.

///An argument of an OSC message
#[derive(Debug, Clone, PartialEq)]
pub enum Arg {
    Int(i32),
    Float(f32),
    Str(String),
}

impl Arg {
    ///Retrieve the value of a string argument
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Arg::Str(s) => Some(s),
            _ => None,
        }
    }
}

impl From<&str> for Arg {
    fn from(value: &str) -> Self {
        Arg::Str(String::from(value))
    }
}

impl From<i32> for Arg {
    fn from(value: i32) -> Self {
        Arg::Int(value)
    }
}

///An OSC message: an address followed by a list of arguments
#[derive(Debug, Clone, PartialEq)]
pub struct Message {
    pub address: String,
    pub args: Vec<Arg>,
}

impl Message {
    pub fn new(address: &str, args: Vec<Arg>) -> Message {
        Message {
            address: String::from(address),
            args,
        }
    }

    ///Retrieve the string argument at the given position
    pub fn str_arg(&self, index: usize) -> Option<&str> {
        self.args.get(index).and_then(Arg::as_str)
    }

    ///Encode the message so that it can be sent through the network
    pub fn encode(&self) -> Vec<u8> {
        let mut out = Vec::with_capacity(64);
        write_string(&mut out, &self.address);
        let mut tags = String::from(",");
        for arg in &self.args {
            tags.push(match arg {
                Arg::Int(_) => 'i',
                Arg::Float(_) => 'f',
                Arg::Str(_) => 's',
            });
        }
        write_string(&mut out, &tags);
        for arg in &self.args {
            match arg {
                Arg::Int(v) => out.extend_from_slice(&v.to_be_bytes()),
                Arg::Float(v) => out.extend_from_slice(&v.to_be_bytes()),
                Arg::Str(s) => write_string(&mut out, s),
            }
        }
        out
    }

    ///Decode a message received from the network. None is returned if the
    /// packet is not a valid message or uses an unsupported type
    pub fn decode(packet: &[u8]) -> Option<Message> {
        let mut position = 0;
        let address = read_string(packet, &mut position)?;
        if !address.starts_with('/') {
            //bundles start with '#'
            return None;
        }
        //very old implementations may not send the type tags
        if position >= packet.len() {
            return Some(Message::new(&address, Vec::new()));
        }
        let tags = read_string(packet, &mut position)?;
        let tags = tags.strip_prefix(',')?;
        let mut args = Vec::with_capacity(tags.len());
        for tag in tags.chars() {
            let arg = match tag {
                'i' => Arg::Int(i32::from_be_bytes(read_word(packet, &mut position)?)),
                'f' => Arg::Float(f32::from_be_bytes(read_word(packet, &mut position)?)),
                's' | 'S' => Arg::Str(read_string(packet, &mut position)?),
                _ => return None,
            };
            args.push(arg);
        }
        Some(Message::new(&address, args))
    }
}

///Write a null terminated string, padded to a multiple of 4 bytes
fn write_string(out: &mut Vec<u8>, s: &str) {
    out.extend_from_slice(s.as_bytes());
    let padding = 4 - s.len() % 4;
    out.extend(std::iter::repeat(0).take(padding));
}

///Read a null terminated string padded to a multiple of 4 bytes
fn read_string(packet: &[u8], position: &mut usize) -> Option<String> {
    let start = *position;
    let length = packet.get(start..)?.iter().position(|b| *b == 0)?;
    let value = std::str::from_utf8(&packet[start..start + length]).ok()?;
    *position = start + (length / 4 + 1) * 4;
    Some(String::from(value))
}

///Read the next 4 bytes
fn read_word(packet: &[u8], position: &mut usize) -> Option<[u8; 4]> {
    let word = packet.get(*position..*position + 4)?;
    *position += 4;
    word.try_into().ok()
}

#[cfg(test)]
mod test {
    use super::{Arg, Message};

    #[test]
    fn encode_announce() {
        let msg = Message::new("/ab", vec![Arg::from("abcd"), Arg::from(1)]);
        let expected: Vec<u8> = [
            b"/ab\0".as_slice(),
            b",si\0",
            b"abcd\0\0\0\0",
            &[0, 0, 0, 1],
        ]
        .concat();
        assert_eq!(msg.encode(), expected);
    }

    #[test]
    fn round_trip() {
        let msg = Message::new(
            "/nsm/client/open",
            vec![
                Arg::from("/home/user/session/client"),
                Arg::from("display"),
                Arg::from("nABCD"),
                Arg::Int(-12),
                Arg::Float(0.5),
            ],
        );
        assert_eq!(Message::decode(&msg.encode()), Some(msg));
    }

    #[test]
    fn invalid_packets() {
        assert_eq!(Message::decode(b""), None);
        assert_eq!(Message::decode(b"#bundle\0"), None);
        //missing argument
        assert_eq!(Message::decode(b"/a\0\0,i\0\0"), None);
        //unknown type
        assert_eq!(Message::decode(b"/a\0\0,b\0\0\0\0\0\0"), None);
        //no type tag at all
        assert_eq!(
            Message::decode(b"/a\0\0"),
            Some(Message::new("/a", Vec::new()))
        );
    }
}
