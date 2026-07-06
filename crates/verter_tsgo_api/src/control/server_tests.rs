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
/// the overlay open but left the sync barrier unanswered — the F2 sent-but-unsynced
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

/// E4: an ABNORMAL control-session termination — the control pipe dropped WITHOUT a
/// `verter/detach` (EOF on the server read) — must STILL retract the session's still-open
/// carrier overlays (send `didClose` to the real tsgo), so no stale Verter overlay lingers
/// in the editor's own tsgo Program. NON-DESTRUCTIVE: the retraction touches Verter's own
/// overlays only; the shim's relay (editor↔tsgo path + its OWNED tsgo child) stays ALIVE.
///
/// RED before the fix: only an explicit `verter/detach` drained the session's carriers, so
/// an EOF / dropped-pipe termination left the overlays OPEN — the fake tsgo never saw the
/// `didClose`.
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

/// F2: a carrier whose `didOpen` was SENT to the real tsgo but whose sync BARRIER never
/// completed (the editor tsgo received the overlay open but never answered the
/// pull-diagnostic barrier — a slow/broken engine, bounded by `CARRIER_SYNC_BARRIER_TIMEOUT`)
/// MUST STILL be retracted by the session-end drain. The overlay is already LIVE in the
/// editor's own tsgo Program the moment the `didOpen` is sent (`relay::did_open` tracks it
/// before the barrier), so an un-retracted sent-but-unsynced open leaks a stale Verter
/// overlay into the editor's Program until editor teardown.
///
/// RED before the fix: the control session recorded a carrier as retract-eligible ONLY on
/// the FULL synced-open success (`did_open_synced` returned Ok), so a barrier timeout left
/// the carrier UNTRACKED and the session-end drain sent NO `didClose` for it — the leak.
/// After the fix the carrier is tracked at `didOpen`-SEND time (before the barrier), so the
/// drain retracts it on every termination mode.
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

/// H1: an explicit `verter/detach` with an OMITTED params body (`{}`) FAILS CLOSED — it
/// retracts the session's open carriers through the unified session-end drain (a
/// `didClose` to the real tsgo), NON-DESTRUCTIVELY. Only an EXPLICIT `closeCarriers:
/// false` opts out; an omitted/unspecified preference must NOT leave a stale Verter
/// overlay in the editor's own tsgo Program.
///
/// RED before the fix: `handle_detach` deserialized into a `bool` whose `Default` is
/// `false` and `unwrap_or_default()`-ed, so an omitted param set
/// `retract_carriers_on_end = false` and the drain was SKIPPED — the fake tsgo never saw
/// the `didClose` (the leak). After the fix an omitted param reads as `None`, which
/// retracts.
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

/// H1: an explicit `verter/detach` with a MALFORMED params body (`closeCarriers` is not a
/// bool) FAILS CLOSED — it retracts the session's open carriers through the unified
/// session-end drain, NON-DESTRUCTIVELY. A malformed body is treated as unspecified, so
/// it must NOT leave a stale Verter overlay in the editor's own tsgo Program.
///
/// RED before the fix: `unwrap_or_default()` mapped the malformed body to
/// `close_carriers = false`, skipping the drain (the leak). After the fix a malformed
/// body maps to `None`, which retracts.
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
