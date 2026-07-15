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
//! Carrier-overlay retraction is UNIFIED into ONE best-effort session-end drain: on ANY
//! control-session end — a clean `verter/detach` AND every abnormal termination
//! (EOF / malformed frame / outbound failure / control-pipe drop without detach) — the server
//! ATTEMPTS to retract every carrier this session sent a `didOpen` for and has not yet closed
//! (a `didClose` to the real tsgo through the gated channel). A carrier is retract-eligible
//! from `didOpen`-SEND time, NOT only once its sync barrier completes, so a sent-but-unsynced
//! open (a barrier timeout, or an abnormal session end while the barrier is still in flight) is
//! included in the drain attempt. The drain is BOUNDED and BEST-EFFORT: delivery of any
//! individual `didClose` is NOT guaranteed — an undelivered close is abandoned when the budget
//! expires, so a stale Verter overlay MAY linger in the editor's own tsgo Program if teardown
//! is cut short. This is overlays-only and stays non-destructive; only an explicit
//! `verter/detach` with `closeCarriers: false` opts out of the drain.
//!
//! The drain fires on every termination mode because the read→dispatch loop breaks to
//! it on EOF, a malformed frame, an outbound-send failure, and `verter/detach` alike. Two
//! independent bounds keep teardown from blocking indefinitely: an in-flight carrier sync
//! barrier is bounded by `CARRIER_SYNC_BARRIER_TIMEOUT` (that const bounds `sync_overlay`
//! ONLY), and the session-end retract loop itself ([`ControlServer::retract_open_carriers`])
//! runs under ONE overall `CARRIER_SYNC_BARRIER_TIMEOUT` drain budget — so a wedged writer
//! that never accepts a `didClose` delays teardown by at most that overall budget, never
//! indefinitely. When the budget expires the drain is cancelled and any undelivered closes are
//! abandoned (the open set was drained up front, so it is empty regardless) and teardown
//! proceeds.

use std::collections::HashSet;
use std::sync::Arc;
use std::time::Duration;

use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite};
use tokio::sync::mpsc;

use crate::error::TsgoApiError;
use crate::jsonrpc::framing::{encode_message, MessageFramer};
use crate::relay::{LspRelay, CARRIER_SYNC_BARRIER_TIMEOUT};

use super::messages::{
    self, verify_hello, ControlAck, ControlCapabilities, FatalParams, FatalReason, HelloParams,
    HelloResult, InitializeApiSessionResult, StatusResult, WaitInitializedResult,
    ERROR_CONTROL_OP_FAILED, ERROR_MALFORMED_PAYLOAD, ERROR_NOT_AUTHENTICATED, METHOD_FATAL,
    PROTOCOL_VERSION,
};

/// JSON-RPC "method not found" error code (per the JSON-RPC 2.0 spec).
const ERROR_METHOD_NOT_FOUND: i64 = -32601;

/// The hard bound `verter/waitInitialized` waits for the editor→tsgo `initialize`
/// witness before returning a typed error. The editor may never send `initialize` (a
/// broken / detached engine), so the handler must never block the control dispatch
/// indefinitely — it awaits the in-band witness under this timeout AND races the
/// relay-stop signal, returning the FIRST of {witness, relay-stop, timeout}.
const WAIT_INITIALIZED_TIMEOUT: Duration = Duration::from_secs(10);

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
    /// `didOpen` is dispatched to the real tsgo (BEFORE the sync barrier) and removed only
    /// after a confirmed/successful `didClose` ack (a close that does not confirm returns an
    /// error frame and leaves the URI tracked for the drain), so a sent-but-unsynced open
    /// (barrier timeout / abnormal session end mid barrier) is INCLUDED in the best-effort
    /// session-end drain ATTEMPT. That drain is BOUNDED (see [`Self::retract_open_carriers`]),
    /// so such an overlay MAY LINGER in the editor's Program if the overall drain budget
    /// expires before its `didClose` is delivered — individual `didClose` delivery is not
    /// guaranteed.
    opened_carriers: HashSet<String>,
    /// Whether the unified session-end drain ATTEMPTS to retract this session's still-open
    /// carrier overlays. Defaults to `true`, so EVERY termination mode — a clean
    /// `verter/detach` AND every abnormal path (EOF / malformed frame / outbound failure /
    /// control-pipe drop without detach) — ATTEMPTS best-effort retraction (the drain is
    /// BOUNDED, so overlays MAY linger). Only an explicit `verter/detach` with
    /// `closeCarriers: false` opts OUT (the client deliberately leaves its overlays).
    retract_carriers_on_end: bool,
    /// The per-op bound each relay-round-trip handler ([`Self::handle_carrier_did_open_synced`],
    /// [`Self::handle_carrier_did_change_synced`], [`Self::handle_carrier_did_close`],
    /// [`Self::handle_initialize_api_session`]) wraps its relay send in. Those sends — the
    /// `didOpen`/`didChange`/`didClose` NOTIFICATIONS and the `custom/initializeAPISession`
    /// REQUEST — all ride the SAME BOUNDED outbound mpsc to the relay's server writer; a WEDGED
    /// writer (the mpsc full, the writer parked on `write_all`) makes an UNBOUNDED send/round-trip
    /// block the SERIAL read→dispatch serve loop forever — so the loop never observes EOF/detach
    /// and never reaches the session-end drain, defeating the drain's own bound. Wrapping each in
    /// this bound turns a wedged send into a fail-closed error frame instead of a block, keeping
    /// the serve loop live so it can terminate and drain. Defaults to [`CARRIER_SYNC_BARRIER_TIMEOUT`]
    /// (the same bound the carrier-sync barrier uses); a test sets it to a SHORT value to observe
    /// the internal timeout fire against a wedged writer.
    carrier_op_bound: Duration,
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
            carrier_op_bound: CARRIER_SYNC_BARRIER_TIMEOUT,
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
        // failure / control-pipe drop WITHOUT detach) — BEST-EFFORT retract this session's
        // still-open carrier overlays so stale Verter overlays are dropped from the editor's
        // own tsgo Program wherever the bounded drain budget allows. The drain is BOUNDED (see
        // `retract_open_carriers`), so an overlay whose `didClose` the budget cannot deliver
        // MAY LINGER — individual `didClose` delivery is not guaranteed.
        // `retract_carriers_on_end` honors an explicit `detach(closeCarriers: false)` opt-out;
        // it defaults to `true`, so an abnormal termination (which carries no detach signal)
        // always ATTEMPTS the best-effort retract. NON-DESTRUCTIVE: overlays only — never a
        // shim/child teardown (the shim owns its OWNED tsgo child's lifecycle and tears it down
        // only on editor disconnect / real-tsgo exit).
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
    /// path every session-end mode shares, BOUNDED by the overall
    /// [`CARRIER_SYNC_BARRIER_TIMEOUT`] drain budget so a wedged writer (a full injection
    /// channel to a peer that never accepts a `didClose`) cannot block teardown indefinitely.
    ///
    /// This is the transport-close removal for EVERY carrier still tracked in
    /// `opened_carriers` — INCLUDING a carrier whose per-carrier close did NOT confirm. A
    /// bounded or failed `didClose` leaves the URI tracked, because
    /// [`Self::handle_carrier_did_close`] removes it ONLY on a successful ack; so a
    /// best-effort overlay retract that did not physically leave the editor's shared tsgo
    /// Program is re-attempted here — a bounded best-effort `didClose` — on session/transport
    /// end (delivery is NOT guaranteed past the budget or on a wedged writer).
    ///
    /// The ACHIEVABLE guarantee, stated precisely — NOT a claim of guaranteed physical
    /// removal under all conditions:
    ///   * On a LIVE control path a carrier's own per-carrier close
    ///     ([`Self::handle_carrier_did_close`]) is an ordered `didClose` that CONFIRMS (its
    ///     send succeeds) before the URI is removed from `opened_carriers`.
    ///   * This session-end drain is BOUNDED BEST-EFFORT: it ATTEMPTS an ordered `didClose` for
    ///     each still-tracked carrier WITHIN the overall `total_budget` and does not await
    ///     confirmation; closes not attempted before the budget expires are abandoned
    ///     (best-effort) — individual delivery is NOT guaranteed past the budget.
    ///   * A residual left by a WEDGED writer (a down / never-draining control path where the
    ///     drain's `didClose` cannot be delivered at all) is backstopped by owned-tsgo
    ///     PROCESS DEATH: the shim tears its OWNED tsgo child down on editor disconnect /
    ///     real-tsgo exit, which drops every lingering overlay along with the Program.
    async fn retract_open_carriers(&mut self) {
        self.retract_open_carriers_within(CARRIER_SYNC_BARRIER_TIMEOUT)
            .await;
    }

    /// [`Self::retract_open_carriers`] with an explicit overall `total_budget` (the
    /// production entry uses [`CARRIER_SYNC_BARRIER_TIMEOUT`]; tests drive a small bound
    /// against a wedged writer to prove the fail-closed drain returns bounded, mirroring
    /// [`crate::relay::CarrierInjectionChannel::sync_overlay_with_timeout`]).
    ///
    /// The open set is DRAINED into an owned `Vec` BEFORE any await, so on ANY outcome —
    /// including budget exhaustion — `opened_carriers` is empty and a repeat drain is a
    /// no-op. The whole best-effort loop runs under ONE `total_budget` timeout; when that
    /// budget expires mid-drain the loop is cancelled and any undelivered `didClose`s are
    /// abandoned (best-effort, non-destructive). Individual carrier retraction is NOT
    /// guaranteed — only overall boundedness is.
    async fn retract_open_carriers_within(&mut self, total_budget: Duration) {
        let uris: Vec<String> = self.opened_carriers.drain().collect();
        let channel = self.relay.injection_channel();
        let _ = tokio::time::timeout(total_budget, async {
            for uri in uris {
                let _ = channel.did_close(&uri).await;
            }
        })
        .await;
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
            messages::METHOD_FEATURE_REQUEST => {
                (Some(self.handle_feature_request(&id, params).await), false)
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
                        feature_requests: true,
                    },
                };
                ok_frame(id, &result)
            }
            Err(rejection) => err_frame(id, rejection.error_code(), &rejection.message()),
        }
    }

    async fn handle_wait_initialized(&self, id: &serde_json::Value) -> Vec<u8> {
        // Bounded AND cancellable: the editor may never complete the LSP handshake (a
        // broken / detached engine), so this races the in-band witness against BOTH a
        // relay-stop signal and a hard timeout — never an unbounded block on the control
        // dispatch. `biased` prefers a real captured witness first, then relay-stop, then
        // the timeout, so a witness that lands as the relay is stopping still wins. The
        // relay-stop and timeout arms return `ERROR_CONTROL_OP_FAILED` with DISTINCT
        // messages so the caller can tell a stopped engine from a hung one.
        tokio::select! {
            biased;
            witness = self.relay.wait_initialized() => match witness {
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
            },
            () = self.relay.wait_stopped() => err_frame(
                id,
                ERROR_CONTROL_OP_FAILED,
                "the relay stopped before the editor initialize was observed",
            ),
            () = tokio::time::sleep(WAIT_INITIALIZED_TIMEOUT) => err_frame(
                id,
                ERROR_CONTROL_OP_FAILED,
                "timed out awaiting the editor initialize witness",
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
        // Send the `didOpen` FIRST (BOUNDED), then track the carrier as retract-eligible the
        // moment it MAY have reached the real tsgo — BEFORE the sync barrier. The overlay is
        // LIVE in the editor's own tsgo Program the instant the `didOpen` is sent, so a barrier
        // timeout/failure (or an abnormal session end while the barrier is in flight) must NOT
        // strand it: the session-end drain must retract EVERY carrier a `didOpen` was (or MAY
        // have been) sent for. The send is BOUNDED so a wedged writer cannot pin the serial
        // serve loop before the drain. Tracking on the full synced-open success alone was the
        // leak — a sent-but-unsynced open never entered the drain set.
        match tokio::time::timeout(
            self.carrier_op_bound,
            self.relay.injection_channel().did_open(
                &params.uri,
                &params.language_id,
                params.version,
                &params.text,
            ),
        )
        .await
        {
            // The `didOpen` reached tsgo — record the carrier for the session-end drain BEFORE
            // awaiting the barrier.
            Ok(Ok(())) => {
                self.opened_carriers.insert(params.uri.clone());
            }
            // A definite send failure (a dead channel): the `didOpen` never reached tsgo, so
            // nothing was injected and nothing can leak — do NOT track.
            Ok(Err(e)) => return op_error_frame(id, "carrier didOpen", &e),
            // The send did not complete within the bound (a wedged writer): the `didOpen` MAY
            // have reached tsgo, so the overlay is POSSIBLY-LIVE — track it as retract-eligible
            // so the session-end drain retracts it, then report the error without pinning the
            // serve loop.
            Err(_elapsed) => {
                self.opened_carriers.insert(params.uri.clone());
                return op_error_frame(
                    id,
                    "carrier didOpen",
                    &carrier_send_timeout_error(self.carrier_op_bound),
                );
            }
        }
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
        // BOUND the `didChange` send so a wedged writer cannot pin the serial serve loop, THEN
        // the ordered sync barrier — mirrors did_open_synced so a subsequent updateSnapshot sees
        // the updated overlay. A `didChange` failure/timeout keeps the prior state on the client
        // side; the shim just reports the error and makes no tracking change either way.
        match tokio::time::timeout(
            self.carrier_op_bound,
            channel.did_change(&params.uri, params.version, &params.text),
        )
        .await
        {
            Ok(Ok(())) => {}
            Ok(Err(e)) => return op_error_frame(id, "carrier didChangeSynced", &e),
            Err(_elapsed) => {
                return op_error_frame(
                    id,
                    "carrier didChangeSynced",
                    &carrier_send_timeout_error(self.carrier_op_bound),
                )
            }
        }
        match channel.sync_overlay(&params.uri).await {
            Ok(()) => ok_frame(id, &ControlAck { ok: true }),
            Err(e) => op_error_frame(id, "carrier didChangeSynced", &e),
        }
    }

    /// Retract ONE carrier overlay: `didClose` through the gated injection channel, then
    /// remove the URI from `opened_carriers` — but ONLY on a successful ack. A failed
    /// `relay.did_close` returns an error frame and leaves the URI TRACKED (removal is gated
    /// on the ack precisely so an unconfirmed close is not silently dropped); the still-open
    /// overlay is retracted by the session-end drain ([`Self::retract_open_carriers`]) on
    /// transport close.
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
        // BOUND the relay `didClose` send. On a WEDGED writer (the outbound mpsc full, the
        // writer parked on `write_all`) an unbounded send blocks the SERIAL serve loop forever,
        // so the loop never reaches the session-end drain. Only a CONFIRMED close removes the
        // URI; on a bounded-out (`Elapsed`) or failed (`Err`) send the URI stays TRACKED so the
        // session-end drain still retracts it (the residual-stays-tracked invariant).
        match tokio::time::timeout(
            self.carrier_op_bound,
            self.relay.injection_channel().did_close(&params.uri),
        )
        .await
        {
            Ok(Ok(())) => {
                self.opened_carriers.remove(&params.uri);
                ok_frame(id, &ControlAck { ok: true })
            }
            Ok(Err(e)) => op_error_frame(id, "carrier didClose", &e),
            Err(_elapsed) => op_error_frame(
                id,
                "carrier didClose",
                &carrier_send_timeout_error(self.carrier_op_bound),
            ),
        }
    }

    async fn handle_initialize_api_session(&mut self, id: &serde_json::Value) -> Vec<u8> {
        // BOUND the `reinitialize_api_session` round-trip. It rides the SAME gated server-writer
        // path as the carrier sends — a `custom/initializeAPISession` REQUEST — so on a WEDGED
        // writer (the outbound mpsc full, the writer parked on `write_all`) an unbounded await
        // would pin the SERIAL read→dispatch serve loop forever: the loop would never observe
        // EOF/detach and never reach the session-end drain. Mirrors the carrier-op handlers'
        // three-arm shape; on a bounded-out (`Elapsed`) or failed (`Err`) round-trip
        // `api_session_active` STAYS false (no `--api` session was minted).
        match tokio::time::timeout(
            self.carrier_op_bound,
            self.relay.injection_channel().reinitialize_api_session(),
        )
        .await
        {
            Ok(Ok(handle)) => {
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
            Ok(Err(e)) => op_error_frame(id, "initializeApiSession", &e),
            Err(_elapsed) => op_error_frame(
                id,
                "initializeApiSession",
                &carrier_send_timeout_error(self.carrier_op_bound),
            ),
        }
    }

    async fn handle_feature_request(
        &self,
        id: &serde_json::Value,
        params: serde_json::Value,
    ) -> Vec<u8> {
        let params: messages::FeatureRequestParams = match serde_json::from_value(params) {
            Ok(params) => params,
            Err(error) => {
                return err_frame(
                    id,
                    ERROR_MALFORMED_PAYLOAD,
                    &format!("feature request params: {error}"),
                )
            }
        };
        match tokio::time::timeout(
            self.carrier_op_bound,
            self.relay.feature_request(params.method, params.params),
        )
        .await
        {
            Ok(Ok(result)) => ok_frame(id, &messages::FeatureRequestResult { result }),
            Ok(Err(error)) => op_error_frame(id, "feature request", &error),
            Err(_) => op_error_frame(
                id,
                "feature request",
                &carrier_send_timeout_error(self.carrier_op_bound),
            ),
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

/// The typed error a relay-op handler returns when its relay op send/request does not
/// complete within [`ControlServer::carrier_op_bound`] — a WEDGED writer (the outbound mpsc full,
/// the writer parked on `write_all`). It bounds BOTH the `didOpen`/`didChange`/`didClose`
/// NOTIFICATIONS and the `custom/initializeAPISession` REQUEST. Returning it instead of blocking
/// keeps the SERIAL serve loop from being pinned, so the loop can terminate and reach the
/// session-end drain. Mirrors
/// [`crate::relay::CarrierInjectionChannel::sync_overlay_with_timeout`]'s fail-closed timeout.
fn carrier_send_timeout_error(bound: Duration) -> TsgoApiError {
    TsgoApiError::Timeout(format!(
        "relay op send/request exceeded {}ms (wedged writer; the serve loop is not pinned)",
        bound.as_millis()
    ))
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
