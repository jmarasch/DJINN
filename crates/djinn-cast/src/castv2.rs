//! The Google Cast wire format (CASTV2): JSON messages wrapped in a small
//! protobuf (`CastMessage`), each frame prefixed with its length, over TLS
//! on port 8009. The protobuf has six fields, so it's encoded by hand.

/// One message on a Cast channel.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct CastMessage {
    pub source: String,
    pub destination: String,
    pub namespace: String,
    /// JSON text.
    pub payload: String,
}

pub const NS_CONNECTION: &str = "urn:x-cast:com.google.cast.tp.connection";
pub const NS_HEARTBEAT: &str = "urn:x-cast:com.google.cast.tp.heartbeat";
pub const NS_RECEIVER: &str = "urn:x-cast:com.google.cast.receiver";
pub const NS_MEDIA: &str = "urn:x-cast:com.google.cast.media";

/// Longest frame accepted from a device.
pub const MAX_FRAME: usize = 64 * 1024;

impl CastMessage {
    pub fn new(source: &str, destination: &str, namespace: &str, payload: String) -> Self {
        Self {
            source: source.to_owned(),
            destination: destination.to_owned(),
            namespace: namespace.to_owned(),
            payload,
        }
    }

    /// The length-prefixed frame.
    pub fn encode(&self) -> Vec<u8> {
        let mut body = Vec::with_capacity(64 + self.payload.len());
        // 1: protocol_version = CASTV2_1_0 (0)
        put_varint(&mut body, 1 << 3);
        put_varint(&mut body, 0);
        put_string(&mut body, 2, &self.source);
        put_string(&mut body, 3, &self.destination);
        put_string(&mut body, 4, &self.namespace);
        // 5: payload_type = STRING (0)
        put_varint(&mut body, 5 << 3);
        put_varint(&mut body, 0);
        put_string(&mut body, 6, &self.payload);
        let mut frame = Vec::with_capacity(body.len() + 4);
        frame.extend_from_slice(&u32::try_from(body.len()).unwrap_or(u32::MAX).to_be_bytes());
        frame.extend_from_slice(&body);
        frame
    }

    /// Decodes a frame body (without its length prefix).
    pub fn decode(mut body: &[u8]) -> Option<Self> {
        let mut msg = Self::new("", "", "", String::new());
        while !body.is_empty() {
            let key = take_varint(&mut body)?;
            let (field, wire) = (key >> 3, key & 7);
            match wire {
                0 => {
                    take_varint(&mut body)?;
                }
                2 => {
                    let len = usize::try_from(take_varint(&mut body)?).ok()?;
                    if len > body.len() {
                        return None;
                    }
                    let (value, rest) = body.split_at(len);
                    body = rest;
                    let text = || String::from_utf8_lossy(value).into_owned();
                    match field {
                        2 => msg.source = text(),
                        3 => msg.destination = text(),
                        4 => msg.namespace = text(),
                        6 => msg.payload = text(),
                        _ => {}
                    }
                }
                // Fixed 64 and 32 bit fields: not used by Cast, skip.
                1 => body = body.get(8..)?,
                5 => body = body.get(4..)?,
                _ => return None,
            }
        }
        Some(msg)
    }
}

fn put_varint(out: &mut Vec<u8>, mut value: u64) {
    while value >= 0x80 {
        out.push((value as u8) | 0x80);
        value >>= 7;
    }
    out.push(value as u8);
}

fn put_string(out: &mut Vec<u8>, field: u64, value: &str) {
    put_varint(out, (field << 3) | 2);
    put_varint(out, value.len() as u64);
    out.extend_from_slice(value.as_bytes());
}

fn take_varint(buf: &mut &[u8]) -> Option<u64> {
    let mut value = 0u64;
    for shift in (0..64).step_by(7) {
        let (&byte, rest) = buf.split_first()?;
        *buf = rest;
        value |= u64::from(byte & 0x7f) << shift;
        if byte & 0x80 == 0 {
            return Some(value);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips() {
        let msg = CastMessage::new(
            "sender-0",
            "receiver-0",
            NS_RECEIVER,
            format!(
                r#"{{"type":"LAUNCH","appId":"CC1AD845","pad":"{}"}}"#,
                "x".repeat(300)
            ),
        );
        let frame = msg.encode();
        let len = u32::from_be_bytes([frame[0], frame[1], frame[2], frame[3]]) as usize;
        assert_eq!(len, frame.len() - 4);
        assert_eq!(CastMessage::decode(&frame[4..]), Some(msg));
    }

    #[test]
    fn matches_the_reference_encoding() {
        // CONNECT as other Cast senders put it on the wire.
        let msg = CastMessage::new("s", "r", "n", "{}".to_owned());
        assert_eq!(
            &msg.encode()[4..],
            [
                0x08, 0x00, 0x12, 1, b's', 0x1a, 1, b'r', 0x22, 1, b'n', 0x28, 0x00, 0x32, 2, b'{',
                b'}'
            ]
        );
    }

    #[test]
    fn rejects_truncated_frames() {
        let frame = CastMessage::new("a", "b", "c", "{}".into()).encode();
        assert!(CastMessage::decode(&frame[4..frame.len() - 1]).is_none());
    }
}
