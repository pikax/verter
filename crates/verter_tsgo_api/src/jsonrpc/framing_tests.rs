//! Discriminating tests for the vscode-jsonrpc framing codec.

use super::*;
use serde_json::json;

#[test]
fn encode_message_emits_content_length_then_body() {
    let value = json!({ "jsonrpc": "2.0", "id": 1, "method": "initialize", "params": null });
    let body = serde_json::to_vec(&value).unwrap();
    let framed = encode_message(&value);

    let text = String::from_utf8(framed.clone()).unwrap();
    let expected_header = format!("Content-Length: {}\r\n\r\n", body.len());
    assert!(
        text.starts_with(&expected_header),
        "frame must start with the exact Content-Length header: got {text:?}"
    );
    // The body bytes follow the header verbatim.
    assert_eq!(
        &framed[expected_header.len()..],
        body.as_slice(),
        "the framed body must be the verbatim JSON bytes"
    );
}

#[test]
fn roundtrip_single_message() {
    let value = json!({ "jsonrpc": "2.0", "id": 7, "result": { "ok": true } });
    let framed = encode_message(&value);

    let mut framer = MessageFramer::new();
    framer.push(&framed);
    let decoded = framer
        .next_message()
        .expect("decode ok")
        .expect("a message");
    assert_eq!(decoded, value);
    // Buffer drained: no second message.
    assert!(framer.next_message().expect("ok").is_none());
}

#[test]
fn decodes_two_concatenated_messages() {
    let a = json!({ "jsonrpc": "2.0", "id": 1, "result": 1 });
    let b = json!({ "jsonrpc": "2.0", "id": 2, "result": 2 });
    let mut bytes = encode_message(&a);
    bytes.extend_from_slice(&encode_message(&b));

    let mut framer = MessageFramer::new();
    framer.push(&bytes);
    assert_eq!(framer.next_message().unwrap().unwrap(), a);
    assert_eq!(framer.next_message().unwrap().unwrap(), b);
    assert!(framer.next_message().unwrap().is_none());
}

#[test]
fn partial_frame_yields_none_until_complete() {
    let value = json!({ "jsonrpc": "2.0", "id": 9, "result": { "currentDirectory": "/x" } });
    let framed = encode_message(&value);
    // Split mid-body.
    let split = framed.len() - 5;

    let mut framer = MessageFramer::new();
    framer.push(&framed[..split]);
    assert!(
        framer.next_message().expect("ok").is_none(),
        "an incomplete body must yield None, not an error or a truncated parse"
    );
    framer.push(&framed[split..]);
    assert_eq!(framer.next_message().unwrap().unwrap(), value);
}

#[test]
fn header_split_across_pushes_yields_none_then_message() {
    let value = json!({ "jsonrpc": "2.0", "id": 3, "result": true });
    let framed = encode_message(&value);
    // Split inside the header (before the separator).
    let mut framer = MessageFramer::new();
    framer.push(&framed[..5]);
    assert!(framer.next_message().unwrap().is_none());
    framer.push(&framed[5..]);
    assert_eq!(framer.next_message().unwrap().unwrap(), value);
}

#[test]
fn extra_headers_are_tolerated() {
    // vscode-jsonrpc may emit a Content-Type line; the reader keys on
    // Content-Length case-insensitively and ignores the rest.
    let body = br#"{"jsonrpc":"2.0","id":1,"result":42}"#;
    let mut frame = Vec::new();
    frame.extend_from_slice(
        format!(
            "Content-Type: application/vscode-jsonrpc; charset=utf-8\r\nContent-Length: {}\r\n\r\n",
            body.len()
        )
        .as_bytes(),
    );
    frame.extend_from_slice(body);

    let mut framer = MessageFramer::new();
    framer.push(&frame);
    let decoded = framer.next_message().unwrap().unwrap();
    assert_eq!(decoded["result"], json!(42));
}

#[test]
fn next_frame_yields_parsed_value_and_exact_raw_bytes() {
    // A hand-built frame with NON-alphabetical keys and extra whitespace —
    // re-encoding the parsed value could not reproduce these bytes.
    let body: &[u8] = br#"{ "zulu": 1,  "alpha": 2 }"#;
    let mut frame = format!("Content-Length: {}\r\n\r\n", body.len()).into_bytes();
    frame.extend_from_slice(body);

    let mut framer = MessageFramer::new();
    framer.push(&frame);
    let (value, raw) = framer
        .next_frame()
        .expect("decode ok")
        .expect("a complete frame");
    assert_eq!(
        raw, frame,
        "the raw bytes must be the EXACT received frame (header + separator + body)"
    );
    assert_eq!(value, json!({ "zulu": 1, "alpha": 2 }));
    assert_ne!(
        encode_message(&value),
        frame,
        "the discriminating premise: a re-encode of the parsed value does \
         NOT reproduce the original bytes (key order + whitespace differ)"
    );
    assert!(
        framer.next_frame().expect("ok").is_none(),
        "the frame must be drained exactly once"
    );
}

#[test]
fn next_frame_drains_exact_frame_boundaries_across_concatenated_frames() {
    let a_body: &[u8] = br#"{"zulu":1,"alpha":2}"#;
    let mut bytes = format!("Content-Length: {}\r\n\r\n", a_body.len()).into_bytes();
    bytes.extend_from_slice(a_body);
    let a_frame_len = bytes.len();
    let b = json!({ "jsonrpc": "2.0", "id": 2, "result": 2 });
    let b_frame = encode_message(&b);
    bytes.extend_from_slice(&b_frame);

    let mut framer = MessageFramer::new();
    framer.push(&bytes);
    let (a_value, a_raw) = framer.next_frame().unwrap().unwrap();
    assert_eq!(a_value, json!({ "zulu": 1, "alpha": 2 }));
    assert_eq!(
        a_raw,
        &bytes[..a_frame_len],
        "the first raw frame must stop exactly at its frame boundary"
    );
    let (b_value, b_raw) = framer.next_frame().unwrap().unwrap();
    assert_eq!(b_value, b);
    assert_eq!(b_raw, b_frame);
    assert!(framer.next_frame().unwrap().is_none());
}

// ── DISCRIMINATING negative assertions ──

#[test]
fn missing_content_length_is_a_hard_error() {
    let body = br#"{"jsonrpc":"2.0","id":1,"result":1}"#;
    let mut frame = Vec::new();
    // A header with NO Content-Length field, then the separator + body.
    frame.extend_from_slice(b"Content-Type: text/plain\r\n\r\n");
    frame.extend_from_slice(body);

    let mut framer = MessageFramer::new();
    framer.push(&frame);
    let err = framer
        .next_message()
        .expect_err("missing Content-Length must error");
    assert!(
        matches!(err, TsgoApiError::Codec(_)),
        "missing Content-Length must be a Codec error, got {err:?}"
    );
}

#[test]
fn non_numeric_content_length_is_a_hard_error() {
    let mut frame = Vec::new();
    frame.extend_from_slice(b"Content-Length: notanumber\r\n\r\n{}");

    let mut framer = MessageFramer::new();
    framer.push(&frame);
    let err = framer
        .next_message()
        .expect_err("non-numeric Content-Length must error");
    assert!(
        matches!(err, TsgoApiError::Codec(_)),
        "non-numeric Content-Length must be a Codec error, got {err:?}"
    );
}

#[test]
fn malformed_json_body_is_a_hard_error() {
    // A valid header but a body that is not valid JSON.
    let body = b"{not json";
    let mut frame = Vec::new();
    frame.extend_from_slice(format!("Content-Length: {}\r\n\r\n", body.len()).as_bytes());
    frame.extend_from_slice(body);

    let mut framer = MessageFramer::new();
    framer.push(&frame);
    let err = framer.next_message().expect_err("bad JSON body must error");
    assert!(
        matches!(err, TsgoApiError::Json(_)),
        "malformed JSON body must be a Json error, got {err:?}"
    );
}

#[test]
fn base64_data_roundtrip() {
    let bytes = vec![0u8, 1, 2, 250, 251, 255, 42, 7];
    let encoded = encode_base64_data(&bytes);
    let value = json!({ "data": encoded });
    let decoded = decode_base64_data(&value).expect("decode ok");
    assert_eq!(decoded, bytes, "base64 {{data}} must round-trip the bytes");
}

#[test]
fn base64_data_missing_field_errors() {
    let value = json!({ "notdata": "x" });
    let err = decode_base64_data(&value).expect_err("missing data must error");
    assert!(matches!(err, TsgoApiError::Codec(_)));
}

#[test]
fn base64_data_invalid_base64_errors() {
    // `!!!!` is not valid base64.
    let value = json!({ "data": "!!!!" });
    let err = decode_base64_data(&value).expect_err("invalid base64 must error");
    assert!(matches!(err, TsgoApiError::Codec(_)));
}
