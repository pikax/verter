//! In-process loopback proof of the whole control server + client dispatch,
//! WITHOUT a real tsgo engine: a fake tsgo server answers the handshake / sync
//! barrier / initializeAPISession behind a real [`LspRelay`], and the control
//! client drives every method through the control server over an in-memory
//! transport. Discriminating: every assertion checks a REAL dispatched result
//! (session witnesses, the captured initialize witness, carrier recording, the
//! minted endpoint, the detach retraction + non-destructive relay liveness), plus the
//! fail-closed negatives (nonce mismatch, protocol mismatch, not-authenticated).

use super::*;
use crate::control::client::ControlClient;
use crate::control::messages::{ERROR_PROTOCOL_MISMATCH, METHOD_HELLO};
use crate::jsonrpc::framing::{encode_message, MessageFramer};
use crate::jsonrpc::JsonRpcConnection;
use crate::relay::LspRelay;

use std::sync::{Arc, Mutex as StdMutex};
use std::time::Duration;

use tokio::io::{AsyncReadExt, AsyncWriteExt, DuplexStream};

/// The fake tsgo's minted `--api` endpoint (never connected here — the test
/// asserts the control result carries it through). Platform-shaped so the
/// control server maps it to the matching `pipeName` / `socketPath` field.
fn fake_api_pipe() -> String {
    if cfg!(windows) {
        r"\\.\pipe\fake-tsgo-api".to_string()
    } else {
        "/tmp/fake-tsgo-api.sock".to_string()
    }
}

#[derive(Default)]
struct FakeServerState {
    opened: Vec<String>,
    closed: Vec<String>,
    saw_initialize: bool,
    saw_api_session: bool,
}

/// A fake `tsgo --lsp` server behind the relay: answers `initialize` with an
/// in-band `serverInfo.version`, `custom/initializeAPISession` with a minted
/// pipe, the pull-diagnostic sync barrier with an empty result, and records
/// carrier didOpen/didClose.
fn spawn_fake_tsgo(endpoint: DuplexStream) -> Arc<StdMutex<FakeServerState>> {
    spawn_fake_tsgo_cfg(endpoint, true)
}

/// [`spawn_fake_tsgo`] with an explicit `answer_barrier` knob. When `false`, the
/// fake still RECORDS the carrier `didOpen`/`didClose` and answers `initialize`,
/// but NEVER answers the pull-diagnostic sync barrier — the connection stays OPEN
/// (no EOF, so the relay stays alive), so the carrier's `did_open_synced` barrier
/// fails CLOSED via timeout. This models a slow/broken editor tsgo that RECEIVED
/// the overlay open but left the sync barrier unanswered — the sent-but-unsynced
/// case.
fn spawn_fake_tsgo_cfg(
    endpoint: DuplexStream,
    answer_barrier: bool,
) -> Arc<StdMutex<FakeServerState>> {
    let state = Arc::new(StdMutex::new(FakeServerState::default()));
    let st = Arc::clone(&state);
    tokio::spawn(async move {
        let (mut read, mut write) = tokio::io::split(endpoint);
        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 8192];
        loop {
            let n = match read.read(&mut chunk).await {
                Ok(0) | Err(_) => break,
                Ok(n) => n,
            };
            framer.push(&chunk[..n]);
            while let Ok(Some(msg)) = framer.next_message() {
                let method = msg.get("method").and_then(|m| m.as_str());
                let id = msg.get("id").cloned().filter(|v| !v.is_null());
                match (method, id) {
                    (Some("initialize"), Some(id)) => {
                        st.lock().unwrap().saw_initialize = true;
                        reply(
                            &mut write,
                            &id,
                            serde_json::json!({
                                "serverInfo": { "name": "faketsgo", "version": "7.0.1-rc" },
                                "capabilities": {},
                            }),
                        )
                        .await;
                    }
                    (Some("custom/initializeAPISession"), Some(id)) => {
                        st.lock().unwrap().saw_api_session = true;
                        reply(
                            &mut write,
                            &id,
                            serde_json::json!({ "sessionId": "s1", "pipe": fake_api_pipe() }),
                        )
                        .await;
                    }
                    (Some("textDocument/diagnostic"), Some(id)) if answer_barrier => {
                        // The sync barrier: any completed response proves order.
                        reply(
                            &mut write,
                            &id,
                            serde_json::json!({ "kind": "full", "items": [] }),
                        )
                        .await;
                    }
                    (Some("textDocument/didOpen"), None) => {
                        if let Some(uri) = carrier_uri(&msg) {
                            st.lock().unwrap().opened.push(uri);
                        }
                    }
                    (Some("textDocument/didClose"), None) => {
                        if let Some(uri) = carrier_uri(&msg) {
                            st.lock().unwrap().closed.push(uri);
                        }
                    }
                    _ => {}
                }
            }
        }
    });
    state
}

fn carrier_uri(msg: &serde_json::Value) -> Option<String> {
    msg.get("params")?
        .get("textDocument")?
        .get("uri")?
        .as_str()
        .map(str::to_string)
}

/// Decode one framed control response into its JSON value (a white-box test that
/// calls a handler directly gets the raw frame bytes back).
fn decode_frame(frame: &[u8]) -> serde_json::Value {
    let mut framer = MessageFramer::new();
    framer.push(frame);
    framer
        .next_message()
        .expect("frame decodes")
        .expect("one complete message")
}

async fn reply<W: AsyncWriteExt + Unpin>(
    write: &mut W,
    id: &serde_json::Value,
    result: serde_json::Value,
) {
    let frame = encode_message(&serde_json::json!({
        "jsonrpc": "2.0", "id": id, "result": result
    }));
    let _ = write.write_all(&frame).await;
    let _ = write.flush().await;
}

/// A wired loopback: the relay + fake tsgo, a control server over an in-memory
/// transport, a control client, and a fake editor endpoint.
struct Loopback {
    relay: Arc<LspRelay>,
    client: ControlClient,
    fake: Arc<StdMutex<FakeServerState>>,
    editor_write: tokio::io::WriteHalf<DuplexStream>,
    editor_read: tokio::io::ReadHalf<DuplexStream>,
}

fn wire_loopback(nonce: &str) -> Loopback {
    wire_loopback_cfg(nonce, true)
}

/// [`wire_loopback`] with an explicit `answer_barrier` knob threaded to the fake
/// tsgo (see [`spawn_fake_tsgo_cfg`]): `false` leaves the pull-diagnostic sync
/// barrier unanswered so a carrier open lands as SENT-but-unsynced.
fn wire_loopback_cfg(nonce: &str, answer_barrier: bool) -> Loopback {
    let (editor_endpoint, relay_editor) = tokio::io::duplex(64 * 1024);
    let (server_endpoint, relay_server) = tokio::io::duplex(64 * 1024);
    let (er, ew) = tokio::io::split(relay_editor);
    let (sr, sw) = tokio::io::split(relay_server);
    let relay = Arc::new(LspRelay::start(er, ew, sr, sw));
    let fake = spawn_fake_tsgo_cfg(server_endpoint, answer_barrier);
    let (editor_read, editor_write) = tokio::io::split(editor_endpoint);

    let (control_client_side, control_server_side) = tokio::io::duplex(64 * 1024);
    let (cs_r, cs_w) = tokio::io::split(control_server_side);
    let server = ControlServer::new(
        Arc::clone(&relay),
        nonce,
        7, // editor_session_generation
        0xABCD_u64,
        "ctl-1",
    );
    tokio::spawn(server.serve(cs_r, cs_w));
    let (cc_r, cc_w) = tokio::io::split(control_client_side);
    let client = ControlClient::from_connection(JsonRpcConnection::connect(cc_r, cc_w));

    Loopback {
        relay,
        client,
        fake,
        editor_write,
        editor_read,
    }
}

/// Drive the editor `initialize`/`initialized` handshake through the relay so
/// the in-band witness is captured (mirrors a real editor).
async fn drive_editor_initialize(
    editor_write: &mut tokio::io::WriteHalf<DuplexStream>,
    editor_read: &mut tokio::io::ReadHalf<DuplexStream>,
) {
    let init = encode_message(&serde_json::json!({
        "jsonrpc": "2.0", "id": 1, "method": "initialize",
        "params": { "rootUri": "file:///w", "workspaceFolders": [{ "uri": "file:///w", "name": "w" }], "capabilities": {} },
    }));
    editor_write.write_all(&init).await.unwrap();
    editor_write.flush().await.unwrap();
    // Read the forwarded initialize response (proves the handshake passed).
    let mut framer = MessageFramer::new();
    let mut chunk = [0u8; 8192];
    loop {
        let n = editor_read.read(&mut chunk).await.unwrap();
        framer.push(&chunk[..n]);
        if let Ok(Some(msg)) = framer.next_message() {
            assert_eq!(msg["id"], 1);
            break;
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn control_dispatch_drives_full_attach_lifecycle_through_relay() {
    let mut lb = wire_loopback("the-nonce");

    // The editor handshake passes → the relay captures the witness.
    drive_editor_initialize(&mut lb.editor_write, &mut lb.editor_read).await;

    // hello: the version + nonce gate passes and returns the shim witnesses.
    let hello = lb
        .client
        .hello("the-nonce", "verter_lsp")
        .await
        .expect("hello");
    assert_eq!(hello.protocol, PROTOCOL_VERSION);
    assert_eq!(hello.session_id, "ctl-1");
    assert_eq!(hello.wire_pin, 0xABCD);
    assert_eq!(hello.editor_session_generation, 7);
    assert!(hello.capabilities.carrier_injection && hello.capabilities.api_session);

    // waitInitialized: the in-band witness the relay captured.
    let witness = tokio::time::timeout(Duration::from_secs(5), lb.client.wait_initialized())
        .await
        .expect("waitInitialized timed out")
        .expect("waitInitialized");
    assert_eq!(witness.server_info_version.as_deref(), Some("7.0.1-rc"));
    assert_eq!(witness.observed_initialize_id, serde_json::json!(1));
    assert_eq!(witness.root_uri.as_deref(), Some("file:///w"));

    // carrierDidOpenSynced: the fake server receives the didOpen through the
    // gated injection channel.
    let carrier_uri = "file:///w/src/Carrier.ts";
    tokio::time::timeout(
        Duration::from_secs(5),
        lb.client
            .carrier_did_open_synced(carrier_uri, "typescript", 1, "export const x = 1;"),
    )
    .await
    .expect("didOpenSynced timed out")
    .expect("didOpenSynced");
    assert!(
        lb.fake
            .lock()
            .unwrap()
            .opened
            .iter()
            .any(|u| u == carrier_uri),
        "the fake server must have received the carrier didOpen"
    );

    // status: reflects the session state.
    let status = lb.client.status().await.expect("status");
    assert!(status.hello_completed);
    assert!(status.initialized, "the initialize witness is present");
    assert_eq!(status.open_carriers, 1);

    // initializeApiSession: the minted endpoint flows back.
    let api = tokio::time::timeout(Duration::from_secs(5), lb.client.initialize_api_session())
        .await
        .expect("initializeApiSession timed out")
        .expect("initializeApiSession");
    assert_eq!(api.endpoint(), Some(fake_api_pipe().as_str()));
    assert_eq!(api.wire_pin, 0xABCD);
    assert_eq!(api.handle_kind, "integer");
    assert!(lb.fake.lock().unwrap().saw_api_session);

    // detach(close_carriers): retracts the carrier and closes THIS control
    // connection ONLY — a NON-DESTRUCTIVE detach. The retraction reached the fake
    // server (best-effort; give it a beat).
    lb.client.detach(true).await.expect("detach");
    tokio::time::sleep(Duration::from_millis(50)).await;
    assert!(
        lb.fake
            .lock()
            .unwrap()
            .closed
            .iter()
            .any(|u| u == carrier_uri),
        "detach(close_carriers) must retract the carrier via didClose"
    );
    // NON-DESTRUCTIVE discriminator: a detach must NOT tear the shim down — the
    // relay (the editor↔tsgo path, and by extension the shim's OWNED tsgo child)
    // stays ALIVE. A shutdown-on-detach would stop the relay; a short wait on
    // `wait_stopped()` must TIME OUT (still alive).
    assert!(
        tokio::time::timeout(Duration::from_millis(200), lb.relay.wait_stopped())
            .await
            .is_err(),
        "verter/detach must be non-destructive — the relay (editor↔tsgo path) stays alive, \
         never torn down by a Verter control detach"
    );

    lb.client.close().await.unwrap();
    lb.relay.shutdown().await;
}

/// `verter/waitInitialized` is BOUNDED + cancellable: when the editor never sends
/// `initialize` (a broken / detached engine that answers hello but never completes the
/// LSP handshake), the control server must return a TYPED JSON-RPC error within its
/// internal timeout instead of blocking the control dispatch forever. The error is the
/// DISTINCT timeout variant (not the relay-stop variant — the relay is still alive), and
/// the timeout must NOT tear the relay down.
///
/// Discrimination: `handle_wait_initialized` bounds its `relay.wait_initialized()` await with
/// an internal 10s timeout, so with no editor initialize the bounded typed timeout error
/// returns after ~10s — well before the 20s outer bound. An UNBOUNDED await would never return,
/// the OUTER bound (strictly longer than the handler's internal 10s timeout) would fire, and the
/// `expect("BOUNDED")` would panic.
///
/// A real (unpaused) clock is used deliberately: `verter_tsgo_api` does not enable
/// tokio's `test-util` feature, so the virtual-clock `start_paused` seam is unavailable —
/// the outer bound (20s) is > the handler's 10s internal timeout, so the handler's
/// timeout is what returns and boundedness is proven within the outer bound.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn wait_initialized_times_out_when_editor_never_initializes() {
    let mut lb = wire_loopback("the-nonce");

    // hello completes (it must precede any other control method). We deliberately do NOT
    // drive the editor `initialize` handshake, so the relay never captures the in-band
    // witness and `relay.wait_initialized()` would block indefinitely.
    lb.client
        .hello("the-nonce", "verter_lsp")
        .await
        .expect("hello");

    // The control call must return a BOUNDED typed error. The outer bound (20s) is
    // strictly longer than the handler's internal 10s timeout, so the INNER timeout is
    // what returns; an unbounded handler would instead let the OUTER bound fire.
    let outer = Duration::from_secs(20);
    let result = tokio::time::timeout(outer, lb.client.wait_initialized()).await;
    let err = result
        .expect("waitInitialized must be BOUNDED — it must return within the outer bound")
        .expect_err("with no editor initialize, waitInitialized must be a typed error, not Ok");

    // DISTINCT timeout message — NOT the relay-stop message (the relay is still alive).
    let msg = err.to_string();
    assert!(
        msg.contains("timed out"),
        "the bounded error must be the TIMEOUT variant (distinct message); got {msg:?}"
    );
    assert!(
        !msg.contains("relay stopped"),
        "a live-relay timeout must NOT report the relay-stop variant; got {msg:?}"
    );

    // NEGATIVE / discriminator: the waitInitialized timeout must NOT stop the relay.
    assert!(
        tokio::time::timeout(Duration::from_millis(50), lb.relay.wait_stopped())
            .await
            .is_err(),
        "the waitInitialized timeout must leave the relay ALIVE (never a teardown)"
    );

    lb.client.close().await.unwrap();
    lb.relay.shutdown().await;
}

/// An ABNORMAL control-session termination — the control pipe dropped WITHOUT a
/// `verter/detach` (EOF on the server read) — must STILL retract the session's still-open
/// carrier overlays (send `didClose` to the real tsgo), so no stale Verter overlay lingers
/// in the editor's own tsgo Program. NON-DESTRUCTIVE: the retraction touches Verter's own
/// overlays only; the shim's relay (editor↔tsgo path + its OWNED tsgo child) stays ALIVE.
///
/// Discrimination: the session-end drain covers an EOF / dropped-pipe termination, not only an
/// explicit `verter/detach`. A drain that fired only on an explicit `verter/detach` would leave
/// the overlays OPEN and the fake tsgo would never see the `didClose`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn abnormal_control_termination_retracts_open_carriers_non_destructively() {
    let mut lb = wire_loopback("the-nonce");

    // Bring the session up and open a carrier (records it in the session's open set).
    drive_editor_initialize(&mut lb.editor_write, &mut lb.editor_read).await;
    lb.client
        .hello("the-nonce", "verter_lsp")
        .await
        .expect("hello");
    tokio::time::timeout(Duration::from_secs(5), lb.client.wait_initialized())
        .await
        .expect("waitInitialized timed out")
        .expect("waitInitialized");
    let carrier_uri = "file:///w/src/Carrier.ts";
    tokio::time::timeout(
        Duration::from_secs(5),
        lb.client
            .carrier_did_open_synced(carrier_uri, "typescript", 1, "export const x = 1;"),
    )
    .await
    .expect("didOpenSynced timed out")
    .expect("didOpenSynced");
    assert!(
        lb.fake
            .lock()
            .unwrap()
            .opened
            .iter()
            .any(|u| u == carrier_uri),
        "the carrier must be open before the abnormal termination"
    );

    // ABNORMAL termination: drop the control pipe WITHOUT a `verter/detach` (an EOF on
    // the server's read half — a control-pipe drop / crash without a clean detach).
    lb.client.close().await.unwrap();

    // The unified session-end drain must retract the session's still-open carrier (a
    // `didClose` to the real tsgo) even though no `verter/detach` was sent.
    let mut retracted = false;
    for _ in 0..50 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        if lb
            .fake
            .lock()
            .unwrap()
            .closed
            .iter()
            .any(|u| u == carrier_uri)
        {
            retracted = true;
            break;
        }
    }
    assert!(
        retracted,
        "an ABNORMAL control-session termination (pipe drop without detach) must retract \
         the session's open carriers (didClose) — no stale Verter overlay may linger in \
         the editor's tsgo Program"
    );

    // NON-DESTRUCTIVE discriminator: the shim's relay (the editor↔tsgo path, and its
    // OWNED tsgo child) must stay ALIVE — a `wait_stopped()` must TIME OUT (still alive).
    // The drain retracts overlays ONLY; it never terminates an engine Verter did not spawn.
    assert!(
        tokio::time::timeout(Duration::from_millis(200), lb.relay.wait_stopped())
            .await
            .is_err(),
        "an abnormal control termination must be NON-DESTRUCTIVE — the relay (editor↔tsgo \
         path + OWNED tsgo child) stays alive, never torn down by a control-session end"
    );

    lb.relay.shutdown().await;
}

/// A carrier whose `didOpen` was SENT to the real tsgo but whose sync BARRIER never
/// completed (the editor tsgo received the overlay open but never answered the
/// pull-diagnostic barrier — a slow/broken engine, bounded by `CARRIER_SYNC_BARRIER_TIMEOUT`)
/// MUST STILL be retracted by the session-end drain. The overlay is already LIVE in the
/// editor's own tsgo Program the moment the `didOpen` is sent (`relay::did_open` tracks it
/// before the barrier), so an un-retracted sent-but-unsynced open leaks a stale Verter
/// overlay into the editor's Program until editor teardown.
///
/// Discrimination: the control session tracks a carrier as retract-eligible at `didOpen`-SEND
/// time (before the barrier), so a barrier timeout still leaves it TRACKED and the session-end
/// drain retracts it on every termination mode. Tracking a carrier only on the FULL synced-open
/// success (`did_open_synced` returned Ok) would instead leave a barrier-timed-out carrier
/// UNTRACKED, and the drain would send NO `didClose` for it — the leak.
///
/// NON-DESTRUCTIVE: the drain retracts Verter's OWN overlay only; the relay (editor↔tsgo
/// path + its OWNED tsgo child) stays ALIVE (`non_owning_attach_lifecycle`).
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn sent_but_unsynced_open_is_retracted_on_session_end() {
    // A fake tsgo that answers `initialize` and RECORDS didOpen/didClose but NEVER answers
    // the `textDocument/diagnostic` sync barrier — the connection stays OPEN (relay alive),
    // so the carrier's sync barrier fails CLOSED via timeout.
    let mut lb = wire_loopback_cfg("the-nonce", false);

    drive_editor_initialize(&mut lb.editor_write, &mut lb.editor_read).await;
    lb.client
        .hello("the-nonce", "verter_lsp")
        .await
        .expect("hello");
    tokio::time::timeout(Duration::from_secs(5), lb.client.wait_initialized())
        .await
        .expect("waitInitialized timed out")
        .expect("waitInitialized");

    let carrier_uri = "file:///w/src/Carrier.ts";
    // Open the carrier: the didOpen REACHES the fake tsgo (recorded as a live overlay in
    // its Program), but the barrier never answers → `did_open_synced` fails CLOSED with a
    // Timeout after the bounded wait. The overlay is nonetheless LIVE in the editor's tsgo.
    let open = tokio::time::timeout(
        Duration::from_secs(30),
        lb.client
            .carrier_did_open_synced(carrier_uri, "typescript", 1, "export const x = 1;"),
    )
    .await
    .expect("carrier_did_open_synced must resolve within the bounded barrier");
    assert!(
        open.is_err(),
        "the sync barrier must FAIL closed (timeout) for the sent-but-unsynced scenario — \
         got {open:?}"
    );
    assert!(
        lb.fake
            .lock()
            .unwrap()
            .opened
            .iter()
            .any(|u| u == carrier_uri),
        "the didOpen must have reached the real tsgo (the overlay is live in its Program)"
    );
    assert!(
        !lb.fake
            .lock()
            .unwrap()
            .closed
            .iter()
            .any(|u| u == carrier_uri),
        "no didClose yet — the session is still open"
    );

    // ABNORMAL session end: drop the control pipe WITHOUT a `verter/detach`.
    lb.client.close().await.unwrap();

    // The session-end drain must retract the SENT-BUT-UNSYNCED overlay (a `didClose` to the
    // real tsgo) even though its sync barrier never completed.
    let mut retracted = false;
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        if lb
            .fake
            .lock()
            .unwrap()
            .closed
            .iter()
            .any(|u| u == carrier_uri)
        {
            retracted = true;
            break;
        }
    }
    assert!(
        retracted,
        "a SENT-BUT-UNSYNCED carrier open (didOpen sent, sync barrier never completed) MUST \
         be retracted by the session-end drain (didClose) — otherwise a stale Verter overlay \
         leaks into the editor's own tsgo Program"
    );

    // NON-DESTRUCTIVE discriminator: the relay (editor↔tsgo path + OWNED tsgo child) stays
    // ALIVE — a `wait_stopped()` must TIME OUT (still alive). The drain retracts overlays
    // ONLY; it never terminates an engine Verter did not spawn.
    assert!(
        tokio::time::timeout(Duration::from_millis(200), lb.relay.wait_stopped())
            .await
            .is_err(),
        "the session-end drain must be NON-DESTRUCTIVE — the relay (editor↔tsgo path + OWNED \
         tsgo child) stays alive, never torn down"
    );

    lb.relay.shutdown().await;
}

/// The `verter/detach` `closeCarriers: false` opt-out is HONORED by the unified
/// session-end drain: a client that explicitly asks to LEAVE its overlays open is not
/// retracted (the drain's `retract_carriers_on_end` gate). This preserves the wire
/// contract — the drain retracts by default and on every abnormal termination, but an
/// explicit opt-out is respected.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn detach_close_carriers_false_opts_out_of_the_session_end_drain() {
    let mut lb = wire_loopback("the-nonce");
    drive_editor_initialize(&mut lb.editor_write, &mut lb.editor_read).await;
    lb.client
        .hello("the-nonce", "verter_lsp")
        .await
        .expect("hello");
    tokio::time::timeout(Duration::from_secs(5), lb.client.wait_initialized())
        .await
        .expect("waitInitialized timed out")
        .expect("waitInitialized");
    let carrier_uri = "file:///w/src/Carrier.ts";
    tokio::time::timeout(
        Duration::from_secs(5),
        lb.client
            .carrier_did_open_synced(carrier_uri, "typescript", 1, "export const x = 1;"),
    )
    .await
    .expect("didOpenSynced timed out")
    .expect("didOpenSynced");

    // Detach with the EXPLICIT `closeCarriers: false` opt-out.
    lb.client.detach(false).await.expect("detach");

    // Give the session-end drain ample time to run; the carrier must NOT be retracted
    // (the opt-out is honored). This can only false-PASS on a too-short wait, never
    // false-fail — the opt-out leaves the carrier open forever.
    tokio::time::sleep(Duration::from_millis(150)).await;
    assert!(
        !lb.fake
            .lock()
            .unwrap()
            .closed
            .iter()
            .any(|u| u == carrier_uri),
        "detach(closeCarriers: false) must NOT retract the carrier — the explicit opt-out \
         is honored (the wire contract is preserved)"
    );

    lb.relay.shutdown().await;
}

#[tokio::test]
async fn control_hello_rejects_wrong_nonce() {
    let mut lb = wire_loopback("the-real-nonce");
    // Discriminating: the client presents a stale/spoofed nonce → fail closed.
    let result = lb.client.hello("stale-nonce", "verter_lsp").await;
    assert!(
        matches!(result, Err(crate::error::TsgoApiError::Transport(_))),
        "a wrong nonce must be refused (error response), got {result:?}"
    );
    lb.relay.shutdown().await;
}

#[tokio::test]
async fn control_methods_require_hello_first() {
    let lb = wire_loopback("n");
    // Discriminating: waitInitialized before hello is refused (not authenticated).
    let result = lb.client.status().await;
    assert!(
        matches!(result, Err(crate::error::TsgoApiError::Transport(_))),
        "a method before hello must be refused, got {result:?}"
    );
    lb.relay.shutdown().await;
}

#[tokio::test]
async fn control_hello_wrong_protocol_returns_typed_error_code() {
    // Raw-frame test to inspect the JSON-RPC error CODE (the client wrapper
    // discards it): a hello with a bumped protocol version fails closed with
    // exactly ERROR_PROTOCOL_MISMATCH — no attach.
    let (client_side, control_server_side) = tokio::io::duplex(64 * 1024);
    let (cs_r, cs_w) = tokio::io::split(control_server_side);
    let (editor_endpoint, relay_editor) = tokio::io::duplex(64 * 1024);
    let (server_endpoint, relay_server) = tokio::io::duplex(64 * 1024);
    let (er, ew) = tokio::io::split(relay_editor);
    let (sr, sw) = tokio::io::split(relay_server);
    let relay = Arc::new(LspRelay::start(er, ew, sr, sw));
    let _fake = spawn_fake_tsgo(server_endpoint);
    let _editor = editor_endpoint; // keep the editor side open

    let server = ControlServer::new(Arc::clone(&relay), "n", 1, 1, "ctl");
    tokio::spawn(server.serve(cs_r, cs_w));

    let (mut cc_r, mut cc_w) = tokio::io::split(client_side);
    let hello = encode_message(&serde_json::json!({
        "jsonrpc": "2.0", "id": 9, "method": METHOD_HELLO,
        "params": { "protocol": PROTOCOL_VERSION + 1, "nonce": "n", "client": "verter_lsp" },
    }));
    cc_w.write_all(&hello).await.unwrap();
    cc_w.flush().await.unwrap();

    let mut framer = MessageFramer::new();
    let mut chunk = [0u8; 8192];
    let response = loop {
        let n = cc_r.read(&mut chunk).await.unwrap();
        framer.push(&chunk[..n]);
        if let Ok(Some(msg)) = framer.next_message() {
            break msg;
        }
    };
    assert_eq!(response["id"], 9);
    assert_eq!(
        response["error"]["code"], ERROR_PROTOCOL_MISMATCH,
        "a wrong protocol must fail closed with ERROR_PROTOCOL_MISMATCH: {response}"
    );
    assert!(
        response.get("result").is_none(),
        "no result on a rejected hello"
    );
    relay.shutdown().await;
}

/// A wired loopback exposing the RAW control connection halves (no typed
/// [`ControlClient`]), so a test can send a `verter/detach` whose params body is
/// OMITTED (`{}`) or MALFORMED — bodies the typed client cannot express — and then
/// observe the unified session-end drain.
struct RawLoopback {
    relay: Arc<LspRelay>,
    fake: Arc<StdMutex<FakeServerState>>,
    editor_write: tokio::io::WriteHalf<DuplexStream>,
    editor_read: tokio::io::ReadHalf<DuplexStream>,
    cc_r: tokio::io::ReadHalf<DuplexStream>,
    cc_w: tokio::io::WriteHalf<DuplexStream>,
}

fn wire_raw_loopback(nonce: &str) -> RawLoopback {
    let (editor_endpoint, relay_editor) = tokio::io::duplex(64 * 1024);
    let (server_endpoint, relay_server) = tokio::io::duplex(64 * 1024);
    let (er, ew) = tokio::io::split(relay_editor);
    let (sr, sw) = tokio::io::split(relay_server);
    let relay = Arc::new(LspRelay::start(er, ew, sr, sw));
    let fake = spawn_fake_tsgo(server_endpoint);
    let (editor_read, editor_write) = tokio::io::split(editor_endpoint);

    let (control_client_side, control_server_side) = tokio::io::duplex(64 * 1024);
    let (cs_r, cs_w) = tokio::io::split(control_server_side);
    let server = ControlServer::new(Arc::clone(&relay), nonce, 7, 0xABCD_u64, "ctl-1");
    tokio::spawn(server.serve(cs_r, cs_w));
    let (cc_r, cc_w) = tokio::io::split(control_client_side);

    RawLoopback {
        relay,
        fake,
        editor_write,
        editor_read,
        cc_r,
        cc_w,
    }
}

/// Send one framed control request and read the matching response frame back
/// (draining any already-buffered frame first). Every control request carries an
/// id, so a response always follows.
async fn raw_control_request(
    cc_w: &mut tokio::io::WriteHalf<DuplexStream>,
    cc_r: &mut tokio::io::ReadHalf<DuplexStream>,
    framer: &mut MessageFramer,
    request: serde_json::Value,
) -> serde_json::Value {
    let frame = encode_message(&request);
    cc_w.write_all(&frame).await.unwrap();
    cc_w.flush().await.unwrap();
    let mut chunk = [0u8; 8192];
    loop {
        if let Ok(Some(msg)) = framer.next_message() {
            return msg;
        }
        let n = cc_r.read(&mut chunk).await.unwrap();
        framer.push(&chunk[..n]);
    }
}

/// Bring a RAW control session up to ONE open carrier: editor initialize + hello +
/// waitInitialized + a `carrierDidOpenSynced`, so the fake tsgo holds a live overlay
/// the session-end drain must retract. The carrier stays open on return.
async fn raw_bring_up_open_carrier(
    lb: &mut RawLoopback,
    framer: &mut MessageFramer,
    nonce: &str,
    carrier_uri: &str,
) {
    drive_editor_initialize(&mut lb.editor_write, &mut lb.editor_read).await;
    let hello = raw_control_request(
        &mut lb.cc_w,
        &mut lb.cc_r,
        framer,
        serde_json::json!({
            "jsonrpc": "2.0", "id": 1, "method": METHOD_HELLO,
            "params": { "protocol": PROTOCOL_VERSION, "nonce": nonce, "client": "verter_lsp" },
        }),
    )
    .await;
    assert!(
        hello.get("result").is_some(),
        "raw hello must succeed: {hello}"
    );
    let _ = raw_control_request(
        &mut lb.cc_w,
        &mut lb.cc_r,
        framer,
        serde_json::json!({
            "jsonrpc": "2.0", "id": 2, "method": crate::control::messages::METHOD_WAIT_INITIALIZED,
        }),
    )
    .await;
    let open = raw_control_request(
        &mut lb.cc_w,
        &mut lb.cc_r,
        framer,
        serde_json::json!({
            "jsonrpc": "2.0", "id": 3,
            "method": crate::control::messages::METHOD_CARRIER_DID_OPEN_SYNCED,
            "params": {
                "uri": carrier_uri, "languageId": "typescript", "version": 1,
                "text": "export const x = 1;"
            },
        }),
    )
    .await;
    assert!(
        open.get("result").is_some(),
        "raw carrier open must succeed: {open}"
    );
    assert!(
        lb.fake
            .lock()
            .unwrap()
            .opened
            .iter()
            .any(|u| u == carrier_uri),
        "the carrier must be open in the fake tsgo before the detach"
    );
}

/// Poll the fake tsgo up to ~2s for a `didClose` retraction of `carrier_uri`.
async fn poll_carrier_retracted(fake: &Arc<StdMutex<FakeServerState>>, carrier_uri: &str) -> bool {
    for _ in 0..100 {
        tokio::time::sleep(Duration::from_millis(20)).await;
        if fake.lock().unwrap().closed.iter().any(|u| u == carrier_uri) {
            return true;
        }
    }
    false
}

/// An explicit `verter/detach` with an OMITTED params body (`{}`) FAILS CLOSED — it
/// retracts the session's open carriers through the unified session-end drain (a
/// `didClose` to the real tsgo), NON-DESTRUCTIVELY. Only an EXPLICIT `closeCarriers:
/// false` opts out; an omitted/unspecified preference must NOT leave a stale Verter
/// overlay in the editor's own tsgo Program.
///
/// Discrimination: `handle_detach` reads `closeCarriers` as `Option<bool>`, so an omitted param
/// is `None` and FAILS CLOSED to a retract. Deserializing into a `bool` whose `Default` is
/// `false` and `unwrap_or_default()`-ing it would instead set `retract_carriers_on_end = false`,
/// SKIP the drain, and leak — the fake tsgo would never see the `didClose`.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn detach_omitted_params_fails_closed_and_retracts_via_drain() {
    let mut lb = wire_raw_loopback("the-nonce");
    let mut framer = MessageFramer::new();
    let carrier_uri = "file:///w/src/Carrier.ts";
    raw_bring_up_open_carrier(&mut lb, &mut framer, "the-nonce", carrier_uri).await;

    // Explicit `verter/detach` with an OMITTED params body (`{}`): FAIL CLOSED = retract.
    let ack = raw_control_request(
        &mut lb.cc_w,
        &mut lb.cc_r,
        &mut framer,
        serde_json::json!({
            "jsonrpc": "2.0", "id": 4, "method": crate::control::messages::METHOD_DETACH,
            "params": {},
        }),
    )
    .await;
    assert!(ack.get("result").is_some(), "detach must ack: {ack}");

    assert!(
        poll_carrier_retracted(&lb.fake, carrier_uri).await,
        "an OMITTED-params `verter/detach` must FAIL CLOSED — retract the session's open \
         carriers via the unified drain (didClose); an omitted preference may not leak a \
         stale Verter overlay into the editor's own tsgo Program"
    );

    // NON-DESTRUCTIVE: the relay (editor↔tsgo path + OWNED tsgo child) stays ALIVE — a
    // `wait_stopped()` must TIME OUT (still alive).
    assert!(
        tokio::time::timeout(Duration::from_millis(200), lb.relay.wait_stopped())
            .await
            .is_err(),
        "an omitted-params detach must be NON-DESTRUCTIVE — the relay stays alive, never \
         torn down by a Verter control detach"
    );

    lb.relay.shutdown().await;
}

/// An explicit `verter/detach` with a MALFORMED params body (`closeCarriers` is not a
/// bool) FAILS CLOSED — it retracts the session's open carriers through the unified
/// session-end drain, NON-DESTRUCTIVELY. A malformed body is treated as unspecified, so
/// it must NOT leave a stale Verter overlay in the editor's own tsgo Program.
///
/// Discrimination: `DetachParams::close_carriers` is `Option<bool>`, so a malformed body
/// deserializes (via `unwrap_or_default()`) to the `None` default and `None != Some(false)`
/// RETRACTS (fail-closed). An implementation that treated an unparseable `closeCarriers` as
/// `false` — a bool-shaped field, or an explicit opt-out fallback — would instead SKIP the
/// drain and leak a stale overlay.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn detach_malformed_params_fails_closed_and_retracts_via_drain() {
    let mut lb = wire_raw_loopback("the-nonce");
    let mut framer = MessageFramer::new();
    let carrier_uri = "file:///w/src/Carrier.ts";
    raw_bring_up_open_carrier(&mut lb, &mut framer, "the-nonce", carrier_uri).await;

    // Explicit `verter/detach` with a MALFORMED params body (`closeCarriers` is a string,
    // not a bool): FAIL CLOSED = retract.
    let ack = raw_control_request(
        &mut lb.cc_w,
        &mut lb.cc_r,
        &mut framer,
        serde_json::json!({
            "jsonrpc": "2.0", "id": 4, "method": crate::control::messages::METHOD_DETACH,
            "params": { "closeCarriers": "not-a-bool" },
        }),
    )
    .await;
    assert!(
        ack.get("result").is_some(),
        "detach must ack even on a malformed params body: {ack}"
    );

    assert!(
        poll_carrier_retracted(&lb.fake, carrier_uri).await,
        "a MALFORMED-params `verter/detach` must FAIL CLOSED — retract the session's open \
         carriers via the unified drain (didClose); a malformed body may not leak a stale \
         Verter overlay into the editor's own tsgo Program"
    );

    assert!(
        tokio::time::timeout(Duration::from_millis(200), lb.relay.wait_stopped())
            .await
            .is_err(),
        "a malformed-params detach must be NON-DESTRUCTIVE — the relay stays alive"
    );

    lb.relay.shutdown().await;
}

/// The session-end carrier drain is BOUNDED even against a WEDGED writer: with far more
/// open carriers than the injection channel's outbound capacity and a peer that never
/// accepts a `didClose`, `retract_open_carriers` must RETURN within its overall drain
/// budget — never block teardown indefinitely — and `opened_carriers` must be emptied
/// (drained up front) regardless of how many closes were actually delivered.
///
/// The wedge: the relay's `server_writer_task` writes to a 1-byte server pipe that is
/// NEVER drained, so it parks on `write_all` after one byte; the 256-slot `server_tx` mpsc
/// then fills and every further `didClose` send BLOCKS. Both duplex endpoints are kept
/// ALIVE (bound, not dropped) so the channels stay OPEN (wedged), not closed.
///
/// This is non-vacuous: an UNBOUNDED drain — each `did_close` awaited with NO overall budget —
/// blocks forever against a wedged writer, so the outer bound below would fire and the `is_ok()`
/// assertion would fail; the bounded drain instead returns within its budget, which the
/// `is_ok()` + elapsed assertions below verify.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_end_drain_is_bounded_against_a_wedged_writer() {
    let (_editor_endpoint, relay_editor) = tokio::io::duplex(64 * 1024);
    let (_server_endpoint, relay_server) = tokio::io::duplex(1);
    let (er, ew) = tokio::io::split(relay_editor);
    let (sr, sw) = tokio::io::split(relay_server);
    let relay = Arc::new(LspRelay::start(er, ew, sr, sw));

    let mut server = ControlServer::new(Arc::clone(&relay), "n", 1, 1, "ctl");
    // Far more carriers than the 256-slot mpsc + the one frame the parked writer holds, so
    // the drain is guaranteed to block on a `didClose` send partway through.
    for i in 0..512 {
        server.opened_carriers.insert(format!("file:///w/c{i}.ts"));
    }
    assert_eq!(server.opened_carriers.len(), 512);

    // Drive a SHORT overall budget so the bounded drain returns fast; the outer bound is
    // generous but strictly below the 10s production default, so an UNBOUNDED drain (or one
    // that ignores its budget) trips it.
    let budget = Duration::from_millis(300);
    let start = std::time::Instant::now();
    let outcome = tokio::time::timeout(
        Duration::from_secs(3),
        server.retract_open_carriers_within(budget),
    )
    .await;
    let elapsed = start.elapsed();

    assert!(
        outcome.is_ok(),
        "the session-end drain must RETURN bounded against a wedged writer — never block \
         teardown indefinitely (an unbounded drain would hang and trip this outer bound)"
    );
    assert!(
        elapsed < Duration::from_secs(2),
        "the drain respects its overall budget (~{budget:?}), not the 10s default; elapsed {elapsed:?}"
    );
    assert!(
        server.opened_carriers.is_empty(),
        "the open set is drained UP FRONT, so it is empty after the bounded drain regardless \
         of how many closes reached the wedged writer"
    );

    relay.shutdown().await;
}

/// Characterization / regression guard: the unified session-end drain retracts EVERY
/// still-tracked carrier — BOTH a FAILED-CLOSE residual AND a NEVER-CLOSED carrier — by emitting
/// one `didClose` per still-tracked URI in a single pass (cross-carrier coverage).
///
/// The invariant this locks (all in `server.rs`):
///   * `handle_carrier_did_close` removes a URI from `opened_carriers` ONLY on a successful
///     `relay.did_close` ack. A close that does not confirm returns an ERROR frame and leaves
///     the URI TRACKED — a failed-close residual.
///   * `retract_open_carriers` drains `opened_carriers` and issues one best-effort `didClose`
///     per still-tracked URI. The drain sees only the `HashSet<String>` of tracked URIs and does
///     not branch on WHY a URI is tracked, so a failed-close residual and a never-closed carrier
///     are identical drain inputs.
///
/// The proof runs in two parts because the residual precondition and the emission observation
/// cannot share one transport. `relay.did_close` is send-only and fails ONLY when the
/// server-side wire is down (its sole send-failure mode): a shim-side residual can form ONLY on
/// a down wire, yet on a down wire the drain's `didClose`s cannot reach tsgo, so emission is
/// unobservable there. Emission is observable only on a LIVE wire, where every close acks and no
/// residual can form.
///   * Part A (down wire) establishes the MEMBERSHIP precondition: an unconfirmed close leaves
///     the residual A tracked, the never-closed B stays tracked, and both are the exact set the
///     drain iterates. Draining that set to empty is the no-op-retract discriminator.
///   * Part B (fresh live wire) establishes EMISSION: seeded with the same two URIs, the drain
///     emits an observable `didClose` for BOTH, recorded by the fake tsgo.
///
/// Together they characterize cross-carrier drain emission over the failed-close-residual and
/// never-closed membership. A no-op `retract_open_carriers` leaves Part A's set populated and
/// sends nothing in Part B; a drain that clears the set without emitting (`opened_carriers`
/// drained but no `didClose` sent) still empties Part A's set yet sends nothing in Part B — so
/// the emission polls are what make this non-vacuous beyond mere set-clearing.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn session_end_drain_retracts_failed_close_residual_and_never_closed_carrier() {
    // ===== Part A — down-wire residual precondition (MEMBERSHIP) =====
    // A LIVE relay + fake tsgo: open two real carrier overlays first.
    let (editor_endpoint, relay_editor) = tokio::io::duplex(64 * 1024);
    let (server_endpoint, relay_server) = tokio::io::duplex(64 * 1024);
    let (er, ew) = tokio::io::split(relay_editor);
    let (sr, sw) = tokio::io::split(relay_server);
    let relay = Arc::new(LspRelay::start(er, ew, sr, sw));
    let fake = spawn_fake_tsgo(server_endpoint);
    // Keep the editor side open so the editor→server pump sees no EOF (relay stays alive
    // while the two carriers are opened).
    let _editor_keepalive = editor_endpoint;
    let mut server = ControlServer::new(Arc::clone(&relay), "n", 1, 1, "ctl");

    let uri_a = "file:///w/src/A.vue.tsx";
    let uri_b = "file:///w/src/B.vue.tsx";
    let open_params = |uri: &str| {
        serde_json::json!({
            "uri": uri, "languageId": "typescript", "version": 1,
            "text": "export const x = 1;",
        })
    };

    // Both carriers open as real live overlays: tracked in `opened_carriers` AND received by
    // the real tsgo.
    let ack_a = tokio::time::timeout(
        Duration::from_secs(5),
        server.handle_carrier_did_open_synced(&serde_json::json!(10), open_params(uri_a)),
    )
    .await
    .expect("open A resolves within bound");
    assert!(
        decode_frame(&ack_a).get("result").is_some(),
        "open A must ack"
    );
    let ack_b = tokio::time::timeout(
        Duration::from_secs(5),
        server.handle_carrier_did_open_synced(&serde_json::json!(11), open_params(uri_b)),
    )
    .await
    .expect("open B resolves within bound");
    assert!(
        decode_frame(&ack_b).get("result").is_some(),
        "open B must ack"
    );
    assert!(
        server.opened_carriers.contains(uri_a) && server.opened_carriers.contains(uri_b),
        "both opens are tracked as live overlays before any close"
    );
    {
        let opened = &fake.lock().unwrap().opened;
        assert!(
            opened.iter().any(|u| u == uri_a) && opened.iter().any(|u| u == uri_b),
            "both carriers reached the real tsgo as live overlays"
        );
    }

    // Bring the server-side wire DOWN so `relay.did_close` fails — the only condition under
    // which a per-carrier close does not confirm and leaves a shim-side residual (a live wire
    // always acks and removes).
    relay.shutdown().await;
    let mut wire_down = false;
    for _ in 0..200 {
        if relay
            .injection_channel()
            .did_close("file:///__probe__")
            .await
            .is_err()
        {
            wire_down = true;
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
    assert!(
        wire_down,
        "the server-side wire must be down so `relay.did_close` fails — the residual precondition"
    );

    // A per-carrier close for A that does NOT confirm: `relay.did_close` errors, so
    // `handle_carrier_did_close` returns an ERROR frame and does NOT remove A.
    let close_frame = server
        .handle_carrier_did_close(&serde_json::json!(99), serde_json::json!({ "uri": uri_a }))
        .await;
    let close_value = decode_frame(&close_frame);
    assert!(
        close_value.get("error").is_some() && close_value.get("result").is_none(),
        "a close whose `relay.did_close` failed must return an ERROR frame, never an ack: \
         {close_value}"
    );
    assert!(
        server.opened_carriers.contains(uri_a),
        "handle_carrier_did_close removes ONLY on a successful ack — an unconfirmed close leaves \
         A TRACKED (the membership residual)"
    );

    // B was never closed. Both the residual A and the never-closed B are still tracked — the
    // exact set the session-end drain iterates to emit one `didClose` per URI.
    assert!(
        server.opened_carriers.contains(uri_a) && server.opened_carriers.contains(uri_b),
        "the residual A and the never-closed B are both still tracked when the drain starts"
    );
    let drain_didclose_targets: Vec<String> = server.opened_carriers.iter().cloned().collect();
    assert!(
        drain_didclose_targets.iter().any(|u| u == uri_a),
        "the failed-close residual A is in the drain's didClose emission set"
    );
    assert!(
        drain_didclose_targets.iter().any(|u| u == uri_b),
        "the never-closed B is in the drain's didClose emission set"
    );

    // No-op-retract discriminator: draining the tracked set to empty proves the drain ran over
    // every still-tracked URI. On this down wire the emitted `didClose`s cannot reach tsgo, so
    // emission itself is proven in Part B; here we characterize only that the drain consumed the
    // whole set (a no-op `retract_open_carriers` leaves it populated).
    server.retract_open_carriers().await;
    assert!(
        server.opened_carriers.is_empty(),
        "the session-end drain consumes EVERY still-tracked carrier — BOTH the failed-close \
         residual A AND the never-closed B are drained from tracking on transport close"
    );

    // ===== Part B — live-wire cross-carrier EMISSION =====
    // A FRESH live relay + fake + server. Part A proved the failed-close residual A and the
    // never-closed B are identical drain inputs (URIs in `opened_carriers`), so seeding the same
    // two URIs directly reproduces that exact set. On this live wire the drain's `didClose` per
    // URI is observable: the fake tsgo records both retractions.
    let (editor_endpoint2, relay_editor2) = tokio::io::duplex(64 * 1024);
    let (server_endpoint2, relay_server2) = tokio::io::duplex(64 * 1024);
    let (er2, ew2) = tokio::io::split(relay_editor2);
    let (sr2, sw2) = tokio::io::split(relay_server2);
    let relay2 = Arc::new(LspRelay::start(er2, ew2, sr2, sw2));
    let fake2 = spawn_fake_tsgo(server_endpoint2);
    let _editor_keepalive2 = editor_endpoint2;
    let mut server2 = ControlServer::new(Arc::clone(&relay2), "n", 1, 1, "ctl");

    server2.opened_carriers.insert(uri_a.to_string());
    server2.opened_carriers.insert(uri_b.to_string());

    server2.retract_open_carriers().await;

    assert!(
        poll_carrier_retracted(&fake2, uri_a).await,
        "the drain must EMIT a `didClose` for the failed-close residual A — observed by the real \
         tsgo, not merely cleared from the tracked set"
    );
    assert!(
        poll_carrier_retracted(&fake2, uri_b).await,
        "the drain must EMIT a `didClose` for the never-closed carrier B — observed by the real \
         tsgo, not merely cleared from the tracked set"
    );
    assert!(
        server2.opened_carriers.is_empty(),
        "the drain empties the tracked set after emitting both cross-carrier `didClose`s"
    );

    relay2.shutdown().await;
}

/// Saturate the relay's outbound `server_tx` mpsc against a WEDGED writer so the NEXT carrier
/// notification send PARKS (the wedge the bounded handler must survive). Sends `didClose`
/// notifications until a bounded probe send fails to complete within its short probe window —
/// at which point the 256-slot channel is full and any further send parks (the writer is parked
/// on a 1-byte, never-drained server pipe, so it never frees a slot). Bounded to 1024 sends so a
/// mis-set-up wedge fails loudly rather than looping forever.
async fn saturate_wedged_server_channel(relay: &Arc<LspRelay>) {
    for _ in 0..1024 {
        if tokio::time::timeout(
            Duration::from_millis(50),
            relay.injection_channel().did_close("file:///w/saturate.ts"),
        )
        .await
        .is_err()
        {
            return; // the outbound channel is full — a further send now parks (wedged)
        }
    }
    panic!("failed to saturate the wedged server channel within 1024 sends");
}

/// A freshly WEDGED relay for a bounded-handler arm: its outbound `server_tx` mpsc is SATURATED
/// (the next relay send parks) and its server writer is parked on a never-drained 1-byte server
/// pipe. Returns the relay plus BOTH duplex peer endpoints — the caller binds the endpoints ALIVE
/// (a dropped peer would CLOSE the pipe and make a send ERROR instead of park, a different failure
/// mode) and `shutdown`s the relay at the end of the arm.
async fn wedged_relay_with_saturated_writer() -> (Arc<LspRelay>, DuplexStream, DuplexStream) {
    let (editor_endpoint, relay_editor) = tokio::io::duplex(64 * 1024);
    let (server_endpoint, relay_server) = tokio::io::duplex(1);
    let (er, ew) = tokio::io::split(relay_editor);
    let (sr, sw) = tokio::io::split(relay_server);
    let relay = Arc::new(LspRelay::start(er, ew, sr, sw));
    // Fill the outbound channel so the NEXT relay send — a handler's `didOpen`/`didChange`/
    // `didClose` NOTIFICATION or the `custom/initializeAPISession` REQUEST — parks on the wedge.
    saturate_wedged_server_channel(&relay).await;
    (relay, editor_endpoint, server_endpoint)
}

/// EVERY relay-round-trip control handler BOUNDS its relay send against a WEDGED writer (a full
/// outbound mpsc whose server writer is parked on `write_all`), so a wedged writer can never PIN
/// the serial read→dispatch serve loop inside a handler. An UNBOUNDED handler send would block the
/// serve loop forever — the loop would never observe EOF/detach and never reach the session-end
/// drain, so the drain's own bound would be moot. This INDEPENDENTLY drives all FOUR
/// relay-round-trip handlers against a freshly saturated wedged relay and asserts each RETURNS a
/// fail-closed error frame within its SHORT injected `carrier_op_bound` (never the 10s default):
///   * `handle_carrier_did_open_synced` — the `didOpen` NOTIFICATION send; a bounded-out open is
///     POSSIBLY-LIVE, so it is tracked for the session-end drain.
///   * `handle_carrier_did_change_synced` — the `didChange` NOTIFICATION send; no tracking change.
///   * `handle_carrier_did_close` — the `didClose` NOTIFICATION send; the URI STAYS tracked (only a
///     confirmed close removes it) and the session-end drain then clears it.
///   * `handle_initialize_api_session` — the `custom/initializeAPISession` REQUEST round-trip; a
///     bounded-out initialize must NOT set `api_session_active`.
///
/// The wedge: the relay's server writer writes to a 1-byte server pipe that is NEVER drained, so it
/// parks after one byte and the 256-slot `server_tx` mpsc fills; a further send/request then parks.
/// A SHORT injected `carrier_op_bound` makes each internal timeout observable fast.
///
/// Discriminator (per arm): with THAT handler's own send left UNBOUNDED the wedged channel blocks
/// the handler forever, so the arm's outer bound fires and its `expect` fails (the handler never
/// returns). With the bound the handler returns a fail-closed error frame within the short bound.
#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn relay_round_trip_handlers_are_bounded_against_a_wedged_writer() {
    // didOpenSynced: the `didOpen` NOTIFICATION send is bounded; a bounded-out open is tracked.
    {
        let (relay, _editor_endpoint, _server_endpoint) =
            wedged_relay_with_saturated_writer().await;
        let mut server = ControlServer::new(Arc::clone(&relay), "n", 1, 1, "ctl");
        server.carrier_op_bound = Duration::from_millis(300);
        let uri = "file:///w/src/OpenWedged.vue.tsx";

        let start = std::time::Instant::now();
        let outcome = tokio::time::timeout(
            Duration::from_secs(3),
            server.handle_carrier_did_open_synced(
                &serde_json::json!(1),
                serde_json::json!({
                    "uri": uri, "languageId": "typescript", "version": 1, "text": "",
                }),
            ),
        )
        .await;
        let elapsed = start.elapsed();
        let frame = outcome.expect(
            "handle_carrier_did_open_synced must RETURN bounded against a wedged writer — never pin \
             the serial serve loop (an unbounded send hangs and this outer bound fires)",
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "didOpenSynced honoured its SHORT ~300ms bound, not the 10s default; elapsed {elapsed:?}"
        );
        let value = decode_frame(&frame);
        assert!(
            value.get("error").is_some() && value.get("result").is_none(),
            "a wedged/timed-out didOpenSynced must return an ERROR frame, never an ack: {value}"
        );
        // A bounded-out open MAY have reached tsgo (POSSIBLY-LIVE), so it is tracked for the drain.
        assert!(
            server.opened_carriers.contains(uri),
            "a bounded-out didOpen tracks the possibly-live overlay for the session-end drain"
        );
        relay.shutdown().await;
    }

    // didChangeSynced: the `didChange` NOTIFICATION send is bounded; makes no tracking change.
    {
        let (relay, _editor_endpoint, _server_endpoint) =
            wedged_relay_with_saturated_writer().await;
        let mut server = ControlServer::new(Arc::clone(&relay), "n", 1, 1, "ctl");
        server.carrier_op_bound = Duration::from_millis(300);
        let uri = "file:///w/src/ChangeWedged.vue.tsx";

        let start = std::time::Instant::now();
        let outcome = tokio::time::timeout(
            Duration::from_secs(3),
            server.handle_carrier_did_change_synced(
                &serde_json::json!(2),
                serde_json::json!({ "uri": uri, "version": 2, "text": "" }),
            ),
        )
        .await;
        let elapsed = start.elapsed();
        let frame = outcome.expect(
            "handle_carrier_did_change_synced must RETURN bounded against a wedged writer — never \
             pin the serial serve loop (an unbounded send hangs and this outer bound fires)",
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "didChangeSynced honoured its SHORT ~300ms bound, not the 10s default; elapsed {elapsed:?}"
        );
        let value = decode_frame(&frame);
        assert!(
            value.get("error").is_some() && value.get("result").is_none(),
            "a wedged/timed-out didChangeSynced must return an ERROR frame, never an ack: {value}"
        );
        relay.shutdown().await;
    }

    // didClose: the `didClose` NOTIFICATION send is bounded; the URI STAYS tracked; the drain
    // clears it.
    {
        let (relay, _editor_endpoint, _server_endpoint) =
            wedged_relay_with_saturated_writer().await;
        let mut server = ControlServer::new(Arc::clone(&relay), "n", 1, 1, "ctl");
        server.carrier_op_bound = Duration::from_millis(300);
        let uri = "file:///w/src/Wedged.vue.tsx";
        server.opened_carriers.insert(uri.to_string());

        let start = std::time::Instant::now();
        let outcome = tokio::time::timeout(
            Duration::from_secs(3),
            server
                .handle_carrier_did_close(&serde_json::json!(3), serde_json::json!({ "uri": uri })),
        )
        .await;
        let elapsed = start.elapsed();
        let frame = outcome.expect(
            "handle_carrier_did_close must RETURN bounded against a wedged writer — never pin the \
             serial serve loop (an unbounded send hangs and this outer bound fires)",
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "didClose honoured its SHORT ~300ms bound, not the 10s default; elapsed {elapsed:?}"
        );
        let value = decode_frame(&frame);
        assert!(
            value.get("error").is_some() && value.get("result").is_none(),
            "a wedged/timed-out didClose must return an ERROR frame, never an ack: {value}"
        );
        // Only a confirmed (`Ok(Ok(()))`) close removes the URI, so a bounded-out close LEAVES it
        // tracked for the session-end drain — a handler that removed on timeout/failure would drop
        // it prematurely.
        assert!(
            server.opened_carriers.contains(uri),
            "a bounded-out close must LEAVE the URI tracked (only a confirmed close removes it) so \
             the session-end drain retracts it"
        );
        // The session-end drain then clears the tracking set (drained up front, itself bounded).
        tokio::time::timeout(
            Duration::from_secs(3),
            server.retract_open_carriers_within(Duration::from_millis(300)),
        )
        .await
        .expect("the session-end drain must itself return bounded against the wedged channel");
        assert!(
            server.opened_carriers.is_empty(),
            "the bounded session-end drain clears the still-tracked URI from the open set"
        );
        relay.shutdown().await;
    }

    // initializeApiSession: the `custom/initializeAPISession` REQUEST round-trip is bounded; a
    // bounded-out initialize must NOT mark the session active.
    {
        let (relay, _editor_endpoint, _server_endpoint) =
            wedged_relay_with_saturated_writer().await;
        let mut server = ControlServer::new(Arc::clone(&relay), "n", 1, 1, "ctl");
        server.carrier_op_bound = Duration::from_millis(300);

        let start = std::time::Instant::now();
        let outcome = tokio::time::timeout(
            Duration::from_secs(3),
            server.handle_initialize_api_session(&serde_json::json!(4)),
        )
        .await;
        let elapsed = start.elapsed();
        let frame = outcome.expect(
            "handle_initialize_api_session must RETURN bounded against a wedged writer — never pin \
             the serial serve loop (an unbounded round-trip hangs and this outer bound fires)",
        );
        assert!(
            elapsed < Duration::from_secs(2),
            "initializeApiSession honoured its SHORT ~300ms bound, not the 10s default; elapsed {elapsed:?}"
        );
        let value = decode_frame(&frame);
        assert!(
            value.get("error").is_some() && value.get("result").is_none(),
            "a wedged/timed-out initializeApiSession must return an ERROR frame, never a result: {value}"
        );
        // A bounded-out initialize never minted a session, so it must NOT mark it active.
        assert!(
            !server.api_session_active,
            "a bounded-out initializeApiSession must NOT set api_session_active (no session minted)"
        );
        relay.shutdown().await;
    }
}
