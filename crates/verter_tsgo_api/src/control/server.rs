//! The shim-side control server: dispatches the `verter/*` control methods to a
//! live [`LspRelay`]'s GATED ops.
//!
//! The server is DUMB by contract: every carrier write it performs goes through
//! [`LspRelay::injection_channel`] (the deny-by-default gate) — it never
//! touches a raw wire, never mutates the egress leak policy, and holds no
//! semantic TS/Vue/Svelte service. It maps control methods to relay ops:
//! `verter/hello` runs the version+nonce gate; `verter/waitInitialized` awaits
//! the relay's in-band initialize witness; the carrier lifecycle methods drive
//! the injection channel; `verter/initializeApiSession` re-emits the `--api`
//! session mint and returns the server-minted pipe/UDS path; `verter/detach`
//! closes THIS control connection ONLY — a NON-DESTRUCTIVE detach that drops the
//! Verter pipe while the shim's editor↔tsgo relay and its OWNED tsgo child stay
//! alive (Verter never terminates an engine it did not spawn). The shim tears its
//! owned child down only on editor disconnect / real-tsgo exit — never on a Verter
//! control detach.
//!
//! Carrier-overlay retraction is UNIFIED into ONE session-end drain: on ANY
//! control-session end — a clean `verter/detach` AND every abnormal termination
//! (EOF / malformed frame / outbound failure / control-pipe drop without detach) —
//! every carrier this session sent a `didOpen` for and has not yet closed is
//! retracted (a `didClose` to the real tsgo through the gated channel). A carrier is
//! retract-eligible from `didOpen`-SEND time, NOT only once its sync barrier
//! completes, so a sent-but-unsynced open (a barrier timeout, or an abnormal session
//! end while the barrier is still in flight) cannot leave a stale Verter overlay in
//! the editor's own tsgo Program. This is overlays-only and stays non-destructive;
//! only an explicit `verter/detach` with `closeCarriers: false` opts out of the drain.
//!
//! The drain fires on every termination mode because the read→dispatch loop breaks to
//! it on EOF, a malformed frame, an outbound-send failure, and `verter/detach` alike;
//! the only in-flight handler that can delay it — the carrier sync barrier — is BOUNDED
//! (`CARRIER_SYNC_BARRIER_TIMEOUT`), so an unanswered barrier delays the drain by at most
//! that bound, never indefinitely.

use std::collections::HashSet;
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::sync::mpsc;

use crate::error::TsgoApiError;
use crate::jsonrpc::framing::{encode_message, MessageFramer};
use crate::relay::LspRelay;

use super::messages::{
    self, verify_hello, ControlAck, ControlCapabilities, FatalParams, FatalReason, HelloParams,
    HelloResult, InitializeApiSessionResult, StatusResult, WaitInitializedResult,
    ERROR_CONTROL_OP_FAILED, ERROR_MALFORMED_PAYLOAD, ERROR_NOT_AUTHENTICATED, METHOD_FATAL,
    PROTOCOL_VERSION,
};

/// JSON-RPC "method not found" error code (per the JSON-RPC 2.0 spec).
const ERROR_METHOD_NOT_FOUND: i64 = -32601;

/// The shim-side control server for ONE control connection. Holds per-connection
/// state (hello completion, the carriers this session opened) plus the shared
/// relay + rendezvous witnesses. A fresh instance is served per accepted
/// connection; the relay is shared across them.
pub struct ControlServer {
    relay: Arc<LspRelay>,
    expected_nonce: String,
    editor_session_generation: u64,
    wire_pin: u64,
    session_id: String,
    hello_completed: bool,
    api_session_active: bool,
    /// The carriers this session has a LIVE `didOpen` overlay for in the editor's tsgo
    /// Program: a "sent-open, not-yet-closed" set. A URI is inserted the moment its
    /// `didOpen` is dispatched to the real tsgo (BEFORE the sync barrier) and removed on a
    /// `didClose`, so a sent-but-unsynced open (barrier timeout / abnormal session end mid
    /// barrier) is still retracted by the session-end drain and cannot leak.
    opened_carriers: HashSet<String>,
    /// Whether the unified session-end drain retracts this session's still-open carrier
    /// overlays. Defaults to `true`, so EVERY termination mode — a clean `verter/detach`
    /// AND every abnormal path (EOF / malformed frame / outbound failure / control-pipe
    /// drop without detach) — retracts. Only an explicit `verter/detach` with
    /// `closeCarriers: false` opts OUT (the client deliberately leaves its overlays).
    retract_carriers_on_end: bool,
}

impl ControlServer {
    /// Assemble a control server over a shared relay + the shim's rendezvous
    /// witnesses. `verter/detach` closes only THIS control connection — it never
    /// signals the shim to tear down (a non-destructive detach; the shim owns its
    /// child's lifecycle and kills it only on editor disconnect / real-tsgo exit).
    #[must_use]
    pub fn new(
        relay: Arc<LspRelay>,
        expected_nonce: impl Into<String>,
        editor_session_generation: u64,
        wire_pin: u64,
        session_id: impl Into<String>,
    ) -> Self {
        Self {
            relay,
            expected_nonce: expected_nonce.into(),
            editor_session_generation,
            wire_pin,
            session_id: session_id.into(),
            hello_completed: false,
            api_session_active: false,
            opened_carriers: HashSet::new(),
            retract_carriers_on_end: true,
        }
    }

    /// Serve control requests over one connection until the client disconnects
    /// or the relay stops. Runs a serialized read→dispatch→respond loop (the
    /// client drives request/response, so serial dispatch preserves order) plus
    /// a concurrent `verter/fatal` emitter that fires when the relay stops.
    pub async fn serve<R, W>(mut self, mut read: R, write: W)
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>(64);
        let writer = tokio::spawn(control_writer_task(write, out_rx));

        // Concurrent fatal emitter: when the relay stops (engine exit / editor
        // disconnect), notify the control client so it can fail the whole
        // shared component over rather than hang.
        let fatal_tx = out_tx.clone();
        let relay_for_fatal = Arc::clone(&self.relay);
        let fatal_task = tokio::spawn(async move {
            relay_for_fatal.wait_stopped().await;
            let notification = fatal_notification(FatalReason::RelayDeath, "relay stopped pumping");
            let _ = fatal_tx.send(encode_message(&notification)).await;
        });

        let mut framer = MessageFramer::new();
        let mut chunk = [0u8; 8192];
        'read: loop {
            let n = match read_chunk(&mut read, &mut chunk).await {
                Some(n) => n,
                None => break,
            };
            framer.push(&chunk[..n]);
            loop {
                match framer.next_message() {
                    Ok(Some(msg)) => {
                        let (response, end_session) = self.dispatch(&msg).await;
                        if let Some(bytes) = response {
                            if out_tx.send(bytes).await.is_err() {
                                break 'read;
                            }
                        }
                        if end_session {
                            // `verter/detach`: the response is queued on the ordered
                            // writer; close THIS control connection (drop the Verter
                            // pipe). The shim's editor↔tsgo relay AND its OWNED tsgo
                            // child stay ALIVE — a non-destructive detach. The shim
                            // tears its owned child down only on editor disconnect /
                            // real-tsgo exit, never on a Verter control detach.
                            break 'read;
                        }
                    }
                    Ok(None) => break,
                    // A malformed control frame is unrecoverable on a framed
                    // stream: fail closed, stop serving.
                    Err(_) => break 'read,
                }
            }
        }

        // The UNIFIED session-end overlay drain: on ANY control-session end — a clean
        // `verter/detach` AND every abnormal termination (EOF / malformed frame / outbound
        // failure / control-pipe drop WITHOUT detach) — retract this session's still-open
        // carrier overlays so no stale Verter overlay lingers in the editor's own tsgo
        // Program. `retract_carriers_on_end` honors an explicit
        // `detach(closeCarriers: false)` opt-out; it defaults to `true`, so an abnormal
        // termination (which carries no detach signal) always retracts. NON-DESTRUCTIVE:
        // overlays only — never a shim/child teardown (the shim owns its OWNED tsgo child's
        // lifecycle and tears it down only on editor disconnect / real-tsgo exit).
        if self.retract_carriers_on_end {
            self.retract_open_carriers().await;
        }

        fatal_task.abort();
        drop(out_tx);
        let _ = writer.await;
    }

    /// Retract EXACTLY the carriers this control session sent a `didOpen` for — the
    /// "sent-open, not-yet-closed" set, which includes a sent-but-unsynced open whose sync
    /// barrier never completed — through the gated injection channel (best-effort — a closed
    /// peer must not fail teardown). Drains the open set so a repeat (e.g. the session-end
    /// drain after a clean detach) is a no-op. NON-DESTRUCTIVE: sends `didClose` to the real
    /// tsgo for Verter's OWN overlays only — never `exit`/`shutdown`, never a shim/child
    /// teardown (Verter must not terminate an engine it did not spawn). The SINGLE drain
    /// path every session-end mode shares.
    async fn retract_open_carriers(&mut self) {
        let channel = self.relay.injection_channel();
        let uris: Vec<String> = self.opened_carriers.drain().collect();
        for uri in uris {
            let _ = channel.did_close(&uri).await;
        }
    }

    /// Dispatch one decoded control message. Returns the response frame bytes to
    /// send (a notification has none) plus whether THIS control connection should
    /// END after the response — raised ONLY by `verter/detach`, which drops the
    /// Verter pipe non-destructively (it never tears the shim or its owned tsgo
    /// child down).
    async fn dispatch(&mut self, msg: &serde_json::Value) -> (Option<Vec<u8>>, bool) {
        let id = msg.get("id").cloned();
        let method = msg.get("method").and_then(|m| m.as_str());
        let params = msg
            .get("params")
            .cloned()
            .unwrap_or(serde_json::Value::Null);

        // A frame with no id is a notification (the client sends none in the
        // stable contract): acknowledge nothing.
        let Some(id) = id.filter(|v| !v.is_null()) else {
            return (None, false);
        };
        let Some(method) = method else {
            return (
                Some(err_frame(
                    &id,
                    ERROR_MALFORMED_PAYLOAD,
                    "request has no method",
                )),
                false,
            );
        };

        match method {
            messages::METHOD_HELLO => (Some(self.handle_hello(&id, params)), false),
            // Every other method requires a completed hello (fail closed).
            _ if !self.hello_completed => (
                Some(err_frame(
                    &id,
                    ERROR_NOT_AUTHENTICATED,
                    "verter/hello must complete before any other control method",
                )),
                false,
            ),
            messages::METHOD_WAIT_INITIALIZED => {
                (Some(self.handle_wait_initialized(&id).await), false)
            }
            messages::METHOD_CARRIER_DID_OPEN_SYNCED => (
                Some(self.handle_carrier_did_open_synced(&id, params).await),
                false,
            ),
            messages::METHOD_CARRIER_DID_CHANGE_SYNCED => (
                Some(self.handle_carrier_did_change_synced(&id, params).await),
                false,
            ),
            messages::METHOD_CARRIER_DID_CLOSE => (
                Some(self.handle_carrier_did_close(&id, params).await),
                false,
            ),
            messages::METHOD_INITIALIZE_API_SESSION => {
                (Some(self.handle_initialize_api_session(&id).await), false)
            }
            messages::METHOD_STATUS => (Some(self.handle_status(&id)), false),
            messages::METHOD_DETACH => {
                // Record the carrier-retraction preference + END this control connection
                // only; the UNIFIED session-end drain performs the retraction
                // (non-destructive — never a shim/child teardown).
                let frame = self.handle_detach(&id, params);
                (Some(frame), true)
            }
            other => (
                Some(err_frame(
                    &id,
                    ERROR_METHOD_NOT_FOUND,
                    &format!("unknown control method {other:?}"),
                )),
                false,
            ),
        }
    }

    fn handle_hello(&mut self, id: &serde_json::Value, params: serde_json::Value) -> Vec<u8> {
        let params: HelloParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => return err_frame(id, ERROR_MALFORMED_PAYLOAD, &format!("hello params: {e}")),
        };
        match verify_hello(&params, &self.expected_nonce) {
            Ok(()) => {
                self.hello_completed = true;
                let result = HelloResult {
                    protocol: PROTOCOL_VERSION,
                    session_id: self.session_id.clone(),
                    wire_pin: self.wire_pin,
                    editor_session_generation: self.editor_session_generation,
                    capabilities: ControlCapabilities {
                        carrier_injection: true,
                        api_session: true,
                        wait_initialized: true,
                    },
                };
                ok_frame(id, &result)
            }
            Err(rejection) => err_frame(id, rejection.error_code(), &rejection.message()),
        }
    }

    async fn handle_wait_initialized(&self, id: &serde_json::Value) -> Vec<u8> {
        match self.relay.wait_initialized().await {
            Some(witness) => {
                let result = WaitInitializedResult {
                    server_info_version: witness.server_info_version,
                    observed_initialize_id: witness.observed_initialize_id,
                    root_uri: witness.root_uri,
                    workspace_folders: witness.workspace_folders,
                };
                ok_frame(id, &result)
            }
            None => err_frame(
                id,
                ERROR_CONTROL_OP_FAILED,
                "the relay stopped before the editor initialize was observed",
            ),
        }
    }

    async fn handle_carrier_did_open_synced(
        &mut self,
        id: &serde_json::Value,
        params: serde_json::Value,
    ) -> Vec<u8> {
        let params: messages::CarrierDidOpenSyncedParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return err_frame(id, ERROR_MALFORMED_PAYLOAD, &format!("carrier params: {e}"))
            }
        };
        // Send the `didOpen` FIRST, then track the carrier as retract-eligible the moment it
        // reaches the real tsgo — BEFORE the sync barrier. The overlay is LIVE in the
        // editor's own tsgo Program the instant the `didOpen` is sent, so a barrier
        // timeout/failure (or an abnormal session end while the barrier is in flight) must
        // NOT strand it: the session-end drain must retract EVERY carrier a `didOpen` was
        // sent for, not only those whose barrier completed. Tracking on the full synced-open
        // success alone was the leak — a sent-but-unsynced open never entered the drain set.
        if let Err(e) = self
            .relay
            .injection_channel()
            .did_open(
                &params.uri,
                &params.language_id,
                params.version,
                &params.text,
            )
            .await
        {
            // The `didOpen` never reached tsgo (a send failure): nothing was injected, so
            // nothing is tracked and nothing can leak.
            return op_error_frame(id, "carrier didOpen", &e);
        }
        // The `didOpen` reached tsgo — record the carrier for the session-end drain BEFORE
        // awaiting the barrier.
        self.opened_carriers.insert(params.uri.clone());
        // Then the ordered sync barrier (the `didOpen` is already drained in LSP order). A
        // barrier failure is reported to the client, but the carrier stays tracked above so
        // the session-end drain still retracts it.
        match self
            .relay
            .injection_channel()
            .sync_overlay(&params.uri)
            .await
        {
            Ok(()) => ok_frame(id, &ControlAck { ok: true }),
            Err(e) => op_error_frame(id, "carrier didOpenSynced", &e),
        }
    }

    async fn handle_carrier_did_change_synced(
        &self,
        id: &serde_json::Value,
        params: serde_json::Value,
    ) -> Vec<u8> {
        let params: messages::CarrierDidChangeSyncedParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return err_frame(id, ERROR_MALFORMED_PAYLOAD, &format!("carrier params: {e}"))
            }
        };
        let channel = self.relay.injection_channel();
        // didChange then the ordered sync barrier — mirrors did_open_synced so a
        // subsequent updateSnapshot sees the updated overlay.
        let result = async {
            channel
                .did_change(&params.uri, params.version, &params.text)
                .await?;
            channel.sync_overlay(&params.uri).await
        }
        .await;
        match result {
            Ok(()) => ok_frame(id, &ControlAck { ok: true }),
            Err(e) => op_error_frame(id, "carrier didChangeSynced", &e),
        }
    }

    async fn handle_carrier_did_close(
        &mut self,
        id: &serde_json::Value,
        params: serde_json::Value,
    ) -> Vec<u8> {
        let params: messages::CarrierDidCloseParams = match serde_json::from_value(params) {
            Ok(p) => p,
            Err(e) => {
                return err_frame(id, ERROR_MALFORMED_PAYLOAD, &format!("carrier params: {e}"))
            }
        };
        let result = self.relay.injection_channel().did_close(&params.uri).await;
        match result {
            Ok(()) => {
                self.opened_carriers.remove(&params.uri);
                ok_frame(id, &ControlAck { ok: true })
            }
            Err(e) => op_error_frame(id, "carrier didClose", &e),
        }
    }

    async fn handle_initialize_api_session(&mut self, id: &serde_json::Value) -> Vec<u8> {
        match self
            .relay
            .injection_channel()
            .reinitialize_api_session()
            .await
        {
            Ok(handle) => {
                self.api_session_active = true;
                // The server mints a Windows named pipe / a Unix-domain socket;
                // report it in the matching field. The path is opaque to the
                // shim — the client connects it verbatim.
                let (pipe_name, socket_path) = if cfg!(windows) {
                    (Some(handle.pipe), None)
                } else {
                    (None, Some(handle.pipe))
                };
                let result = InitializeApiSessionResult {
                    pipe_name,
                    socket_path,
                    wire_pin: self.wire_pin,
                    handle_kind: "integer".to_string(),
                };
                ok_frame(id, &result)
            }
            Err(e) => op_error_frame(id, "initializeApiSession", &e),
        }
    }

    fn handle_status(&self, id: &serde_json::Value) -> Vec<u8> {
        let result = StatusResult {
            protocol: PROTOCOL_VERSION,
            hello_completed: self.hello_completed,
            initialized: self.relay.initialized_witness().is_some(),
            open_carriers: self.opened_carriers.len() as u32,
            api_session_active: self.api_session_active,
        };
        ok_frame(id, &result)
    }

    fn handle_detach(&mut self, id: &serde_json::Value, params: serde_json::Value) -> Vec<u8> {
        // FAIL CLOSED on an unspecified preference: an OMITTED or MALFORMED params body
        // deserializes to `close_carriers: None` (`unwrap_or_default()` maps a malformed
        // body to the `None` default), and `None` RETRACTS — only an EXPLICIT
        // `closeCarriers: false` opts out. The ACTUAL retraction runs through the UNIFIED
        // session-end drain (so a clean `verter/detach` and every abnormal termination share
        // ONE drain path). `closeCarriers: false` opts OUT (the client deliberately leaves
        // its overlays open); an omitted / malformed param — like every abnormal termination
        // — retracts.
        let params: messages::DetachParams = serde_json::from_value(params).unwrap_or_default();
        self.retract_carriers_on_end = params.close_carriers != Some(false);
        ok_frame(id, &ControlAck { ok: true })
    }
}

/// Read one chunk from the connection read half. `None` on EOF / error.
async fn read_chunk<R: AsyncRead + Unpin>(read: &mut R, chunk: &mut [u8]) -> Option<usize> {
    match read.read(chunk).await {
        Ok(0) | Err(_) => None,
        Ok(n) => Some(n),
    }
}

/// The serialized control-writer task: drains every server→client frame
/// (responses + the fatal notification) onto the connection in channel order.
async fn control_writer_task<W>(mut write: W, mut out_rx: mpsc::Receiver<Vec<u8>>)
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    use tokio::io::AsyncWriteExt;
    while let Some(bytes) = out_rx.recv().await {
        if write.write_all(&bytes).await.is_err() {
            break;
        }
        if write.flush().await.is_err() {
            break;
        }
    }
    let _ = write.shutdown().await;
}

/// Encode a JSON-RPC success response frame.
fn ok_frame<T: serde::Serialize>(id: &serde_json::Value, result: &T) -> Vec<u8> {
    let value = serde_json::to_value(result).unwrap_or(serde_json::Value::Null);
    encode_message(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "result": value,
    }))
}

/// Encode a JSON-RPC error response frame.
fn err_frame(id: &serde_json::Value, code: i64, message: &str) -> Vec<u8> {
    encode_message(&serde_json::json!({
        "jsonrpc": "2.0",
        "id": id,
        "error": { "code": code, "message": message },
    }))
}

/// Encode a control-op failure (a relay/injection error) as a JSON-RPC error.
fn op_error_frame(id: &serde_json::Value, op: &str, error: &TsgoApiError) -> Vec<u8> {
    err_frame(
        id,
        ERROR_CONTROL_OP_FAILED,
        &format!("{op} failed: {error:?}"),
    )
}

/// Build a `verter/fatal` notification frame.
fn fatal_notification(reason: FatalReason, detail: &str) -> serde_json::Value {
    let params = FatalParams {
        reason,
        detail: detail.to_string(),
    };
    serde_json::json!({
        "jsonrpc": "2.0",
        "method": METHOD_FATAL,
        "params": serde_json::to_value(params).unwrap_or(serde_json::Value::Null),
    })
}

#[cfg(test)]
#[path = "server_tests.rs"]
mod tests;
