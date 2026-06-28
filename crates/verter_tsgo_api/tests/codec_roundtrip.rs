//! Codec correctness test against RECORDED tsgo `--api` wire frames.
//!
//! This is the core correctness rail for the hand-written codec. The fixtures
//! in `tests/fixtures/wire-frames.json` are captured by
//! `tests/js/capture-fixtures.mjs`:
//!   - `requestFrames` are the exact bytes the official JS channel's
//!     `writeTuple` emits for each op's request (the WRITE-side ground truth);
//!   - `liveFrames` are the genuine bytes the real tsgo engine wrote back on
//!     the pipe (the READ-side ground truth — not a JS re-encode).
//!
//! The test asserts the Rust codec produces byte-identical request frames and
//! decodes both kinds back to the expected typed values. A wrong field order,
//! marker, or length fails here.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::Deserialize;
use verter_tsgo_api::proto::frame::{decode_frame, encode_frame, MessageType};
use verter_tsgo_api::proto::types::InitializeResponse;

#[derive(Debug, Deserialize)]
struct WireFrames {
    #[serde(rename = "engineVersion")]
    engine_version: String,
    #[serde(rename = "requestFrames")]
    request_frames: BTreeMap<String, RequestFrame>,
    #[serde(rename = "liveFrames")]
    live_frames: BTreeMap<String, serde_json::Value>,
}

#[derive(Debug, Deserialize)]
struct RequestFrame {
    method: String,
    payload: String,
    hex: String,
}

fn fixtures_path() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests")
        .join("fixtures")
        .join("wire-frames.json")
}

fn load_fixtures() -> WireFrames {
    let raw = std::fs::read_to_string(fixtures_path())
        .expect("wire-frames.json fixture must exist (run tests/js/capture-fixtures.mjs)");
    serde_json::from_str(&raw).expect("wire-frames.json must be valid JSON")
}

fn hex_to_bytes(hex: &str) -> Vec<u8> {
    assert!(
        hex.len().is_multiple_of(2),
        "hex string must have even length"
    );
    (0..hex.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&hex[i..i + 2], 16).expect("valid hex byte"))
        .collect()
}

#[test]
fn fixtures_pin_the_expected_engine_version() {
    // Guards that the recorded fixtures came from the pinned engine. If the
    // engine is bumped, the fixtures must be re-captured (and the codec
    // re-verified) — this assertion surfaces a stale-fixture mismatch.
    let fx = load_fixtures();
    assert_eq!(
        fx.engine_version, "7.0.1-rc",
        "fixture engine version drifted; re-capture fixtures and re-verify the codec"
    );
    assert!(
        !fx.request_frames.is_empty(),
        "request-frame fixtures must be present"
    );
}

#[test]
fn rust_encode_matches_recorded_request_frames_byte_for_byte() {
    let fx = load_fixtures();
    let mut checked = 0usize;
    for (name, rf) in &fx.request_frames {
        let recorded = hex_to_bytes(&rf.hex);
        let produced = encode_frame(
            MessageType::Request,
            rf.method.as_bytes(),
            rf.payload.as_bytes(),
        );
        assert_eq!(
            produced, recorded,
            "encode_frame for `{name}` ({}) does not match the recorded JS-channel bytes",
            rf.method
        );
        checked += 1;
    }
    // Must cover each op the codec hand-writes (initialize, updateSnapshot x2,
    // semantic diags, type/symbol at position, typeToString, echo).
    assert!(
        checked >= 8,
        "expected at least 8 request-frame fixtures, checked {checked}"
    );
}

#[test]
fn rust_decode_recovers_typed_request_frames() {
    let fx = load_fixtures();
    for (name, rf) in &fx.request_frames {
        let recorded = hex_to_bytes(&rf.hex);
        let (frame, consumed) = decode_frame(&recorded, 0)
            .unwrap_or_else(|e| panic!("decode_frame failed for `{name}`: {e}"));
        assert_eq!(frame.msg_type, MessageType::Request, "frame `{name}` type");
        assert_eq!(
            frame.name,
            rf.method.as_bytes(),
            "frame `{name}` method name"
        );
        assert_eq!(
            frame.payload,
            rf.payload.as_bytes(),
            "frame `{name}` payload"
        );
        assert_eq!(
            consumed,
            recorded.len(),
            "frame `{name}` consumed all bytes"
        );

        // The payload must be valid JSON (every high-level op carries JSON).
        if rf.method != "echo" {
            serde_json::from_slice::<serde_json::Value>(frame.payload)
                .unwrap_or_else(|e| panic!("payload of `{name}` is not valid JSON: {e}"));
        }
    }
}

#[test]
fn rust_decodes_real_engine_initialize_response() {
    let fx = load_fixtures();
    let live = fx
        .live_frames
        .get("initialize_response")
        .and_then(|v| v.get("hex"))
        .and_then(|v| v.as_str());

    // The fixture was captured against the engine present in this worktree. If
    // the capture failed (no-engine environment), the harness records a
    // `__capture_error` instead; only a genuinely captured frame is asserted.
    let Some(hex) = live else {
        // Surface the recorded capture error so a missing live frame is visible.
        let err = fx
            .live_frames
            .get("__capture_error")
            .map(|v| v.to_string())
            .unwrap_or_else(|| "no initialize_response and no __capture_error".to_string());
        panic!("live initialize_response frame absent — capture error was: {err}");
    };

    let bytes = hex_to_bytes(hex);
    let (frame, consumed) = decode_frame(&bytes, 0).expect("decode real engine response");
    assert_eq!(
        frame.msg_type,
        MessageType::Response,
        "engine reply is a RESPONSE frame"
    );
    assert_eq!(
        frame.name, b"initialize",
        "response name echoes the request method"
    );
    assert_eq!(consumed, bytes.len(), "consumed the full engine frame");

    // The payload deserializes into the typed InitializeResponse.
    let resp: InitializeResponse =
        serde_json::from_slice(frame.payload).expect("payload is an InitializeResponse");
    // currentDirectory is the worktree path the engine reported; it is non-empty.
    assert!(
        !resp.current_directory.is_empty(),
        "engine reported a current directory"
    );

    // NEGATIVE: the response is NOT a Request/Call/Error frame.
    assert_ne!(frame.msg_type, MessageType::Request);
    assert_ne!(frame.msg_type, MessageType::Call);
    assert_ne!(frame.msg_type, MessageType::Error);
}
