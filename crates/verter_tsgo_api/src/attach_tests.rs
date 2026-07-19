//! Unit tests for the attach orchestration's pure parsing + seam shape, the
//! OWNED handshake-half (`lsp_handshake`), the non-owning composer
//! (`attach_to_initialized`), and the ownership-dispatched teardown
//! (`teardown` → `shutdown` / `detach`). The LIVE end-to-end attach proof
//! (against a real tsgo) lives in `tests/attach_live.rs` (gated on
//! `VERTER_REQUIRE_TSGO`).

use super::*;

use std::collections::HashSet;
use std::sync::{Arc, Mutex as StdMutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};

use crate::gate::{self, EngineVersionWitness, GateClearance, ObservedEngine};
use crate::jsonrpc::framing::{encode_message, MessageFramer};

#[test]
fn initialize_api_session_method_string_is_exact() {
    // The method string is server-side (Go binary), verified against the shipped
    // native-preview binary. Pin it so a typo cannot silently break the attach.
    assert_eq!(INITIALIZE_API_SESSION_METHOD, "custom/initializeAPISession");
}

#[tokio::test]
async fn initialize_api_session_parses_session_and_pipe() {
    // Drive the handshake parse over an in-memory duplex with a fake server that
    // answers `custom/initializeAPISession` with `{ sessionId, pipe }` (the exact
    // shape the live binary returns).
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client);
    let (mut sr, mut sw) = tokio::io::split(server);

    tokio::spawn(async move {
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = match sr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                if let (Some(id), Some(method)) = (
                    msg.get("id").cloned(),
                    msg.get("method").and_then(|m| m.as_str()),
                ) {
                    if method == INITIALIZE_API_SESSION_METHOD {
                        let reply = serde_json::json!({
                            "jsonrpc": "2.0", "id": id,
                            "result": { "sessionId": "api-session-1", "pipe": r"\\.\pipe\tsgo-api-abc-def" }
                        });
                        let _ = sw.write_all(&encode_message(&reply)).await;
                        let _ = sw.flush().await;
                    }
                }
            }
        }
    });

    let conn = JsonRpcConnection::connect(cr, cw);
    let handle = TsgoAttach::<Owned>::initialize_api_session(&conn)
        .await
        .expect("attach handshake ok");
    assert_eq!(handle.session_id, "api-session-1");
    assert_eq!(handle.pipe, r"\\.\pipe\tsgo-api-abc-def");
    conn.close().await.unwrap();
}

#[tokio::test]
async fn initialize_api_session_missing_pipe_is_a_typed_error() {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client);
    let (mut sr, mut sw) = tokio::io::split(server);

    tokio::spawn(async move {
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = match sr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                if let Some(id) = msg.get("id").cloned() {
                    // Answer with NO pipe field (a malformed/old server).
                    let reply = serde_json::json!({
                        "jsonrpc": "2.0", "id": id, "result": { "sessionId": "x" }
                    });
                    let _ = sw.write_all(&encode_message(&reply)).await;
                    let _ = sw.flush().await;
                }
            }
        }
    });

    let conn = JsonRpcConnection::connect(cr, cw);
    let err = TsgoAttach::<Owned>::initialize_api_session(&conn)
        .await
        .expect_err("a result without `pipe` must fail");
    assert!(
        matches!(err, TsgoApiError::Transport(_)),
        "a missing `pipe` must be a typed Transport error, got {err:?}"
    );
    conn.close().await.unwrap();
}

// ────────────────────────────────────────────────────────────────────────────
// Fake-server harness: an in-memory duplex JSON-RPC peer that records every
// inbound `method` (requests AND notifications) in arrival order, answers
// `initialize` with a configurable result, answers
// `custom/initializeAPISession` with `{ sessionId, pipe }`, and swallows all
// other notifications. The server task ends when the client's write half
// drops (connection closed) — at that point the trace is COMPLETE.
// ────────────────────────────────────────────────────────────────────────────

/// The recorded wire trace: `(method, params.textDocument.uri)` per inbound
/// message, in arrival order.
type WireTrace = Arc<StdMutex<Vec<(String, Option<String>)>>>;

fn trace_methods(trace: &WireTrace) -> Vec<String> {
    trace
        .lock()
        .unwrap()
        .iter()
        .map(|(m, _)| m.clone())
        .collect()
}

fn did_close_uris(trace: &WireTrace) -> Vec<String> {
    trace
        .lock()
        .unwrap()
        .iter()
        .filter(|(m, _)| m == "textDocument/didClose")
        .filter_map(|(_, u)| u.clone())
        .collect()
}

/// A pipe path that exists on NO platform — `custom/initializeAPISession`
/// answers with it so any attempt to actually connect the `--api` pipe fails
/// fast instead of hanging (headless tests never reach a real pipe).
fn fake_nonexistent_pipe_path() -> String {
    if cfg!(windows) {
        format!(
            r"\\.\pipe\verter-tsgo-test-nonexistent-{}",
            std::process::id()
        )
    } else {
        std::env::temp_dir()
            .join(format!(
                "verter-tsgo-test-nonexistent-{}.sock",
                std::process::id()
            ))
            .to_string_lossy()
            .into_owned()
    }
}

fn init_result_with_version(version: &str) -> serde_json::Value {
    serde_json::json!({
        "capabilities": {},
        "serverInfo": { "name": "tsgo", "version": version },
    })
}

fn init_result_without_server_info() -> serde_json::Value {
    serde_json::json!({ "capabilities": {} })
}

/// Spawn the recording fake server over an in-memory duplex. Returns the
/// client-side connection, the shared trace, and the server task's join
/// handle. Await the join handle AFTER closing/dropping the client side to
/// read a complete trace (the task ends on the client write half's EOF).
fn spawn_fake_lsp_server(
    initialize_result: serde_json::Value,
) -> (JsonRpcConnection, WireTrace, tokio::task::JoinHandle<()>) {
    let (client, server) = tokio::io::duplex(64 * 1024);
    let (cr, cw) = tokio::io::split(client);
    let (mut sr, mut sw) = tokio::io::split(server);
    let trace: WireTrace = Arc::new(StdMutex::new(Vec::new()));
    let trace_task = Arc::clone(&trace);

    let join = tokio::spawn(async move {
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 4096];
        loop {
            let n = match sr.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                let Some(method) = msg.get("method").and_then(|m| m.as_str()) else {
                    continue;
                };
                let uri = msg
                    .get("params")
                    .and_then(|p| p.get("textDocument"))
                    .and_then(|t| t.get("uri"))
                    .and_then(|u| u.as_str())
                    .map(str::to_string);
                trace_task.lock().unwrap().push((method.to_string(), uri));
                if let Some(id) = msg.get("id").cloned() {
                    let result = match method {
                        "initialize" => initialize_result.clone(),
                        INITIALIZE_API_SESSION_METHOD => serde_json::json!({
                            "sessionId": "api-session-1",
                            "pipe": fake_nonexistent_pipe_path(),
                        }),
                        _ => serde_json::Value::Null,
                    };
                    let reply = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result });
                    let _ = sw.write_all(&encode_message(&reply)).await;
                    let _ = sw.flush().await;
                }
            }
        }
    });

    (JsonRpcConnection::connect(cr, cw), trace, join)
}

/// A clearance minted through the REAL gate over a supported stable version
/// with the in-band witness — the clearance `from_parts` stores.
fn test_clearance() -> GateClearance {
    gate::validate(&ObservedEngine::from_in_band_server_info("7.0.3"))
        .expect("a supported stable version clears the gate")
}

fn fake_session_handle() -> ApiSessionHandle {
    ApiSessionHandle {
        session_id: "api-session-test".into(),
        pipe: fake_nonexistent_pipe_path(),
    }
}

// ────────────────────────────────────────────────────────────────────────────
// The OWNED handshake-half: `lsp_handshake` reads the in-band
// `serverInfo.version` witness and feeds it to the wire gate (fail-closed).
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn lsp_handshake_reads_in_band_serverinfo_and_gates_accepted() {
    let (conn, trace, join) = spawn_fake_lsp_server(init_result_with_version("7.0.3"));

    let clearance = TsgoAttach::lsp_handshake(&conn, "file:///ws")
        .await
        .expect("a supported serverInfo.version must clear the gate");
    assert_eq!(clearance.observed_version, "7.0.3");
    assert_eq!(
        clearance.witness,
        EngineVersionWitness::InBandServerInfo,
        "the handshake's version witness is the IN-BAND serverInfo report, \
         not a --version probe"
    );

    conn.close().await.unwrap();
    join.await.unwrap();
    let methods = trace_methods(&trace);
    let init_at = methods
        .iter()
        .position(|m| m == "initialize")
        .expect("the handshake originates `initialize`");
    let inited_at = methods
        .iter()
        .position(|m| m == "initialized")
        .expect("the handshake completes with `initialized`");
    assert!(
        init_at < inited_at,
        "`initialize` must precede `initialized`: {methods:?}"
    );
}

#[tokio::test]
async fn lsp_handshake_fails_closed_on_unknown_serverinfo_version() {
    let (conn, trace, join) = spawn_fake_lsp_server(init_result_with_version("6.9.9"));

    let err = TsgoAttach::lsp_handshake(&conn, "file:///ws")
        .await
        .expect_err("an unsupported serverInfo.version must be refused");
    assert!(
        matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m) if m.contains("6.9.9")),
        "the refusal must be the typed wire gate naming the version; got {err:?}"
    );

    conn.close().await.unwrap();
    join.await.unwrap();
    let methods = trace_methods(&trace);
    assert!(
        methods.iter().any(|m| m == "initialize"),
        "the handshake sent `initialize` before observing the version: {methods:?}"
    );
    assert!(
        !methods.iter().any(|m| m == "initialized"),
        "the gate fails BEFORE `initialized` is sent: {methods:?}"
    );
}

#[tokio::test]
async fn lsp_handshake_fails_closed_on_missing_serverinfo() {
    let (conn, trace, join) = spawn_fake_lsp_server(init_result_without_server_info());

    let err = TsgoAttach::lsp_handshake(&conn, "file:///ws")
        .await
        .expect_err("an initialize result with no serverInfo cannot gate the engine");
    assert!(
        matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m) if m.contains("serverInfo")),
        "the refusal must name the missing serverInfo witness; got {err:?}"
    );

    conn.close().await.unwrap();
    join.await.unwrap();
    assert!(
        !trace_methods(&trace).iter().any(|m| m == "initialized"),
        "no `initialized` after a failed gate"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Composer ownership discipline: `attach_over` is the OWNED composer and must
// never re-`initialize` an editor-owned connection; `attach_to_initialized`
// gates the supplied in-band witness per-attach (fail-closed) BEFORE opening
// the `--api` session.
// ────────────────────────────────────────────────────────────────────────────

#[tokio::test]
async fn attach_over_refuses_a_non_owning_connection() {
    let (conn, trace, join) = spawn_fake_lsp_server(init_result_with_version("7.0.3"));
    let lsp = TsgoLspConnection::new_attached(conn.clone());
    assert_eq!(lsp.ownership(), ConnectionOwnership::AttachedNonOwning);

    let err = TsgoAttach::attach_over(lsp, "file:///ws")
        .await
        .expect_err("attach_over must refuse a non-owning connection");
    assert!(
        matches!(err, TsgoApiError::Transport(ref m) if m.contains("attach_to_initialized")),
        "the refusal must route the caller to the non-owning composer; got {err:?}"
    );

    conn.close().await.unwrap();
    join.await.unwrap();
    let methods = trace_methods(&trace);
    assert!(
        methods.is_empty(),
        "attach_over must refuse BEFORE originating ANY request — no second \
         `initialize` ever reaches an editor-owned connection: {methods:?}"
    );

    // CONTROL (discriminates the refusal on OWNERSHIP, not on the composer
    // being generally broken): the same composer over an OWNED connection DOES
    // originate `initialize` (the flow proceeds past the ownership check and
    // only fails later at the fake nonexistent pipe, which is irrelevant here).
    let (conn2, trace2, join2) = spawn_fake_lsp_server(init_result_with_version("7.0.3"));
    let owned = TsgoLspConnection::new_owned(conn2.clone(), None);
    assert_eq!(owned.ownership(), ConnectionOwnership::Owned);
    let _ = TsgoAttach::attach_over(owned, "file:///ws").await;
    conn2.close().await.unwrap();
    join2.await.unwrap();
    assert!(
        trace_methods(&trace2).iter().any(|m| m == "initialize"),
        "control: attach_over on an OWNED connection originates the handshake"
    );
}

#[tokio::test]
async fn attach_to_initialized_gates_supplied_version_fail_closed() {
    let (conn, trace, join) = spawn_fake_lsp_server(init_result_with_version("7.0.3"));
    let lsp = TsgoLspConnection::new_attached(conn.clone());

    let err = TsgoAttach::attach_to_initialized(lsp, "garbage")
        .await
        .expect_err("an unclassifiable supplied version must fail the per-attach gate");
    assert!(
        matches!(err, TsgoApiError::UnsupportedTsgoWire(ref m) if m.contains("garbage")),
        "the per-attach gate refusal names the supplied version; got {err:?}"
    );

    conn.close().await.unwrap();
    join.await.unwrap();
    let methods = trace_methods(&trace);
    assert!(
        !methods.iter().any(|m| m == INITIALIZE_API_SESSION_METHOD),
        "the gate PRECEDES the session open — no `custom/initializeAPISession` \
         after a refused witness: {methods:?}"
    );
    assert!(
        !methods.iter().any(|m| m == "initialize"),
        "attach_to_initialized NEVER originates `initialize` (the editor \
         already initialized this connection): {methods:?}"
    );
}

/// Mirror of `attach_over_refuses_a_non_owning_connection`: the non-owning
/// composer refuses an OWNED connection at entry — an Owned connection must
/// route through `attach_over` (whose teardown arm terminates the engine),
/// never through the non-owning composer.
#[tokio::test]
async fn attach_to_initialized_refuses_owned_connection() {
    let (conn, trace, join) = spawn_fake_lsp_server(init_result_with_version("7.0.3"));
    let owned = TsgoLspConnection::new_owned(conn.clone(), None);
    assert_eq!(owned.ownership(), ConnectionOwnership::Owned);

    let err = TsgoAttach::attach_to_initialized(owned, "7.0.3")
        .await
        .expect_err("attach_to_initialized must refuse an OWNED connection");
    assert!(
        matches!(err, TsgoApiError::Transport(ref m) if m.contains("non-owning")),
        "the refusal must name the non-owning requirement; got {err:?}"
    );

    conn.close().await.unwrap();
    join.await.unwrap();
    let methods = trace_methods(&trace);
    assert!(
        !methods.iter().any(|m| m == INITIALIZE_API_SESSION_METHOD),
        "the ownership refusal PRECEDES the session open — no \
         `custom/initializeAPISession` on a refused connection: {methods:?}"
    );
}

// ────────────────────────────────────────────────────────────────────────────
// Teardown behavior: the NON-OWNING teardown retracts Verter's own overlays
// and drops the --api pipe, NEVER exit/shutdown/kill; the OWNED teardown DOES
// terminate. Built via `from_parts` over duplexes (no real OS pipe).
// ────────────────────────────────────────────────────────────────────────────

/// Open the two invariant-pair overlays through the gated injection channel.
async fn open_invariant_pair<O: AttachOwnership>(attach: &TsgoAttach<O>) {
    let channel = attach.injection_channel();
    channel
        .did_open("file:///ws/A.vue.tsx", "typescriptreact", 1, "export {};")
        .await
        .unwrap();
    channel
        .did_open("file:///ws/B.vue.tsx", "typescriptreact", 1, "export {};")
        .await
        .unwrap();
}

/// Drive the IDENTICAL attach flow for the non-owning invariant A/B pair:
/// same two overlays opened, torn down through the SAME public `teardown()`
/// entry of each ownership. The ONLY input difference between the pair is
/// the connection OWNERSHIP (the runtime tag plus its matching compile-time
/// marker), so every observed wire delta is attributable solely to it: the
/// load-bearing delta is the engine-terminating `exit` (present on OWNED,
/// ABSENT on non-owning), while the non-owning arm instead retracts its
/// overlays via `didClose`. Returns the completed `--lsp` and `--api` traces.
async fn teardown_flow_traces(ownership: ConnectionOwnership) -> (WireTrace, WireTrace) {
    let (lsp_conn, lsp_trace, lsp_join) = spawn_fake_lsp_server(init_result_with_version("7.0.3"));
    let (api_conn, api_trace, api_join) = spawn_fake_lsp_server(serde_json::Value::Null);

    match ownership {
        // Owned WITHOUT a real child (None): ownership drives the teardown
        // arm; no process exists to kill, isolating the wire-visible teardown.
        ConnectionOwnership::Owned => {
            let lsp = TsgoLspConnection::new_owned(lsp_conn.clone(), None);
            assert_eq!(lsp.ownership(), ownership);
            let attach = TsgoAttach::<Owned>::from_parts(
                lsp,
                ApiAttachClient::new(api_conn),
                fake_session_handle(),
                test_clearance(),
            );
            open_invariant_pair(&attach).await;
            attach.teardown().await.expect("teardown returns Ok");
        }
        // AttachedNonOwning carries NO child BY CONSTRUCTION (`new_attached`
        // takes no child handle), so no kill path is structurally reachable.
        ConnectionOwnership::AttachedNonOwning => {
            let lsp = TsgoLspConnection::new_attached(lsp_conn.clone());
            assert_eq!(lsp.ownership(), ownership);
            let attach = TsgoAttach::<NonOwning>::from_parts(
                lsp,
                ApiAttachClient::new(api_conn),
                fake_session_handle(),
                test_clearance(),
            );
            open_invariant_pair(&attach).await;
            attach.teardown().await.expect("teardown returns Ok");
        }
    }

    // Complete the traces: the OWNED arm closed the --lsp connection itself;
    // the NON-OWNING arm leaves it open — end the writer via the retained
    // clone so the fake server sees EOF either way (a second close is a
    // no-op on an already-closed connection).
    let _ = lsp_conn.close().await;
    lsp_join.await.unwrap();
    // Both arms drop the --api pipe: its fake server saw EOF and finished.
    api_join.await.unwrap();

    (lsp_trace, api_trace)
}

/// THE non-owning invariant: a Verter-side `teardown()` of an editor-owned
/// engine issues NO engine-terminating signal — only `textDocument/didClose`
/// for its own overlays plus the `--api` pipe drop. Identical A/B pair with
/// `owned_teardown_sends_exit`: same overlays, same `teardown()` entry; the
/// pair differs ONLY on the ownership → `exit` delta.
#[tokio::test]
async fn non_owning_detach_retracts_overlays_and_never_exits_or_kills() {
    let (lsp_trace, api_trace) = teardown_flow_traces(ConnectionOwnership::AttachedNonOwning).await;

    let uris = did_close_uris(&lsp_trace);
    assert!(
        uris.contains(&"file:///ws/A.vue.tsx".to_string())
            && uris.contains(&"file:///ws/B.vue.tsx".to_string()),
        "non-owning teardown retracts BOTH overlays via textDocument/didClose: {uris:?}"
    );
    let methods = trace_methods(&lsp_trace);
    assert!(
        !methods.iter().any(|m| m == "exit"),
        "NON-OWNING teardown must NEVER send `exit`: {methods:?}"
    );
    assert!(
        !methods.iter().any(|m| m == "shutdown"),
        "NON-OWNING teardown must NEVER send `shutdown`: {methods:?}"
    );
    assert!(
        trace_methods(&api_trace).is_empty(),
        "non-owning teardown sends NO request on the --api wire — it only \
         drops the pipe"
    );
}

/// CONTROL (discriminates the non-owning invariant): the SAME flow — same
/// overlays, same `teardown()` entry — over an OWNED connection DOES send
/// `exit`. Identical A/B pair with
/// `non_owning_detach_retracts_overlays_and_never_exits_or_kills`; only the
/// ownership (and therefore the engine-terminating signal) differs.
#[tokio::test]
async fn owned_teardown_sends_exit() {
    let (lsp_trace, _api_trace) = teardown_flow_traces(ConnectionOwnership::Owned).await;

    let methods = trace_methods(&lsp_trace);
    assert!(
        methods.iter().any(|m| m == "exit"),
        "OWNED teardown DOES send `exit` — the discriminating control for the \
         non-owning pair: {methods:?}"
    );
}

/// `teardown()` dispatches on the ownership marker: on
/// `TsgoAttach<NonOwning>` it behaves as `detach` (didClose + NO exit). The
/// Owned arm is proven by `owned_teardown_sends_exit` through the same-named
/// `teardown()` entry point.
#[tokio::test]
async fn teardown_dispatches_on_ownership() {
    let (lsp_conn, lsp_trace, lsp_join) = spawn_fake_lsp_server(init_result_with_version("7.0.3"));
    let (api_conn, _api_trace, api_join) = spawn_fake_lsp_server(serde_json::Value::Null);

    let lsp = TsgoLspConnection::new_attached(lsp_conn.clone());
    let attach = TsgoAttach::<NonOwning>::from_parts(
        lsp,
        ApiAttachClient::new(api_conn),
        fake_session_handle(),
        test_clearance(),
    );
    attach
        .injection_channel()
        .did_open("file:///ws/C.vue.tsx", "typescriptreact", 1, "export {};")
        .await
        .unwrap();

    attach.teardown().await.expect("non-owning teardown");

    lsp_conn.close().await.unwrap();
    lsp_join.await.unwrap();
    api_join.await.unwrap();

    let uris = did_close_uris(&lsp_trace);
    assert!(
        uris.contains(&"file:///ws/C.vue.tsx".to_string()),
        "teardown on AttachedNonOwning retracts the overlay: {uris:?}"
    );
    let methods = trace_methods(&lsp_trace);
    assert!(
        !methods.iter().any(|m| m == "exit"),
        "teardown on AttachedNonOwning dispatches to detach — NO exit: {methods:?}"
    );
}

/// Overlay-set correctness: the channel's `did_open` tracks DISTINCT URIs for
/// retraction; a `did_change` on an already-open overlay must NOT add a
/// duplicate.
#[tokio::test]
async fn did_open_tracks_overlay_uris_for_retraction() {
    let (lsp_conn, lsp_trace, lsp_join) = spawn_fake_lsp_server(init_result_with_version("7.0.3"));
    let (api_conn, _api_trace, api_join) = spawn_fake_lsp_server(serde_json::Value::Null);

    let lsp = TsgoLspConnection::new_attached(lsp_conn.clone());
    let attach = TsgoAttach::<NonOwning>::from_parts(
        lsp,
        ApiAttachClient::new(api_conn),
        fake_session_handle(),
        test_clearance(),
    );

    {
        let channel = attach.injection_channel();
        channel
            .did_open("file:///ws/A.vue.tsx", "typescriptreact", 1, "export {};")
            .await
            .unwrap();
        channel
            .did_open("file:///ws/B.vue.tsx", "typescriptreact", 1, "export {};")
            .await
            .unwrap();
        channel
            .did_change("file:///ws/A.vue.tsx", 2, "export const x = 1;")
            .await
            .unwrap();
    }

    attach.detach().await.unwrap();

    lsp_conn.close().await.unwrap();
    lsp_join.await.unwrap();
    api_join.await.unwrap();

    let uris = did_close_uris(&lsp_trace);
    assert_eq!(
        uris.len(),
        2,
        "exactly ONE didClose per DISTINCT overlay (did_change must not \
         duplicate the retraction): {uris:?}"
    );
    let distinct: HashSet<&String> = uris.iter().collect();
    assert_eq!(distinct.len(), 2, "the two didClose target distinct URIs");
    assert!(
        uris.contains(&"file:///ws/A.vue.tsx".to_string())
            && uris.contains(&"file:///ws/B.vue.tsx".to_string()),
        "both opened overlays are retracted: {uris:?}"
    );
}

/// A FAILED `did_open` must NOT track a phantom overlay: the URI enters the
/// retraction set only AFTER the notify succeeded — otherwise a later detach
/// would `didClose` a document the server never opened.
#[tokio::test]
async fn did_open_failure_tracks_no_phantom_overlay() {
    let (lsp_conn, _lsp_trace, lsp_join) = spawn_fake_lsp_server(init_result_with_version("7.0.3"));
    let (api_conn, _api_trace, api_join) = spawn_fake_lsp_server(serde_json::Value::Null);
    let api_conn_keep = api_conn.clone();

    let lsp = TsgoLspConnection::new_attached(lsp_conn.clone());
    let attach = TsgoAttach::<NonOwning>::from_parts(
        lsp,
        ApiAttachClient::new(api_conn),
        fake_session_handle(),
        test_clearance(),
    );

    // Kill the --lsp wire, then wait (bounded, cooperative) until the writer
    // task has observed the close — a subsequent notify fails — so the
    // did_open below fails deterministically.
    lsp_conn.close().await.unwrap();
    let mut tries = 0u32;
    while lsp_conn
        .notify("verter/test-probe", serde_json::Value::Null)
        .await
        .is_ok()
    {
        tries += 1;
        assert!(tries < 100_000, "a closed connection must refuse notifies");
        tokio::task::yield_now().await;
    }

    attach
        .injection_channel()
        .did_open(
            "file:///ws/Phantom.vue.tsx",
            "typescriptreact",
            1,
            "export {};",
        )
        .await
        .expect_err("did_open over a closed connection fails");

    assert!(
        attach.open_overlays.lock().unwrap().is_empty(),
        "a FAILED did_open must not track a phantom overlay URI for retraction"
    );

    lsp_join.await.unwrap();
    api_conn_keep.close().await.unwrap();
    api_join.await.unwrap();
}
