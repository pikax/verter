//! The tsgo `--api` tuple framing.
//!
//! Mirrors `dist/api/syncChannel.js`. Every message on the wire is a
//! MessagePack 3-element fixarray `[MessageType, name, payload]` where:
//! - `MessageType` is a `u8` (a positive fixint, or `0xcc` + u8 — syncChannel.js:331-340),
//! - `name` is the method/callback name as a `bin` field, and
//! - `payload` is a `bin` field (UTF-8 JSON for the high-level ops).
//!
//! The write side mirrors `writeTuple` (syncChannel.js:264-317): `0x93`, the
//! type byte, then the `name` and `payload` bin fields. The read side mirrors
//! `readTuple` (syncChannel.js:324-343).

use crate::error::{TsgoApiError, TsgoApiResult};
use crate::proto::msgpack::{MsgpackReader, MSGPACK_FIXARRAY3};

/// Message type tags mirrored from `syncChannel.js:15-23`.
///
/// Parent → child: [`Request`](MessageType::Request),
/// [`CallResponse`](MessageType::CallResponse), [`CallError`](MessageType::CallError).
/// Child → parent: [`Response`](MessageType::Response),
/// [`Error`](MessageType::Error), [`Call`](MessageType::Call).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum MessageType {
    /// Parent → child request. `MSG_REQUEST = 1`.
    Request = 1,
    /// Parent → child callback response. `MSG_CALL_RESPONSE = 2`.
    CallResponse = 2,
    /// Parent → child callback error. `MSG_CALL_ERROR = 3`.
    CallError = 3,
    /// Child → parent response. `MSG_RESPONSE = 4`.
    Response = 4,
    /// Child → parent error. `MSG_ERROR = 5`.
    Error = 5,
    /// Child → parent host callback invocation. `MSG_CALL = 6`.
    Call = 6,
}

impl MessageType {
    /// The raw wire byte value for this message type.
    pub fn as_u8(self) -> u8 {
        self as u8
    }

    /// Decode a message type byte. Unknown values are a typed codec error
    /// (mirrors syncChannel.js:225-226 "Invalid message type from child").
    pub fn from_u8(value: u8) -> TsgoApiResult<Self> {
        match value {
            1 => Ok(MessageType::Request),
            2 => Ok(MessageType::CallResponse),
            3 => Ok(MessageType::CallError),
            4 => Ok(MessageType::Response),
            5 => Ok(MessageType::Error),
            6 => Ok(MessageType::Call),
            other => Err(TsgoApiError::Codec(format!(
                "invalid message type byte {other:#04x}"
            ))),
        }
    }
}

/// One decoded tuple frame `[type, name, payload]`. The `name` and `payload`
/// fields borrow from the source buffer to avoid copying large payloads.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Frame<'a> {
    /// The message type tag.
    pub msg_type: MessageType,
    /// The method/callback name bytes (UTF-8).
    pub name: &'a [u8],
    /// The payload bytes (UTF-8 JSON for high-level ops; raw for binary ops).
    pub payload: &'a [u8],
}

/// Encode a tuple frame `[type, name, payload]` into a fresh byte buffer.
///
/// Mirrors `writeTuple` (syncChannel.js:264-317): the leading `0x93`, the type
/// byte written as a bare positive fixint (every [`MessageType`] is ≤ 6, so it
/// is always a fixint — the reference also accepts a `0xcc`+u8 form on read),
/// then the `name` and `payload` bin fields.
pub fn encode_frame(msg_type: MessageType, name: &[u8], payload: &[u8]) -> Vec<u8> {
    let mut out = Vec::with_capacity(
        2 + crate::proto::msgpack::bin_header_size(name.len())
            + name.len()
            + crate::proto::msgpack::bin_header_size(payload.len())
            + payload.len(),
    );
    out.push(MSGPACK_FIXARRAY3);
    out.push(msg_type.as_u8());
    crate::proto::msgpack::write_bin_header(&mut out, name.len());
    out.extend_from_slice(name);
    crate::proto::msgpack::write_bin_header(&mut out, payload.len());
    out.extend_from_slice(payload);
    out
}

/// Decode a single tuple frame from `data` starting at `offset`.
///
/// Mirrors `readTuple` (syncChannel.js:324-343): the `0x93` marker, the type
/// byte (a positive fixint or `0xcc`+u8), then the `name` and `payload` bin
/// fields. Returns the decoded frame and the number of bytes consumed.
pub fn decode_frame(data: &[u8], offset: usize) -> TsgoApiResult<(Frame<'_>, usize)> {
    let mut r = MsgpackReader::new(data, offset);
    // Fixed 3-element array marker (syncChannel.js:326-329).
    let array_len = r.read_array_header()?;
    if array_len != 3 {
        return Err(TsgoApiError::Codec(format!(
            "expected fixed 3-element array (0x93), got array of length {array_len}"
        )));
    }
    // Message type — positive fixint or uint8 (syncChannel.js:331-340).
    let type_byte = read_msg_type_byte(&mut r)?;
    let msg_type = MessageType::from_u8(type_byte)?;
    let name = r.read_bin()?;
    let payload = r.read_bin()?;
    let consumed = r.position() - offset;
    Ok((
        Frame {
            msg_type,
            name,
            payload,
        },
        consumed,
    ))
}

/// Read the message-type byte: a positive fixint (`<= 0x7f`) read directly, or
/// `0xcc` + a following u8. Mirrors syncChannel.js:331-340.
fn read_msg_type_byte(r: &mut MsgpackReader<'_>) -> TsgoApiResult<u8> {
    // Peek the marker by reading a uint, but the reference distinguishes only
    // fixint vs uint8; reuse the msgpack uint reader which accepts both forms
    // (and the larger uint16/uint32 forms, which never appear for a type byte).
    let value = r.read_uint()?;
    u8::try_from(value)
        .map_err(|_| TsgoApiError::Codec(format!("message type {value} out of u8 range")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::proto::msgpack::{MSGPACK_BIN8, MSGPACK_UINT8};

    #[test]
    fn message_type_roundtrips_all_tags() {
        for (mt, byte) in [
            (MessageType::Request, 1u8),
            (MessageType::CallResponse, 2),
            (MessageType::CallError, 3),
            (MessageType::Response, 4),
            (MessageType::Error, 5),
            (MessageType::Call, 6),
        ] {
            assert_eq!(mt.as_u8(), byte);
            assert_eq!(MessageType::from_u8(byte).unwrap(), mt);
        }
    }

    #[test]
    fn message_type_rejects_unknown_byte() {
        assert!(matches!(
            MessageType::from_u8(0),
            Err(TsgoApiError::Codec(_))
        ));
        assert!(matches!(
            MessageType::from_u8(7),
            Err(TsgoApiError::Codec(_))
        ));
        assert!(matches!(
            MessageType::from_u8(255),
            Err(TsgoApiError::Codec(_))
        ));
    }

    // ── encode mirrors writeTuple (syncChannel.js:264-317) exactly ──────────
    #[test]
    fn encode_request_frame_matches_reference_layout() {
        // A REQUEST for method "echo" with payload "hi".
        let bytes = encode_frame(MessageType::Request, b"echo", b"hi");
        assert_eq!(
            bytes,
            vec![
                MSGPACK_FIXARRAY3, // 0x93
                0x01,              // MSG_REQUEST as positive fixint
                MSGPACK_BIN8,      // name bin header
                0x04,
                b'e',
                b'c',
                b'h',
                b'o',
                MSGPACK_BIN8, // payload bin header
                0x02,
                b'h',
                b'i',
            ]
        );
    }

    #[test]
    fn encode_empty_name_and_payload() {
        let bytes = encode_frame(MessageType::Response, b"", b"");
        assert_eq!(
            bytes,
            vec![
                MSGPACK_FIXARRAY3,
                0x04,
                MSGPACK_BIN8,
                0x00,
                MSGPACK_BIN8,
                0x00
            ]
        );
    }

    #[test]
    fn encode_decode_roundtrip() {
        let payload = br#"{"snapshot":"n1","project":"p.x","file":"/a.ts"}"#;
        let bytes = encode_frame(MessageType::Request, b"getSemanticDiagnostics", payload);
        let (frame, consumed) = decode_frame(&bytes, 0).unwrap();
        assert_eq!(frame.msg_type, MessageType::Request);
        assert_eq!(frame.name, b"getSemanticDiagnostics");
        assert_eq!(frame.payload, payload.as_slice());
        assert_eq!(consumed, bytes.len(), "consumed the whole frame");
    }

    // ── reader accepts the 0xcc+u8 message-type form (syncChannel.js:335) ────
    #[test]
    fn decode_accepts_uint8_message_type_form() {
        // Hand-build a frame whose type byte is written as 0xcc 0x06 (uint8 6)
        // rather than the bare fixint 0x06. The reference reader accepts both.
        let mut bytes = vec![MSGPACK_FIXARRAY3, MSGPACK_UINT8, 0x06];
        bytes.extend_from_slice(&[MSGPACK_BIN8, 0x00]); // empty name
        bytes.extend_from_slice(&[MSGPACK_BIN8, 0x00]); // empty payload
        let (frame, _) = decode_frame(&bytes, 0).unwrap();
        assert_eq!(frame.msg_type, MessageType::Call);
    }

    // ── DISCRIMINATING: non-FIXARRAY3 leading marker is rejected ─────────────
    #[test]
    fn decode_rejects_non_fixarray3_marker() {
        // 0x92 is a 2-element fixarray, not 3.
        let bytes = vec![0x92, 0x01, MSGPACK_BIN8, 0x00, MSGPACK_BIN8, 0x00];
        assert!(
            matches!(decode_frame(&bytes, 0), Err(TsgoApiError::Codec(m)) if m.contains("3-element")),
            "a 2-element array must be rejected as not-a-frame"
        );
    }

    #[test]
    fn decode_rejects_invalid_message_type_in_frame() {
        // FIXARRAY3 then type byte 0x07 (out of the 1..=6 taxonomy).
        let bytes = vec![
            MSGPACK_FIXARRAY3,
            0x07,
            MSGPACK_BIN8,
            0x00,
            MSGPACK_BIN8,
            0x00,
        ];
        assert!(matches!(
            decode_frame(&bytes, 0),
            Err(TsgoApiError::Codec(_))
        ));
    }

    #[test]
    fn decode_rejects_truncated_payload_field() {
        // Valid header through name, payload bin header claims 9 bytes, none present.
        let mut bytes = vec![MSGPACK_FIXARRAY3, 0x04, MSGPACK_BIN8, 0x00];
        bytes.extend_from_slice(&[MSGPACK_BIN8, 0x09]); // claims 9 payload bytes
        assert!(matches!(
            decode_frame(&bytes, 0),
            Err(TsgoApiError::Codec(_))
        ));
    }

    // ── decode at a non-zero offset (multiple frames in one buffer) ──────────
    #[test]
    fn decode_at_offset_reads_second_frame() {
        let f1 = encode_frame(MessageType::Response, b"a", b"1");
        let f2 = encode_frame(MessageType::Error, b"bb", b"22");
        let mut buf = f1.clone();
        buf.extend_from_slice(&f2);
        let (frame, consumed) = decode_frame(&buf, f1.len()).unwrap();
        assert_eq!(frame.msg_type, MessageType::Error);
        assert_eq!(frame.name, b"bb");
        assert_eq!(frame.payload, b"22");
        assert_eq!(consumed, f2.len());
    }
}
