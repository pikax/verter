//! Discriminating behavioral tests for the deny-by-default
//! [`CarrierInjectionChannel`] write gate and the [`LspRelay`] frame relay,
//! over in-memory duplex transports (NON-VACUOUS: real framing, real async
//! I/O, a real recording peer) without a live tsgo process or editor.

use super::*;

use std::sync::Mutex as TestMutex;
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::jsonrpc::framing::{encode_message, MessageFramer};
use crate::jsonrpc::JsonRpcConnection;

/// Every full JSON-RPC frame a recording peer observed, in arrival order.
type FrameTrace = Arc<TestMutex<Vec<serde_json::Value>>>;

fn trace_methods(trace: &FrameTrace) -> Vec<String> {
    trace
        .lock()
        .unwrap()
        .iter()
        .filter_map(|f| f.get("method").and_then(|m| m.as_str()).map(str::to_owned))
        .collect()
}

/// Wait (bounded, cooperative) until `predicate` holds over the trace.
async fn wait_for_trace(trace: &FrameTrace, predicate: impl Fn(&[serde_json::Value]) -> bool) {
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if predicate(&trace.lock().unwrap()) {
            return;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "trace predicate not satisfied within the bounded wait; trace: {:?}",
            trace.lock().unwrap()
        );
        tokio::time::sleep(Duration::from_millis(2)).await;
    }
}

/// Spawn a recording fake `tsgo --lsp` server over one end of a duplex: it
/// records EVERY inbound frame and answers each request (`id` + `method`) —
/// `custom/initializeAPISession` with `{ sessionId, pipe }`, everything else
/// with a deterministic `{ "answered": <method> }` result. The task ends on
/// the peer write half's EOF; await the join handle after closing the client
/// side to read a complete trace.
fn spawn_recording_server(
    endpoint: tokio::io::DuplexStream,
) -> (FrameTrace, tokio::task::JoinHandle<()>) {
    let (mut read, mut write) = tokio::io::split(endpoint);
    let trace: FrameTrace = Arc::new(TestMutex::new(Vec::new()));
    let trace_task = Arc::clone(&trace);
    let join = tokio::spawn(async move {
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = match read.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                trace_task.lock().unwrap().push(msg.clone());
                let (Some(id), Some(method)) = (
                    msg.get("id").filter(|v| !v.is_null()).cloned(),
                    msg.get("method").and_then(|m| m.as_str()),
                ) else {
                    continue;
                };
                let result = if method == crate::attach::INITIALIZE_API_SESSION_METHOD {
                    serde_json::json!({ "sessionId": "api-session-2", "pipe": "test-pipe" })
                } else {
                    serde_json::json!({ "answered": method })
                };
                let reply = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result });
                let _ = write.write_all(&encode_message(&reply)).await;
                let _ = write.flush().await;
            }
        }
    });
    (trace, join)
}

/// A connection + recording fake server pair over an in-memory duplex.
fn connection_to_recording_server() -> (JsonRpcConnection, FrameTrace, tokio::task::JoinHandle<()>)
{
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client);
    let (trace, join) = spawn_recording_server(server);
    (JsonRpcConnection::connect(cr, cw), trace, join)
}

// ────────────────────────────────────────────────────────────────────────────
// Deny-by-default: the channel refuses every non-allowlisted write with a
// typed error BEFORE the wire; allowlisted carrier ops pass through exactly.
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn carrier_channel_refuses_exit_shutdown_initialize_and_arbitrary() {
    let (conn, trace, join) = connection_to_recording_server();
    let overlays = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays);

    for method in ["exit", "shutdown", "initialize", "anything/atAll"] {
        let err = channel
            .gated_notify(method, serde_json::json!({}))
            .await
            .expect_err("a non-allowlisted notify must be refused before the wire");
        assert!(
            matches!(err, TsgoApiError::WriteGateDenied { method: ref m } if m == method),
            "the refusal must be the typed WriteGateDenied naming `{method}`; got {err:?}"
        );
    }
    let err = channel
        .gated_request("initialize", serde_json::json!({}))
        .await
        .expect_err("a non-allowlisted request must be refused before the wire");
    assert!(
        matches!(err, TsgoApiError::WriteGateDenied { method: ref m } if m == "initialize"),
        "the request gate must refuse `initialize` with the typed error; got {err:?}"
    );

    // CONTROL (discriminates "gate denied" from "wire dead"): one allowlisted
    // write on the SAME channel does reach the server — through the typed
    // `did_open` (the raw sender refuses the stateful lifecycle).
    channel
        .did_open("file:///ws/ctl.vue.tsx", "typescriptreact", 1, "export {};")
        .await
        .expect("the typed did_open passes the gate");

    conn.close().await.unwrap();
    join.await.unwrap();
    let methods = trace_methods(&trace);
    for denied in ["exit", "shutdown", "initialize", "anything/atAll"] {
        assert!(
            !methods.iter().any(|m| m == denied),
            "the server must NEVER observe the denied method `{denied}` — the \
             gate refuses BEFORE the wire: {methods:?}"
        );
    }
    assert!(
        methods.iter().any(|m| m == "textDocument/didOpen"),
        "the control write proves the wire was live while the gate denied: {methods:?}"
    );
}

#[tokio::test]
async fn carrier_channel_allows_didopen_didchange_didclose_diagnostic_and_apisession() {
    let (conn, trace, join) = connection_to_recording_server();
    let overlays = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays);

    let uri = "file:///ws/C.vue.tsx";
    channel
        .did_open(uri, "typescriptreact", 1, "export {};")
        .await
        .expect("didOpen passes the gate");
    assert!(
        overlays.lock().unwrap().contains(uri),
        "a successful did_open must track the overlay URI for retraction"
    );
    channel
        .did_change(uri, 2, "export const x = 1;")
        .await
        .expect("didChange passes the gate");
    channel
        .sync_overlay(uri)
        .await
        .expect("the sync-barrier request passes the gate");
    let session = channel
        .reinitialize_api_session()
        .await
        .expect("the API-session re-emission passes the gate");
    assert_eq!(session.session_id, "api-session-2");
    assert_eq!(session.pipe, "test-pipe");
    channel
        .did_close(uri)
        .await
        .expect("didClose passes the gate");
    assert!(
        !overlays.lock().unwrap().contains(uri),
        "a successful did_close must retract the overlay URI from tracking"
    );

    conn.close().await.unwrap();
    join.await.unwrap();
    let methods = trace_methods(&trace);
    for (i, expected) in [
        "textDocument/didOpen",
        "textDocument/didChange",
        "textDocument/diagnostic",
        crate::attach::INITIALIZE_API_SESSION_METHOD,
        "textDocument/didClose",
    ]
    .iter()
    .enumerate()
    {
        assert_eq!(
            methods.get(i).map(String::as_str),
            Some(*expected),
            "the server must observe the exact allowlisted methods in send \
             order (transparency of allowed writes): {methods:?}"
        );
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Kind-correctness: an allowlisted method sent as the WRONG JSON-RPC kind is
// refused before the wire (not just name-checked); the correctly-kinded op
// passes. The raw notification sender additionally refuses the stateful
// overlay open/close lifecycle (reachable ONLY through did_open/did_close).
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn carrier_channel_refuses_kind_mismatched_allowlisted_ops() {
    let (conn, trace, join) = connection_to_recording_server();
    let overlays = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays);

    // diagnostic is a REQUEST-only carrier op — sent as a NOTIFICATION it is a
    // kind mismatch and must be refused, though the method name is allowlisted.
    let err = channel
        .gated_notify("textDocument/diagnostic", serde_json::json!({}))
        .await
        .expect_err("an allowlisted REQUEST method sent as a NOTIFICATION must be refused");
    assert!(
        matches!(err, TsgoApiError::WriteGateDenied { method: ref m } if m == "textDocument/diagnostic"),
        "the kind-mismatch refusal must be the typed WriteGateDenied; got {err:?}"
    );

    // custom/initializeAPISession is a REQUEST-only op — as a notification, refused.
    let err = channel
        .gated_notify("custom/initializeAPISession", serde_json::json!({}))
        .await
        .expect_err("initializeAPISession as a notification must be refused");
    assert!(
        matches!(err, TsgoApiError::WriteGateDenied { .. }),
        "got {err:?}"
    );

    // didChange is REQUEST-mismatched (it is notification-only): as a request, refused.
    let err = channel
        .gated_request("textDocument/didChange", serde_json::json!({}))
        .await
        .expect_err("a notification-only method sent as a request must be refused");
    assert!(
        matches!(err, TsgoApiError::WriteGateDenied { method: ref m } if m == "textDocument/didChange"),
        "got {err:?}"
    );

    // CONTROL (discriminates kind-gate-denied from wire-dead): the correctly-
    // kinded ops DO pass and reach the server on the SAME channel.
    channel
        .did_change("file:///ws/K.vue.tsx", 1, "export {};")
        .await
        .expect("didChange as a notification passes the gate");
    channel
        .sync_overlay("file:///ws/K.vue.tsx")
        .await
        .expect("diagnostic as a request passes the gate");

    conn.close().await.unwrap();
    join.await.unwrap();
    let frames = trace.lock().unwrap().clone();
    let frames_for = |method: &str| -> Vec<serde_json::Value> {
        frames
            .iter()
            .filter(|f| f.get("method").and_then(|m| m.as_str()) == Some(method))
            .cloned()
            .collect()
    };
    // Exactly ONE diagnostic frame — the control REQUEST (it carries an `id`).
    // A leaked `gated_notify("textDocument/diagnostic")` would add an id-LESS
    // notification frame, so the count AND the kind discriminate the gate.
    let diagnostics = frames_for("textDocument/diagnostic");
    assert_eq!(
        diagnostics.len(),
        1,
        "exactly one diagnostic frame (the control request) reached the wire — \
         no kind-mismatched diagnostic leaked: {frames:?}"
    );
    assert!(
        diagnostics[0]
            .get("id")
            .map(|v| !v.is_null())
            .unwrap_or(false),
        "the sole diagnostic frame is a REQUEST (carries an id) — a leaked \
         diagnostic-as-notification would be id-less: {:?}",
        diagnostics[0]
    );
    // Exactly ONE didChange frame — the control NOTIFICATION (no `id`). A leaked
    // `gated_request("textDocument/didChange")` would carry an `id`.
    let did_changes = frames_for("textDocument/didChange");
    assert_eq!(
        did_changes.len(),
        1,
        "exactly one didChange frame (the control notification) reached the \
         wire — no kind-mismatched didChange leaked: {frames:?}"
    );
    assert!(
        did_changes[0]
            .get("id")
            .map(|v| v.is_null())
            .unwrap_or(true),
        "the sole didChange frame is a NOTIFICATION (no id) — a leaked \
         didChange-as-request would carry an id: {:?}",
        did_changes[0]
    );
    // The mismatched request-only method sent as a notification never reached.
    assert!(
        frames_for("custom/initializeAPISession").is_empty(),
        "the mismatched initializeAPISession-as-notification never reached the wire: {frames:?}"
    );
}

#[tokio::test]
async fn gated_notify_refuses_overlay_open_close_lifecycle() {
    // The raw notification sender refuses the stateful open/close lifecycle, so
    // overlay open/close rides ONLY did_open/did_close (which thread the
    // open_overlays bookkeeping) — no overlay is ever opened untracked.
    let (conn, trace, join) = connection_to_recording_server();
    let overlays = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays);

    for method in ["textDocument/didOpen", "textDocument/didClose"] {
        let err = channel
            .gated_notify(
                method,
                serde_json::json!({ "textDocument": { "uri": "file:///ws/raw.vue.tsx" } }),
            )
            .await
            .expect_err(
                "the raw notification sender must refuse the stateful open/close lifecycle",
            );
        assert!(
            matches!(err, TsgoApiError::WriteGateDenied { method: ref m } if m == method),
            "got {err:?}"
        );
    }
    assert!(
        overlays.lock().unwrap().is_empty(),
        "a refused raw lifecycle notify must track no overlay"
    );

    // CONTROL: did_open IS the legitimate open path — it opens AND tracks.
    channel
        .did_open("file:///ws/ok.vue.tsx", "typescriptreact", 1, "export {};")
        .await
        .expect("did_open is the legitimate, tracked open path");
    assert!(
        overlays.lock().unwrap().contains("file:///ws/ok.vue.tsx"),
        "did_open tracks the overlay for retraction"
    );

    conn.close().await.unwrap();
    join.await.unwrap();
    let methods = trace_methods(&trace);
    // Only did_open's didOpen reached the wire; no raw didOpen/didClose did.
    assert_eq!(
        methods
            .iter()
            .filter(|m| *m == "textDocument/didOpen")
            .count(),
        1,
        "only the legitimate did_open reached the wire: {methods:?}"
    );
    assert!(
        !methods.iter().any(|m| m == "textDocument/didClose"),
        "no raw didClose reached the wire: {methods:?}"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// The sync barrier reports the truth: a completed round-trip (a success OR a
// JSON-RPC error response) proves the server drained the queued didOpen in
// order; a request that never round-trips (Closed) proves nothing — the
// failure must propagate, or `did_open_synced` reports a synchronization
// that did not happen.
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn sync_overlay_propagates_no_round_trip_failure() {
    // The server side is dropped immediately → EOF on the client read
    // (mirrors `connection_tests::closed_connection_fails_request`).
    let (client, server) = tokio::io::duplex(64 * 1024);
    drop(server);
    let (cr, cw) = tokio::io::split(client);
    let conn = JsonRpcConnection::connect(cr, cw);
    let overlays = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays);

    let err = tokio::time::timeout(
        Duration::from_secs(5),
        channel.sync_overlay("file:///ws/Dead.vue.tsx"),
    )
    .await
    .expect("sync_overlay must resolve on a closed connection")
    .expect_err(
        "sync_overlay over a connection that never round-trips must FAIL — \
         an Ok would report an ordering barrier that never held",
    );
    assert!(
        matches!(err, TsgoApiError::Closed),
        "the no-round-trip failure must surface as the transport-closed \
         error (never swallowed, never a completed-round-trip Transport), \
         got {err:?}"
    );
}

#[tokio::test]
async fn did_open_synced_propagates_no_round_trip_failure() {
    let (client, server) = tokio::io::duplex(64 * 1024);
    drop(server);
    let (cr, cw) = tokio::io::split(client);
    let conn = JsonRpcConnection::connect(cr, cw);
    let overlays = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays);

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        channel.did_open_synced(
            "file:///ws/Dead.vue.tsx",
            "typescriptreact",
            1,
            "export {};",
        ),
    )
    .await
    .expect("did_open_synced must resolve on a closed connection");
    assert!(
        result.is_err(),
        "did_open_synced over a closed connection must NOT report a \
         synchronized open — the barrier never round-tripped"
    );
}

#[tokio::test]
async fn sync_overlay_tolerates_jsonrpc_error_response() {
    // A server WITHOUT pull diagnostics answers the barrier request with a
    // JSON-RPC error response — the round-trip still COMPLETED, so LSP
    // in-order processing consumed the queued didOpen before the answer:
    // the barrier held and sync_overlay returns Ok.
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client);
    let (mut sr, mut sw) = tokio::io::split(server);
    let responder = tokio::spawn(async move {
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = match sr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                let Some(id) = msg.get("id").filter(|v| !v.is_null()).cloned() else {
                    continue;
                };
                let reply = serde_json::json!({
                    "jsonrpc": "2.0", "id": id,
                    "error": { "code": -32601, "message": "method not found" },
                });
                let _ = sw.write_all(&encode_message(&reply)).await;
                let _ = sw.flush().await;
            }
        }
    });

    let conn = JsonRpcConnection::connect(cr, cw);
    let overlays = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays);
    channel
        .sync_overlay("file:///ws/NoPull.vue.tsx")
        .await
        .expect(
            "a completed round-trip answered with a JSON-RPC error is a HELD \
             barrier (the server processed the queue) — sync_overlay returns Ok",
        );
    conn.close().await.unwrap();
    responder.await.unwrap();
}

// ────────────────────────────────────────────────────────────────────────────
// Relay harness: a fake EDITOR endpoint and a fake SERVER endpoint on two
// in-memory duplexes, with the relay pumping between them. The editor side is
// driven directly (raw framed writes) and observed through a collector task.
// ────────────────────────────────────────────────────────────────────────────

/// The relay under test between two duplex endpoints. Returns the EDITOR
/// endpoint halves (write into the relay / read from the relay), the server
/// trace + join handle, and the relay.
fn relay_between_editor_and_server() -> (
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    FrameTrace,
    tokio::task::JoinHandle<()>,
    LspRelay,
) {
    let (editor_endpoint, relay_editor_side) = tokio::io::duplex(64 * 1024);
    let (server_endpoint, relay_server_side) = tokio::io::duplex(64 * 1024);
    let (editor_read_at_relay, editor_write_at_relay) = tokio::io::split(relay_editor_side);
    let (server_read_at_relay, server_write_at_relay) = tokio::io::split(relay_server_side);
    let relay = LspRelay::start(
        editor_read_at_relay,
        editor_write_at_relay,
        server_read_at_relay,
        server_write_at_relay,
    );
    let (editor_read, editor_write) = tokio::io::split(editor_endpoint);
    let (server_trace, server_join) = spawn_recording_server(server_endpoint);
    (editor_write, editor_read, server_trace, server_join, relay)
}

/// Collect every frame the EDITOR endpoint receives from the relay.
fn spawn_editor_collector(
    mut editor_read: tokio::io::ReadHalf<tokio::io::DuplexStream>,
) -> FrameTrace {
    let trace: FrameTrace = Arc::new(TestMutex::new(Vec::new()));
    let trace_task = Arc::clone(&trace);
    tokio::spawn(async move {
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = match editor_read.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                trace_task.lock().unwrap().push(msg);
            }
        }
    });
    trace
}

/// Write one framed JSON-RPC message into a raw endpoint half.
async fn write_frame(
    write: &mut tokio::io::WriteHalf<tokio::io::DuplexStream>,
    msg: &serde_json::Value,
) {
    write.write_all(&encode_message(msg)).await.unwrap();
    write.flush().await.unwrap();
}

/// Read the next framed JSON-RPC message from a raw endpoint half (bounded).
async fn read_frame(read: &mut tokio::io::ReadHalf<tokio::io::DuplexStream>) -> serde_json::Value {
    let mut framer = MessageFramer::new();
    let mut chunk = [0u8; 8192];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Ok(Some(msg)) = framer.next_message() {
            return msg;
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no frame arrived within the bounded wait"
        );
        match tokio::time::timeout(Duration::from_secs(5), read.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => framer.push(&chunk[..n]),
            other => panic!("editor endpoint read failed/EOF while awaiting a frame: {other:?}"),
        }
    }
}

/// Read the next RAW framed message (`Content-Length` header + separator +
/// body, exactly as received) from a raw endpoint half (bounded). Test-local
/// on purpose: byte-fidelity is asserted against bytes read straight off the
/// transport, independent of the production framer.
async fn read_raw_frame(read: &mut tokio::io::ReadHalf<tokio::io::DuplexStream>) -> Vec<u8> {
    let mut buf: Vec<u8> = Vec::new();
    let mut chunk = [0u8; 8192];
    let deadline = tokio::time::Instant::now() + Duration::from_secs(5);
    loop {
        if let Some(sep) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
            let header = std::str::from_utf8(&buf[..sep]).expect("frame header is UTF-8");
            let len: usize = header
                .split("\r\n")
                .find_map(|line| {
                    let lower = line.to_ascii_lowercase();
                    let rest = lower.strip_prefix("content-length:")?;
                    Some(rest.trim().parse().expect("numeric Content-Length"))
                })
                .expect("frame carries Content-Length");
            let end = sep + 4 + len;
            if buf.len() >= end {
                return buf[..end].to_vec();
            }
        }
        assert!(
            tokio::time::Instant::now() < deadline,
            "no complete raw frame arrived within the bounded wait; buffered: {buf:?}"
        );
        match tokio::time::timeout(Duration::from_secs(5), read.read(&mut chunk)).await {
            Ok(Ok(n)) if n > 0 => buf.extend_from_slice(&chunk[..n]),
            other => panic!("endpoint read failed/EOF while awaiting a raw frame: {other:?}"),
        }
    }
}

// ────────────────────────────────────────────────────────────────────────────
// Transparency: editor traffic passes through the relay untouched in BOTH
// directions (no id rewriting, no field mutation, no reordering) — down to
// the exact bytes (original key order + whitespace, never re-encoded).
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn relay_pass_through_is_byte_identical_preserving_key_order() {
    // Raw endpoints on BOTH sides — the test controls and observes the exact
    // bytes crossing the relay.
    let (editor_endpoint, relay_editor_side) = tokio::io::duplex(64 * 1024);
    let (server_endpoint, relay_server_side) = tokio::io::duplex(64 * 1024);
    let (er, ew) = tokio::io::split(relay_editor_side);
    let (sr, sw) = tokio::io::split(relay_server_side);
    let relay = LspRelay::start(er, ew, sr, sw);
    let (mut editor_read, mut editor_write) = tokio::io::split(editor_endpoint);
    let (mut server_read, mut server_write) = tokio::io::split(server_endpoint);

    // Hand-built frames whose object keys are NOT alphabetical and whose
    // whitespace is idiosyncratic: a decode→re-encode would reorder the keys
    // (`alpha` before `zulu`) and recompact the whitespace — byte-faithful
    // pass-through must forward the ORIGINAL bytes.
    let editor_body: &[u8] =
        br#"{"zulu": 1, "method": "textDocument/hover", "jsonrpc": "2.0", "id": 42, "alpha": 2}"#;
    let mut editor_frame = format!("Content-Length: {}\r\n\r\n", editor_body.len()).into_bytes();
    editor_frame.extend_from_slice(editor_body);
    editor_write.write_all(&editor_frame).await.unwrap();
    editor_write.flush().await.unwrap();
    assert_eq!(
        read_raw_frame(&mut server_read).await,
        editor_frame,
        "an editor→server frame must reach the server BYTE-IDENTICAL \
         (original key order + whitespace — no re-encode)"
    );

    let server_body: &[u8] = br#"{"zulu": true, "method": "window/logMessage", "jsonrpc": "2.0", "alpha": {"omega": 1, "beta": 2}}"#;
    let mut server_frame = format!("Content-Length: {}\r\n\r\n", server_body.len()).into_bytes();
    server_frame.extend_from_slice(server_body);
    server_write.write_all(&server_frame).await.unwrap();
    server_write.flush().await.unwrap();
    assert_eq!(
        read_raw_frame(&mut editor_read).await,
        server_frame,
        "a server→editor frame must reach the editor BYTE-IDENTICAL \
         (original key order + whitespace — no re-encode)"
    );
    relay.shutdown().await;
}

#[tokio::test]
async fn relay_passes_editor_request_and_server_response_untouched() {
    let (mut editor_write, mut editor_read, server_trace, _server_join, relay) =
        relay_between_editor_and_server();

    let request = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 42,
        "method": "textDocument/hover",
        "params": { "textDocument": { "uri": "file:///ws/a.ts" },
                    "position": { "line": 3, "character": 7 } },
    });
    write_frame(&mut editor_write, &request).await;

    // The fake server echoes `{ "answered": <method> }` under the SAME id.
    let response = read_frame(&mut editor_read).await;
    assert_eq!(
        response,
        serde_json::json!({
            "jsonrpc": "2.0", "id": 42,
            "result": { "answered": "textDocument/hover" }
        }),
        "the server response must reach the editor UNTOUCHED (same id, same \
         result — pass-through transparency)"
    );
    // The server observed the editor request EXACTLY as sent.
    wait_for_trace(&server_trace, |frames| !frames.is_empty()).await;
    assert_eq!(
        server_trace.lock().unwrap().first(),
        Some(&request),
        "the editor request must reach the server UNTOUCHED"
    );
    relay.shutdown().await;
}

#[tokio::test]
async fn relay_forwards_server_to_client_notifications_untouched() {
    // A bespoke harness with a RAW server endpoint (no recording server) so
    // the test controls the server side frame-for-frame.
    let (editor_endpoint, relay_editor_side) = tokio::io::duplex(64 * 1024);
    let (server_endpoint, relay_server_side) = tokio::io::duplex(64 * 1024);
    let (er, ew) = tokio::io::split(relay_editor_side);
    let (sr, sw) = tokio::io::split(relay_server_side);
    let relay = LspRelay::start(er, ew, sr, sw);
    let (editor_read, _editor_write) = tokio::io::split(editor_endpoint);
    let editor_trace = spawn_editor_collector(editor_read);
    let (_server_read, mut server_write) = tokio::io::split(server_endpoint);

    // A server-originated notification (no id) must pass through UNTOUCHED.
    let notification = serde_json::json!({
        "jsonrpc": "2.0",
        "method": "textDocument/publishDiagnostics",
        "params": { "uri": "file:///ws/a.ts", "diagnostics": [] },
    });
    write_frame(&mut server_write, &notification).await;
    wait_for_trace(&editor_trace, |frames| !frames.is_empty()).await;
    assert_eq!(
        editor_trace.lock().unwrap().first(),
        Some(&notification),
        "a server→client notification must pass through to the editor UNTOUCHED"
    );
    relay.shutdown().await;
}

// ────────────────────────────────────────────────────────────────────────────
// The reserved `verter:*` id namespace: an editor frame carrying a reserved
// id is a violation — dropped and recorded, never forwarded, never misrouted.
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn relay_reserves_verter_namespace_rejects_editor_verter_id() {
    let (mut editor_write, _editor_read, server_trace, _server_join, relay) =
        relay_between_editor_and_server();

    let violating = serde_json::json!({
        "jsonrpc": "2.0",
        "id": "verter:9",
        "method": "textDocument/hover",
        "params": {},
    });
    write_frame(&mut editor_write, &violating).await;
    // A follow-up legitimate frame through the SAME pump: once the server
    // observes it, the earlier violating frame (same ordered pump) was
    // definitively dropped, not delayed.
    let follow_up = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "probe/after",
        "params": {},
    });
    write_frame(&mut editor_write, &follow_up).await;

    wait_for_trace(&server_trace, |frames| {
        frames
            .iter()
            .any(|f| f.get("method").and_then(|m| m.as_str()) == Some("probe/after"))
    })
    .await;
    let frames = server_trace.lock().unwrap().clone();
    assert!(
        !frames.iter().any(|f| {
            f.get("id").and_then(|v| v.as_str()) == Some("verter:9")
                || f.get("method").and_then(|m| m.as_str()) == Some("textDocument/hover")
        }),
        "an editor frame carrying a reserved `verter:*` id must NEVER be \
         forwarded to the server: {frames:?}"
    );
    assert_eq!(
        relay.reservation_violations(),
        1,
        "the dropped reserved-namespace frame must be recorded as a violation"
    );
    relay.shutdown().await;
}

// ────────────────────────────────────────────────────────────────────────────
// Injection + demux: Verter-injected frames ride the serialized server writer;
// responses to `verter:*` requests demux to Verter, never to the editor.
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn relay_injects_didopen_onto_server_stream() {
    let (editor_write, _editor_read, server_trace, _server_join, relay) =
        relay_between_editor_and_server();

    relay
        .injection_channel()
        .did_open("file:///ws/Inj.vue.tsx", "typescriptreact", 1, "export {};")
        .await
        .expect("injected didOpen passes the gate onto the server stream");

    wait_for_trace(&server_trace, |frames| !frames.is_empty()).await;
    let frames = server_trace.lock().unwrap().clone();
    let did_open = frames
        .iter()
        .find(|f| f.get("method").and_then(|m| m.as_str()) == Some("textDocument/didOpen"))
        .expect("the fake server must observe the injected didOpen");
    assert_eq!(
        did_open
            .get("params")
            .and_then(|p| p.get("textDocument"))
            .and_then(|t| t.get("uri"))
            .and_then(|u| u.as_str()),
        Some("file:///ws/Inj.vue.tsx"),
        "the injected didOpen must carry the carrier URI: {did_open:?}"
    );
    drop(editor_write);
    relay.shutdown().await;
}

#[tokio::test]
async fn relay_injected_request_demuxes_to_verter_not_editor() {
    let (editor_write, editor_read, server_trace, _server_join, relay) =
        relay_between_editor_and_server();
    let editor_trace = spawn_editor_collector(editor_read);

    let channel = relay.injection_channel();
    let result = channel
        .gated_request(
            "textDocument/diagnostic",
            serde_json::json!({ "textDocument": { "uri": "file:///ws/Inj.vue.tsx" } }),
        )
        .await
        .expect("the injected allowlisted request must round-trip");
    assert_eq!(
        result,
        serde_json::json!({ "answered": "textDocument/diagnostic" }),
        "the injected request's response must demux back to Verter"
    );
    // The server observed the injected request under a reserved `verter:*` id.
    let frames = server_trace.lock().unwrap().clone();
    let injected = frames
        .iter()
        .find(|f| f.get("method").and_then(|m| m.as_str()) == Some("textDocument/diagnostic"))
        .expect("the server must observe the injected request");
    let injected_id = injected
        .get("id")
        .and_then(|v| v.as_str())
        .expect("the injected request id must be a string");
    assert!(
        injected_id.starts_with("verter:"),
        "injected request ids are minted in the reserved namespace; got {injected_id:?}"
    );

    // Ordering fence: an editor request round-trip AFTER the demuxed
    // response rides the same ordered server→editor pump — once its answer
    // reaches the editor, the earlier `verter:*` response (had it been
    // misrouted) would already have arrived too.
    let mut editor_write = editor_write;
    let fence_req = serde_json::json!({
        "jsonrpc": "2.0", "id": 99, "method": "fence/after", "params": {},
    });
    write_frame(&mut editor_write, &fence_req).await;
    wait_for_trace(&editor_trace, |frames| {
        frames.iter().any(|f| {
            f.get("id").and_then(serde_json::Value::as_i64) == Some(99) && f.get("method").is_none()
        })
    })
    .await;
    let editor_frames = editor_trace.lock().unwrap().clone();
    assert!(
        !editor_frames.iter().any(|f| f
            .get("id")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.starts_with("verter:"))),
        "the editor must NEVER receive a `verter:*` response frame: {editor_frames:?}"
    );
    relay.shutdown().await;
}

/// `custom/initializeAPISession` re-emission rides the SAME gated injection
/// path: the request demuxes to Verter and parses into an [`ApiSessionHandle`].
#[tokio::test]
async fn relay_reemits_api_session_request_and_parses_handle() {
    let (editor_write, _editor_read, server_trace, _server_join, relay) =
        relay_between_editor_and_server();

    let handle = relay
        .injection_channel()
        .reinitialize_api_session()
        .await
        .expect("the API-session re-emission must round-trip over the relay");
    assert_eq!(handle.session_id, "api-session-2");
    assert_eq!(handle.pipe, "test-pipe");

    let frames = server_trace.lock().unwrap().clone();
    let reemit = frames
        .iter()
        .find(|f| {
            f.get("method").and_then(|m| m.as_str())
                == Some(crate::attach::INITIALIZE_API_SESSION_METHOD)
        })
        .expect("the server must observe the re-emitted API-session request");
    assert!(
        reemit
            .get("id")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.starts_with("verter:")),
        "the re-emitted request rides a reserved-namespace id: {reemit:?}"
    );
    drop(editor_write);
    relay.shutdown().await;
}

// ────────────────────────────────────────────────────────────────────────────
// Ordering: the injected didOpen is observed on the ordered server wire
// BEFORE the sync-barrier request (the overlay is registered before any
// `--api` updateSnapshot could enumerate roots).
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn injected_didopen_precedes_sync_barrier() {
    let (editor_write, _editor_read, server_trace, _server_join, relay) =
        relay_between_editor_and_server();

    relay
        .injection_channel()
        .did_open_synced("file:///ws/Ord.vue.tsx", "typescriptreact", 1, "export {};")
        .await
        .expect("did_open_synced over the relay");

    let methods: Vec<String> = server_trace
        .lock()
        .unwrap()
        .iter()
        .filter_map(|f| f.get("method").and_then(|m| m.as_str()).map(str::to_owned))
        .collect();
    let open_at = methods
        .iter()
        .position(|m| m == "textDocument/didOpen")
        .expect("the server observes the injected didOpen");
    let barrier_at = methods
        .iter()
        .position(|m| m == "textDocument/diagnostic")
        .expect("the server observes the sync-barrier request");
    assert!(
        open_at < barrier_at,
        "the injected didOpen must be observed BEFORE the sync barrier on the \
         ordered server wire: {methods:?}"
    );
    drop(editor_write);
    relay.shutdown().await;
}
