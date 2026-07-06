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
    let taint = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays, &taint);

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
    let taint = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays, &taint);

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
    let taint = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays, &taint);

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
    let taint = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays, &taint);

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
    let taint = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays, &taint);

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
    let taint = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays, &taint);

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
    let taint = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays, &taint);
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

/// FAIL-CLOSED: a slow/broken editor tsgo that RECEIVES the barrier
/// request but NEVER answers (the connection stays OPEN — not an EOF/Closed) must not
/// stall the carrier lifecycle. Bounded by its timeout, `sync_overlay` fails CLOSED with
/// `TsgoApiError::Timeout` within the bound rather than blocking forever.
#[tokio::test]
async fn sync_overlay_times_out_when_barrier_never_answers() {
    // A black-hole server: it drains inbound bytes and NEVER responds, holding its
    // write half so the client never sees EOF (distinct from the Closed case).
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (mut sr, sw) = tokio::io::split(server);
    let black_hole = tokio::spawn(async move {
        let _keep_write_open = sw; // never respond; never EOF the client
        let mut chunk = [0u8; 8192];
        loop {
            match sr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(_) => { /* drain, never answer */ }
            }
        }
    });

    let (cr, cw) = tokio::io::split(client);
    let conn = JsonRpcConnection::connect(cr, cw);
    let overlays = StdMutex::new(HashSet::new());
    let taint = StdMutex::new(HashSet::new());
    let channel = CarrierInjectionChannel::new(&conn, &overlays, &taint);

    let started = tokio::time::Instant::now();
    // The OUTER guard proves the barrier does not hang (a pre-timeout sync_overlay would
    // block until this 5s guard fired); the INNER 50ms bound is what must actually fire.
    let err = tokio::time::timeout(
        Duration::from_secs(5),
        channel.sync_overlay_with_timeout("file:///ws/Slow.vue.tsx", Duration::from_millis(50)),
    )
    .await
    .expect("the bounded barrier must resolve well within the outer guard — never hang")
    .expect_err("a barrier that never round-trips within its bound must FAIL, not return Ok");
    assert!(
        matches!(err, TsgoApiError::Timeout(_)),
        "the fail-closed barrier must surface TsgoApiError::Timeout, got {err:?}"
    );
    assert!(
        started.elapsed() < Duration::from_secs(2),
        "the barrier must fail closed within its small bound, not the outer guard"
    );

    conn.close().await.unwrap();
    let _ = black_hole.await;
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

// ────────────────────────────────────────────────────────────────────────────
// Egress: the server→editor pump runs the deny-by-default carrier egress
// policy AFTER the `verter:*` demux and BEFORE the raw forward. Carrier
// authority is the relay's own overlay tracker (the URIs an injected
// did_open recorded). Carrier-free frames still pass byte-identical.
// ────────────────────────────────────────────────────────────────────────────

/// The carrier overlay URI the egress wiring tests open through the relay's
/// injection channel.
const EGRESS_CARRIER: &str = "file:///ws/App.vue.tsx";

/// Relay harness with a COLLECTED editor endpoint and RAW server endpoint
/// halves the test drives frame-for-frame (the egress tests push arbitrary
/// server→editor frames while observing exactly what the editor receives).
fn relay_with_collected_editor_and_raw_server() -> (
    FrameTrace,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
    tokio::io::ReadHalf<tokio::io::DuplexStream>,
    tokio::io::WriteHalf<tokio::io::DuplexStream>,
    LspRelay,
) {
    let (editor_endpoint, relay_editor_side) = tokio::io::duplex(64 * 1024);
    let (server_endpoint, relay_server_side) = tokio::io::duplex(64 * 1024);
    let (er, ew) = tokio::io::split(relay_editor_side);
    let (sr, sw) = tokio::io::split(relay_server_side);
    let relay = LspRelay::start(er, ew, sr, sw);
    let (editor_read, editor_write) = tokio::io::split(editor_endpoint);
    let editor_trace = spawn_editor_collector(editor_read);
    let (server_read, server_write) = tokio::io::split(server_endpoint);
    (editor_trace, editor_write, server_read, server_write, relay)
}

#[tokio::test]
async fn relay_suppresses_carrier_publish_diagnostics_for_open_overlay() {
    let (editor_trace, _editor_write, _server_read, mut server_write, relay) =
        relay_with_collected_editor_and_raw_server();
    relay
        .injection_channel()
        .did_open(EGRESS_CARRIER, "typescriptreact", 1, "export {};")
        .await
        .expect("the injected didOpen tracks the carrier overlay");

    // The server pushes diagnostics for the CARRIER — a leak if forwarded.
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": EGRESS_CARRIER,
                "diagnostics": [{ "message": "carrier-internal" }],
            },
        }),
    )
    .await;
    // CONTROL, sent AFTER: a carrier-free notification on the SAME ordered
    // pump — once it arrives, the earlier diagnostics frame was definitively
    // dropped (not merely delayed), and the wire is proven live.
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "window/logMessage",
            "params": { "type": 3, "message": "control" },
        }),
    )
    .await;
    wait_for_trace(&editor_trace, |frames| {
        frames
            .iter()
            .any(|f| f.get("method").and_then(|m| m.as_str()) == Some("window/logMessage"))
    })
    .await;

    let frames = editor_trace.lock().unwrap().clone();
    assert!(
        !frames.iter().any(|f| {
            f.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
        }),
        "the editor must NEVER observe diagnostics for a carrier overlay: {frames:?}"
    );
    assert!(
        !frames
            .iter()
            .any(|f| serde_json::to_string(f).unwrap().contains(EGRESS_CARRIER)),
        "no forwarded frame may reference the carrier URI: {frames:?}"
    );
    assert_eq!(
        relay.suppressed_egress(),
        1,
        "the dropped carrier frame must be recorded on the egress counter"
    );
    relay.shutdown().await;
}

#[tokio::test]
async fn relay_suppresses_in_flight_carrier_frame_after_did_close() {
    let (editor_trace, _editor_write, _server_read, mut server_write, relay) =
        relay_with_collected_editor_and_raw_server();
    let channel = relay.injection_channel();
    channel
        .did_open(EGRESS_CARRIER, "typescriptreact", 1, "export {};")
        .await
        .expect("the injected didOpen taints the carrier URI");
    channel
        .did_close(EGRESS_CARRIER)
        .await
        .expect("the injected didClose retracts the overlay");

    // An IN-FLIGHT carrier frame the server emits about the just-closed
    // overlay (e.g. diagnostics already queued when the didClose raced past
    // the pump): egress taint is MONOTONIC, so the frame must still be
    // suppressed — a retraction never fails open.
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "textDocument/publishDiagnostics",
            "params": {
                "uri": EGRESS_CARRIER,
                "diagnostics": [{ "message": "carrier-internal, post-close" }],
            },
        }),
    )
    .await;
    // CONTROL, sent AFTER: a carrier-FREE notification on the SAME ordered
    // pump — once it arrives, the earlier carrier frame was definitively
    // dropped (not merely delayed), and the wire is proven live (the taint
    // suppresses exactly carrier frames, not everything).
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "window/logMessage",
            "params": { "type": 3, "message": "control-after-close" },
        }),
    )
    .await;
    wait_for_trace(&editor_trace, |frames| {
        frames
            .iter()
            .any(|f| f.get("method").and_then(|m| m.as_str()) == Some("window/logMessage"))
    })
    .await;

    let frames = editor_trace.lock().unwrap().clone();
    assert!(
        !frames.iter().any(|f| {
            f.get("method").and_then(|m| m.as_str()) == Some("textDocument/publishDiagnostics")
        }),
        "an in-flight carrier frame emitted AFTER didClose must still be \
         suppressed — the egress taint is monotonic, never removed on \
         retraction: {frames:?}"
    );
    assert!(
        !frames
            .iter()
            .any(|f| serde_json::to_string(f).unwrap().contains(EGRESS_CARRIER)),
        "no forwarded frame may reference the closed carrier URI: {frames:?}"
    );
    assert_eq!(
        frames.len(),
        1,
        "the editor receives ONLY the carrier-free control frame: {frames:?}"
    );
    assert_eq!(
        relay.suppressed_egress(),
        1,
        "the post-close carrier frame must be recorded on the egress counter"
    );
    relay.shutdown().await;
}

#[tokio::test]
async fn relay_strips_carrier_entries_from_mixed_workspace_symbol_response() {
    let (editor_trace, _editor_write, _server_read, mut server_write, relay) =
        relay_with_collected_editor_and_raw_server();
    relay
        .injection_channel()
        .did_open(EGRESS_CARRIER, "typescriptreact", 1, "export {};")
        .await
        .expect("the injected didOpen tracks the carrier overlay");

    // A workspace/symbol RESPONSE mixing a carrier symbol with a user
    // symbol: the editor must receive the frame with ONLY the user symbol.
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 41,
            "result": [
                { "name": "CarrierOnlySymbol", "kind": 12,
                  "location": { "uri": EGRESS_CARRIER,
                                "range": { "start": { "line": 0, "character": 0 },
                                           "end": { "line": 0, "character": 1 } } } },
                { "name": "UserSymbol", "kind": 12,
                  "location": { "uri": "file:///ws/user.ts",
                                "range": { "start": { "line": 0, "character": 0 },
                                           "end": { "line": 0, "character": 1 } } } },
            ],
        }),
    )
    .await;
    wait_for_trace(&editor_trace, |frames| {
        frames
            .iter()
            .any(|f| f.get("id").and_then(serde_json::Value::as_i64) == Some(41))
    })
    .await;

    let frames = editor_trace.lock().unwrap().clone();
    let received = frames
        .iter()
        .find(|f| f.get("id").and_then(serde_json::Value::as_i64) == Some(41))
        .expect("the (filtered) response must still reach the editor");
    let text = serde_json::to_string(received).unwrap();
    assert!(
        !text.contains(EGRESS_CARRIER),
        "the carrier URI must be ABSENT from the delivered response: {text}"
    );
    assert!(
        !text.contains("CarrierOnlySymbol"),
        "the carrier symbol entry must be ABSENT whole: {text}"
    );
    assert!(
        text.contains("UserSymbol"),
        "the USER symbol must SURVIVE (per-entry filter, not a whole-frame \
         drop): {text}"
    );
    assert_eq!(
        received["result"].as_array().map(|a| a.len()),
        Some(1),
        "exactly the carrier entry was stripped: {received:?}"
    );
    relay.shutdown().await;
}

#[tokio::test]
async fn relay_forwards_carrier_free_frame_byte_identical_while_overlay_open() {
    // CONTROL: with a carrier overlay OPEN, a carrier-free server frame with
    // idiosyncratic key order + whitespace still reaches the editor
    // BYTE-IDENTICAL — the egress policy re-encodes only carrier-contaminated
    // frames (discriminates "suppressed/re-encoded" from "wire dead").
    let (editor_endpoint, relay_editor_side) = tokio::io::duplex(64 * 1024);
    let (server_endpoint, relay_server_side) = tokio::io::duplex(64 * 1024);
    let (er, ew) = tokio::io::split(relay_editor_side);
    let (sr, sw) = tokio::io::split(relay_server_side);
    let relay = LspRelay::start(er, ew, sr, sw);
    let (mut editor_read, _editor_write) = tokio::io::split(editor_endpoint);
    let (_server_read, mut server_write) = tokio::io::split(server_endpoint);

    relay
        .injection_channel()
        .did_open(EGRESS_CARRIER, "typescriptreact", 1, "export {};")
        .await
        .expect("the injected didOpen tracks the carrier overlay");

    let server_body: &[u8] = br#"{"zulu": true, "method": "window/logMessage", "jsonrpc": "2.0", "params": {"omega": 1, "beta": 2}}"#;
    let mut server_frame = format!("Content-Length: {}\r\n\r\n", server_body.len()).into_bytes();
    server_frame.extend_from_slice(server_body);
    server_write.write_all(&server_frame).await.unwrap();
    server_write.flush().await.unwrap();
    assert_eq!(
        read_raw_frame(&mut editor_read).await,
        server_frame,
        "a carrier-FREE server→editor frame stays BYTE-IDENTICAL even while \
         a carrier overlay is open (original key order + whitespace)"
    );
    assert_eq!(
        relay.suppressed_egress(),
        0,
        "nothing was suppressed on the carrier-free path"
    );
    relay.shutdown().await;
}

#[tokio::test]
async fn relay_answers_all_carrier_apply_edit_to_server_never_editor() {
    let (editor_trace, _editor_write, server_read, mut server_write, relay) =
        relay_with_collected_editor_and_raw_server();
    // The collector is a generic frame recorder: over the server endpoint's
    // read half it records every frame the RELAY writes toward the server.
    let server_inbound = spawn_editor_collector(server_read);

    relay
        .injection_channel()
        .did_open(EGRESS_CARRIER, "typescriptreact", 1, "export {};")
        .await
        .expect("the injected didOpen tracks the carrier overlay");

    // The server asks the editor to apply an ALL-carrier edit: forwarding
    // would leak the carrier, dropping would leave the server waiting
    // forever — the relay must answer the SERVER `{applied:false}` under
    // the ORIGINAL id, and the editor must receive nothing of it.
    let mut only_carrier = serde_json::Map::new();
    only_carrier.insert(
        EGRESS_CARRIER.to_string(),
        serde_json::json!([{
            "range": { "start": { "line": 0, "character": 0 },
                       "end": { "line": 0, "character": 1 } },
            "newText": "x",
        }]),
    );
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": "apply-1",
            "method": "workspace/applyEdit",
            "params": { "edit": { "changes": only_carrier } },
        }),
    )
    .await;

    // The SERVER receives the synthesized negative response, matching id.
    wait_for_trace(&server_inbound, |frames| {
        frames.iter().any(|f| {
            f.get("id").and_then(|v| v.as_str()) == Some("apply-1") && f.get("method").is_none()
        })
    })
    .await;
    let inbound = server_inbound.lock().unwrap().clone();
    let answer = inbound
        .iter()
        .find(|f| {
            f.get("id").and_then(|v| v.as_str()) == Some("apply-1") && f.get("method").is_none()
        })
        .expect("the server observes the synthesized response")
        .clone();
    assert_eq!(
        answer["result"],
        serde_json::json!({ "applied": false }),
        "the relay answers the suppressed applyEdit on the editor's behalf \
         with the negative ApplyWorkspaceEditResult: {answer}"
    );

    // CONTROL on the ordered server→editor pump: once a LATER carrier-free
    // frame reaches the editor, the earlier applyEdit was definitively
    // withheld (not merely delayed) and the wire is proven live.
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "window/logMessage",
            "params": { "type": 3, "message": "control" },
        }),
    )
    .await;
    wait_for_trace(&editor_trace, |frames| {
        frames
            .iter()
            .any(|f| f.get("method").and_then(|m| m.as_str()) == Some("window/logMessage"))
    })
    .await;
    let editor_frames = editor_trace.lock().unwrap().clone();
    assert_eq!(
        editor_frames.len(),
        1,
        "the editor receives ONLY the control frame — nothing of the \
         all-carrier applyEdit: {editor_frames:?}"
    );
    assert!(
        !editor_frames
            .iter()
            .any(|f| serde_json::to_string(f).unwrap().contains(EGRESS_CARRIER)),
        "no editor-bound frame may reference the carrier URI: {editor_frames:?}"
    );
    assert_eq!(
        relay.suppressed_egress(),
        1,
        "the answered server request counts as not-forwarded-to-editor"
    );
    relay.shutdown().await;
}

#[tokio::test]
async fn relay_answers_mixed_apply_edit_to_server_never_editor() {
    let (editor_trace, _editor_write, server_read, mut server_write, relay) =
        relay_with_collected_editor_and_raw_server();
    // The collector is a generic frame recorder: over the server endpoint's
    // read half it records every frame the RELAY writes toward the server.
    let server_inbound = spawn_editor_collector(server_read);

    relay
        .injection_channel()
        .did_open(EGRESS_CARRIER, "typescriptreact", 1, "export {};")
        .await
        .expect("the injected didOpen taints the carrier overlay");

    // The server asks the editor to apply a MIXED (carrier+user) edit: a
    // filtered forward would be a partial-apply lie (the editor answers
    // `applied:true` while the carrier part was silently dropped), and a
    // raw forward would leak — the relay must answer the SERVER
    // `{applied:false}` under the ORIGINAL id, and the editor must receive
    // NOTHING of it (the user remainder is NOT delivered).
    let mut mixed_changes = serde_json::Map::new();
    mixed_changes.insert(
        EGRESS_CARRIER.to_string(),
        serde_json::json!([{
            "range": { "start": { "line": 0, "character": 0 },
                       "end": { "line": 0, "character": 1 } },
            "newText": "x",
        }]),
    );
    mixed_changes.insert(
        "file:///ws/user.ts".to_string(),
        serde_json::json!([{
            "range": { "start": { "line": 0, "character": 0 },
                       "end": { "line": 0, "character": 1 } },
            "newText": "y",
        }]),
    );
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": "apply-mixed-1",
            "method": "workspace/applyEdit",
            "params": { "label": "refactor", "edit": { "changes": mixed_changes } },
        }),
    )
    .await;

    // The SERVER receives the synthesized negative response, matching id.
    wait_for_trace(&server_inbound, |frames| {
        frames.iter().any(|f| {
            f.get("id").and_then(|v| v.as_str()) == Some("apply-mixed-1")
                && f.get("method").is_none()
        })
    })
    .await;
    let inbound = server_inbound.lock().unwrap().clone();
    let answer = inbound
        .iter()
        .find(|f| {
            f.get("id").and_then(|v| v.as_str()) == Some("apply-mixed-1")
                && f.get("method").is_none()
        })
        .expect("the server observes the synthesized response")
        .clone();
    assert_eq!(
        answer["result"],
        serde_json::json!({ "applied": false }),
        "the relay answers the mixed applyEdit fail-closed on the editor's \
         behalf with the negative ApplyWorkspaceEditResult: {answer}"
    );

    // CONTROL on the ordered server→editor pump: once a LATER carrier-free
    // frame reaches the editor, the earlier applyEdit was definitively
    // withheld (not merely delayed) and the wire is proven live.
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "window/logMessage",
            "params": { "type": 3, "message": "control" },
        }),
    )
    .await;
    wait_for_trace(&editor_trace, |frames| {
        frames
            .iter()
            .any(|f| f.get("method").and_then(|m| m.as_str()) == Some("window/logMessage"))
    })
    .await;
    let editor_frames = editor_trace.lock().unwrap().clone();
    assert_eq!(
        editor_frames.len(),
        1,
        "the editor receives ONLY the control frame — neither the mixed \
         applyEdit nor its filtered user remainder: {editor_frames:?}"
    );
    assert!(
        !editor_frames
            .iter()
            .any(|f| f.get("method").and_then(|m| m.as_str()) == Some("workspace/applyEdit")),
        "no applyEdit request may be editor-routed: {editor_frames:?}"
    );
    assert!(
        !editor_frames
            .iter()
            .any(|f| serde_json::to_string(f).unwrap().contains(EGRESS_CARRIER)),
        "no editor-bound frame may reference the carrier URI: {editor_frames:?}"
    );
    assert_eq!(
        relay.suppressed_egress(),
        1,
        "the answered server request counts as not-forwarded-to-editor"
    );
    relay.shutdown().await;
}

#[tokio::test]
async fn relay_answers_reserved_id_server_request_to_server_never_editor() {
    let (editor_trace, _editor_write, server_read, mut server_write, relay) =
        relay_with_collected_editor_and_raw_server();
    // The collector is a generic frame recorder: over the server endpoint's
    // read half it records every frame the RELAY writes toward the server.
    let server_inbound = spawn_editor_collector(server_read);

    // The server emits a carrier-FREE server→client REQUEST whose id sits in
    // the reserved `verter:*` namespace. Forwarding it would hang the
    // server: the editor's answer would carry the same reserved id, and the
    // editor→server pump drops ALL reserved-id editor frames (the namespace
    // boundary) — the server's request would never resolve. The relay must
    // answer the SERVER with a synthesized negative under the ORIGINAL id
    // and keep the request from the editor entirely.
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": "verter:7",
            "method": "window/showDocument",
            "params": { "uri": "file:///ws/user.ts" },
        }),
    )
    .await;

    // The SERVER receives the synthesized negative response, matching id.
    wait_for_trace(&server_inbound, |frames| {
        frames.iter().any(|f| {
            f.get("id").and_then(|v| v.as_str()) == Some("verter:7") && f.get("method").is_none()
        })
    })
    .await;
    let inbound = server_inbound.lock().unwrap().clone();
    let answer = inbound
        .iter()
        .find(|f| {
            f.get("id").and_then(|v| v.as_str()) == Some("verter:7") && f.get("method").is_none()
        })
        .expect("the server observes the synthesized negative response")
        .clone();
    assert_eq!(
        answer["error"]["code"],
        serde_json::json!(-32803),
        "the reserved-id anomaly is answered with the sanitized RequestFailed \
         error under the ORIGINAL id: {answer}"
    );
    assert!(
        answer.get("result").is_none(),
        "the synthesized anomaly answer is a plain ERROR response: {answer}"
    );

    // CONTROL on the ordered server→editor pump: once a LATER carrier-free
    // frame reaches the editor, the earlier reserved-id request was
    // definitively withheld (not merely delayed) and the wire is proven live.
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "window/logMessage",
            "params": { "type": 3, "message": "control" },
        }),
    )
    .await;
    wait_for_trace(&editor_trace, |frames| {
        frames
            .iter()
            .any(|f| f.get("method").and_then(|m| m.as_str()) == Some("window/logMessage"))
    })
    .await;
    let editor_frames = editor_trace.lock().unwrap().clone();
    assert_eq!(
        editor_frames.len(),
        1,
        "the editor receives ONLY the control frame — nothing of the \
         reserved-id server request: {editor_frames:?}"
    );
    assert!(
        !editor_frames.iter().any(|f| f
            .get("id")
            .and_then(|v| v.as_str())
            .is_some_and(|s| s.starts_with("verter:"))),
        "no editor-bound frame may carry a reserved `verter:*` id: {editor_frames:?}"
    );
    assert_eq!(
        relay.suppressed_egress(),
        1,
        "the answered reserved-id anomaly counts as not-forwarded-to-editor"
    );
    relay.shutdown().await;
}

#[tokio::test]
async fn relay_answers_editor_definition_with_neutral_when_carrier_only() {
    let (editor_trace, mut editor_write, server_read, mut server_write, relay) =
        relay_with_collected_editor_and_raw_server();
    // The collector is a generic frame recorder: over the server endpoint's
    // read half it records every frame the RELAY writes toward the server.
    let server_inbound = spawn_editor_collector(server_read);

    relay
        .injection_channel()
        .did_open(EGRESS_CARRIER, "typescriptreact", 1, "export {};")
        .await
        .expect("the injected didOpen taints the carrier overlay");

    // The EDITOR asks for a definition (a carrier-free request — it forwards
    // to the server untouched, and its id→method is tracked on ingress).
    write_frame(
        &mut editor_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "method": "textDocument/definition",
            "params": { "textDocument": { "uri": "file:///ws/user.ts" },
                        "position": { "line": 1, "character": 2 } },
        }),
    )
    .await;
    // Wait until the request reached the server — by then the ingress pump
    // has recorded the pending id→method (record happens before forward).
    wait_for_trace(&server_inbound, |frames| {
        frames
            .iter()
            .any(|f| f.get("method").and_then(|m| m.as_str()) == Some("textDocument/definition"))
    })
    .await;

    // The server answers with a carrier-ONLY singleton Location — an
    // unfilterable response shape (a bare Location object). Suppressing it
    // whole would strand the editor's pending request; the relay must
    // instead resolve it with the method-valid NEUTRAL `result: null` under
    // the ORIGINAL id, carrying NO carrier data.
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 5,
            "result": { "uri": EGRESS_CARRIER,
                        "range": { "start": { "line": 0, "character": 0 },
                                   "end": { "line": 0, "character": 1 } } },
        }),
    )
    .await;

    wait_for_trace(&editor_trace, |frames| {
        frames
            .iter()
            .any(|f| f.get("id").and_then(serde_json::Value::as_i64) == Some(5))
    })
    .await;
    let frames = editor_trace.lock().unwrap().clone();
    let received = frames
        .iter()
        .find(|f| f.get("id").and_then(serde_json::Value::as_i64) == Some(5))
        .expect("the editor's pending request must RESOLVE (no strand)")
        .clone();
    assert_eq!(
        received,
        serde_json::json!({ "jsonrpc": "2.0", "id": 5, "result": null }),
        "the synthesized neutral response carries the ORIGINAL id and a \
         method-valid null result — nothing else: {received}"
    );
    assert!(
        !frames
            .iter()
            .any(|f| serde_json::to_string(f).unwrap().contains(EGRESS_CARRIER)),
        "no editor-bound frame may reference the carrier URI: {frames:?}"
    );
    assert_eq!(
        relay.suppressed_egress(),
        1,
        "the carrier response was kept from the editor (replaced by the \
         neutral) — recorded on the egress counter"
    );
    relay.shutdown().await;
}

#[tokio::test]
async fn relay_prunes_pending_request_on_cancel_no_fabricated_reply() {
    let (editor_trace, mut editor_write, server_read, mut server_write, relay) =
        relay_with_collected_editor_and_raw_server();
    // The collector is a generic frame recorder: over the server endpoint's
    // read half it records every frame the RELAY writes toward the server.
    let server_inbound = spawn_editor_collector(server_read);

    relay
        .injection_channel()
        .did_open(EGRESS_CARRIER, "typescriptreact", 1, "export {};")
        .await
        .expect("the injected didOpen taints the carrier overlay");

    // The EDITOR asks for a definition (tracked on ingress, id → method).
    write_frame(
        &mut editor_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "method": "textDocument/definition",
            "params": { "textDocument": { "uri": "file:///ws/user.ts" },
                        "position": { "line": 1, "character": 2 } },
        }),
    )
    .await;
    wait_for_trace(&server_inbound, |frames| {
        frames
            .iter()
            .any(|f| f.get("method").and_then(|m| m.as_str()) == Some("textDocument/definition"))
    })
    .await;

    // The EDITOR cancels the request: the relay prunes the pending record
    // (bounding the table even when a server never responds) AND still
    // forwards the notification raw to the server.
    write_frame(
        &mut editor_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "$/cancelRequest",
            "params": { "id": 7 },
        }),
    )
    .await;
    wait_for_trace(&server_inbound, |frames| {
        frames
            .iter()
            .any(|f| f.get("method").and_then(|m| m.as_str()) == Some("$/cancelRequest"))
    })
    .await;

    // The server STILL answers the cancelled request — with a carrier-ONLY
    // unfilterable response. The pending entry was pruned, so the response
    // correlates with NO tracked editor request: it suppresses whole,
    // fail-closed — the relay never fabricates a reply for an id it no
    // longer tracks.
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "id": 7,
            "result": { "uri": EGRESS_CARRIER,
                        "range": { "start": { "line": 0, "character": 0 },
                                   "end": { "line": 0, "character": 1 } } },
        }),
    )
    .await;

    // CONTROL on the ordered server→editor pump: once a LATER carrier-free
    // frame reaches the editor, the earlier id-7 response was definitively
    // withheld (not merely delayed) and the wire is proven live.
    write_frame(
        &mut server_write,
        &serde_json::json!({
            "jsonrpc": "2.0",
            "method": "window/logMessage",
            "params": { "type": 3, "message": "control" },
        }),
    )
    .await;
    wait_for_trace(&editor_trace, |frames| {
        frames
            .iter()
            .any(|f| f.get("method").and_then(|m| m.as_str()) == Some("window/logMessage"))
    })
    .await;
    let frames = editor_trace.lock().unwrap().clone();
    assert!(
        !frames
            .iter()
            .any(|f| f.get("id").and_then(serde_json::Value::as_i64) == Some(7)),
        "the cancelled request's entry was pruned — the editor receives NO \
         completion (neither a neutral nor an error) for id 7: {frames:?}"
    );
    assert!(
        !frames
            .iter()
            .any(|f| serde_json::to_string(f).unwrap().contains(EGRESS_CARRIER)),
        "no editor-bound frame may reference the carrier URI: {frames:?}"
    );
    assert_eq!(
        relay.suppressed_egress(),
        1,
        "the untracked carrier-only response suppresses whole, fail-closed — \
         recorded on the egress counter"
    );
    relay.shutdown().await;
}

#[tokio::test]
async fn relay_demuxes_verter_response_before_egress_suppression() {
    let (editor_trace, _editor_write, server_read, server_write, relay) =
        relay_with_collected_editor_and_raw_server();

    // A bespoke responder answering every request with a result that
    // REFERENCES the carrier. The egress policy classifies such a
    // carrier-referencing response as unfilterable (its result shape is not
    // a recognized filter target once the top-level `uri` survives) — so if
    // egress ran BEFORE the `verter:*` demux, the injected barrier's waiter
    // would never resolve and the round-trip below would time out.
    let mut server_read = server_read;
    let mut server_write = server_write;
    tokio::spawn(async move {
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = match server_read.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                let Some(id) = msg.get("id").filter(|v| !v.is_null()).cloned() else {
                    continue;
                };
                let reply = serde_json::json!({
                    "jsonrpc": "2.0",
                    "id": id,
                    "result": { "uri": EGRESS_CARRIER, "items": [] },
                });
                let _ = server_write.write_all(&encode_message(&reply)).await;
                let _ = server_write.flush().await;
            }
        }
    });

    let channel = relay.injection_channel();
    channel
        .did_open(EGRESS_CARRIER, "typescriptreact", 1, "export {};")
        .await
        .expect("the injected didOpen tracks the carrier overlay");
    tokio::time::timeout(Duration::from_secs(5), channel.sync_overlay(EGRESS_CARRIER))
        .await
        .expect(
            "the injected barrier must resolve — its carrier-referencing \
             response demuxes to Verter BEFORE the egress policy runs",
        )
        .expect("the demuxed response completes the round-trip");

    let frames = editor_trace.lock().unwrap().clone();
    assert!(
        !frames
            .iter()
            .any(|f| serde_json::to_string(f).unwrap().contains(EGRESS_CARRIER)),
        "the demuxed `verter:*` response must never reach the editor: {frames:?}"
    );
    relay.shutdown().await;
}

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

// ────────────────────────────────────────────────────────────────────────────
// In-band `initialize` witness capture + the relay-stopped signal — the hooks a
// shim drives (waitInitialized barrier / editor-disconnect teardown).
// ────────────────────────────────────────────────────────────────────────────

/// The relay captures the in-band `initialize` witness (the engine
/// `serverInfo.version`, the editor's `initialize` id, and its workspace
/// params) as the pass-through handshake completes — and NOT before.
#[tokio::test]
async fn relay_captures_in_band_initialize_witness_as_handshake_passes() {
    let (editor_endpoint, relay_editor_side) = tokio::io::duplex(64 * 1024);
    let (server_endpoint, relay_server_side) = tokio::io::duplex(64 * 1024);
    let (er, ew) = tokio::io::split(relay_editor_side);
    let (sr, sw) = tokio::io::split(relay_server_side);
    let relay = LspRelay::start(er, ew, sr, sw);
    let (mut editor_read, mut editor_write) = tokio::io::split(editor_endpoint);
    let (mut server_read, mut server_write) = tokio::io::split(server_endpoint);

    // Discriminating negative: before any handshake, there is NO witness (the
    // capture is not an always-Some default).
    assert!(
        relay.initialized_witness().is_none(),
        "a relay with no observed initialize must have no witness"
    );

    // The editor sends `initialize` (id 7 + workspace params); the relay
    // forwards it to the server.
    let init_req = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 7,
        "method": "initialize",
        "params": {
            "rootUri": "file:///w",
            "workspaceFolders": [{ "uri": "file:///w", "name": "w" }],
            "capabilities": {},
        },
    });
    write_frame(&mut editor_write, &init_req).await;
    let forwarded_req = read_frame(&mut server_read).await;
    assert_eq!(forwarded_req["method"], "initialize");

    // The server answers with its in-band `serverInfo.version`; the relay
    // forwards the response to the editor AND captures the witness.
    let init_resp = serde_json::json!({
        "jsonrpc": "2.0",
        "id": 7,
        "result": { "serverInfo": { "name": "tsgo", "version": "7.0.1-rc" }, "capabilities": {} },
    });
    write_frame(&mut server_write, &init_resp).await;
    let forwarded_resp = read_frame(&mut editor_read).await;
    assert_eq!(
        forwarded_resp["id"], 7,
        "the editor still receives the real response"
    );

    let witness = tokio::time::timeout(Duration::from_secs(5), relay.wait_initialized())
        .await
        .expect("wait_initialized timed out")
        .expect("the handshake completed, so a witness must be present");
    assert_eq!(
        witness.server_info_version.as_deref(),
        Some("7.0.1-rc"),
        "the in-band serverInfo.version is captured"
    );
    assert_eq!(
        witness.observed_initialize_id,
        serde_json::json!(7),
        "the editor's initialize id is captured"
    );
    assert_eq!(
        witness.root_uri.as_deref(),
        Some("file:///w"),
        "the rootUri workspace witness is captured from the request"
    );
    assert_eq!(
        witness.workspace_folders,
        Some(serde_json::json!([{ "uri": "file:///w", "name": "w" }])),
        "the workspaceFolders witness is captured from the request"
    );

    drop(editor_write);
    relay.shutdown().await;
}

/// The relay signals `stopped` when the editor side disconnects (stdin EOF),
/// and does NOT signal while it is live — the teardown trigger a shim selects on.
#[tokio::test]
async fn relay_signals_stopped_on_editor_disconnect() {
    let (editor_endpoint, relay_editor_side) = tokio::io::duplex(64 * 1024);
    let (server_endpoint, relay_server_side) = tokio::io::duplex(64 * 1024);
    let (er, ew) = tokio::io::split(relay_editor_side);
    let (sr, sw) = tokio::io::split(relay_server_side);
    let relay = LspRelay::start(er, ew, sr, sw);

    // Discriminating: a LIVE relay must NOT report stopped.
    let while_live = tokio::time::timeout(Duration::from_millis(250), relay.wait_stopped()).await;
    assert!(
        while_live.is_err(),
        "a live relay (both streams open) must not report stopped"
    );

    // The editor disconnects: dropping the whole editor endpoint EOFs the
    // relay's editor read, ending the editor→server pump.
    drop(editor_endpoint);

    tokio::time::timeout(Duration::from_secs(5), relay.wait_stopped())
        .await
        .expect("the relay must report stopped after the editor disconnects");

    // The server endpoint is dropped last (kept alive until here so the stop is
    // attributable to the EDITOR disconnect, not a server EOF).
    drop(server_endpoint);
    relay.shutdown().await;
}
