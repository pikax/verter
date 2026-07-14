//! The gated carrier-injection write surface + the bidirectional `--lsp`
//! frame relay.
//!
//! Two cooperating pieces live here:
//!
//! - [`CarrierInjectionChannel`] — the SINGLE deny-by-default write gate for
//!   every non-owning carrier write. For non-owning/editor-owned attach
//!   handles, public carrier writes are available ONLY through
//!   `CarrierInjectionChannel`. The channel deny-by-default allowlist permits
//!   Verter overlay lifecycle notifications, the ordered sync-barrier
//!   request, and `custom/initializeAPISession` re-emission; ALL other
//!   methods are rejected before reaching the wire. Owned attach handles may
//!   expose the raw `JsonRpcConnection`; non-owning attach handles must not
//!   expose or clone it through public API.
//! - [`LspRelay`] — a transport-agnostic bidirectional frame relay between an
//!   editor and a `tsgo --lsp` server. Editor→server traffic passes through
//!   untouched (except reserved `verter:*`-id frames, which are dropped +
//!   recorded); server→editor frames run the deny-by-default carrier egress
//!   policy (`classify_egress`, see the `egress` module) after the Verter
//!   response demux and the reserved-id request-anomaly answer (a
//!   server→client request carrying a `verter:*` id is answered back to the
//!   server with a synthesized negative, never editor-routed) and before
//!   the raw forward: carrier-FREE frames are
//!   forwarded as the RAW original bytes, byte-identical (original object
//!   key order + whitespace, never re-encoded), while carrier-contaminated
//!   frames are suppressed or JSON re-encoded after carrier entries are
//!   removed; EVERY carrier-referencing server→client request (a mixed or
//!   all-carrier `workspace/applyEdit` included) is answered on the
//!   server's behalf with a synthesized negative response (protocol
//!   liveness) — never routed to the editor, raw or filtered, and never
//!   through the Verter write-gate; a carrier-referencing unfilterable response
//!   to a TRACKED editor request is completed with a method-valid
//!   carrier-free neutral to the editor (original id — no leak, no
//!   strand), while an untracked one still drops whole, fail-closed.
//!   Verter injects its own frames onto the server
//!   stream under the reserved `verter:*` request-id namespace, and
//!   responses to those injected requests demux back to Verter (never to
//!   the editor).
//!
//! This layer does NOT claim feature-read routing, mode selection, live
//! editor attachment, source-position presentation, or proof that injected
//! carriers appear in the editor Program. Those concerns are OUT of this
//! layer's scope.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;
use std::time::Duration;

use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot, watch};

use crate::attach::{parse_api_session_handle, ApiSessionHandle, INITIALIZE_API_SESSION_METHOD};
use crate::egress::{classify_egress, synthesize_server_response, EgressDecision};
use crate::error::{TsgoApiError, TsgoApiResult};
use crate::jsonrpc::framing::{encode_message, MessageFramer};
use crate::jsonrpc::JsonRpcConnection;

/// The JSON-RPC message kind a carrier write rides. The deny-by-default gate
/// admits a method only for its correct kind — an allowlisted method sent as
/// the wrong kind (e.g. the `didOpen` notification sent as a request, or the
/// `diagnostic` request sent as a notification) is refused before the wire.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum CarrierWriteKind {
    /// A JSON-RPC notification (no response).
    Notification,
    /// A JSON-RPC request (awaits a response).
    Request,
}

/// The single deny-by-default allowlist TRUTH: the ONLY `(method, kind)` pairs
/// a [`CarrierInjectionChannel`] lets onto the wire.
///
/// - `textDocument/didOpen` / `didChange` / `didClose` — Verter's carrier
///   overlay lifecycle NOTIFICATIONS.
/// - `textDocument/diagnostic` — the ordered sync-barrier REQUEST (see
///   [`CarrierInjectionChannel::sync_overlay`]).
/// - `custom/initializeAPISession` — the `--api` session (re-)emission REQUEST.
///
/// Both axes matter: a method admitted for one kind is refused for the other.
/// Feature reads (hover/definition/references/…) are NOT admitted here;
/// feature-read routing is out of this layer's scope.
const CARRIER_INJECTION_ALLOWLIST: &[(&str, CarrierWriteKind)] = &[
    ("textDocument/didOpen", CarrierWriteKind::Notification),
    ("textDocument/didChange", CarrierWriteKind::Notification),
    ("textDocument/didClose", CarrierWriteKind::Notification),
    ("textDocument/diagnostic", CarrierWriteKind::Request),
    ("custom/initializeAPISession", CarrierWriteKind::Request),
];

/// Whether `(method, kind)` is admitted by the deny-by-default carrier-write
/// gate — BOTH the method AND its JSON-RPC kind must match an allowlist entry.
pub(crate) fn carrier_write_allowed(method: &str, kind: CarrierWriteKind) -> bool {
    CARRIER_INJECTION_ALLOWLIST
        .iter()
        .any(|&(m, k)| m == method && k == kind)
}

/// The stateful overlay open/close lifecycle notifications. Their wire write is
/// bound to the `open_overlays` bookkeeping, so they ride ONLY the
/// tracking-aware [`CarrierInjectionChannel::did_open`] /
/// [`CarrierInjectionChannel::did_close`], never the raw notification sender
/// ([`CarrierInjectionChannel::gated_notify`]), which refuses them — so no
/// overlay is ever opened without being tracked for retraction.
const OVERLAY_LIFECYCLE_METHODS: &[&str] = &["textDocument/didOpen", "textDocument/didClose"];

/// Whether `method` is a stateful overlay open/close lifecycle notification
/// (see [`OVERLAY_LIFECYCLE_METHODS`]).
fn is_overlay_lifecycle_method(method: &str) -> bool {
    OVERLAY_LIFECYCLE_METHODS.contains(&method)
}

/// A boxed sink future (mirrors the crate's existing boxed-future seam style).
type SinkFuture<'a, T> = Pin<Box<dyn Future<Output = TsgoApiResult<T>> + Send + 'a>>;

/// The write surface a [`CarrierInjectionChannel`] forwards ADMITTED writes
/// to: either a [`JsonRpcConnection`] directly or an [`LspRelay`]'s inject
/// port. Crate-private BY DESIGN — the channel is the single gate over both
/// sinks, and no public API hands a sink out.
pub(crate) trait GatedWireSink: Send + Sync {
    /// Send a notification (no response expected) onto the sink.
    fn send_notify<'a>(&'a self, method: &'a str, params: serde_json::Value) -> SinkFuture<'a, ()>;
    /// Send a request onto the sink and await its result.
    ///
    /// Error contract (both sinks — [`JsonRpcConnection::request`] and the
    /// relay inject port — uphold it, and
    /// [`CarrierInjectionChannel::sync_overlay`]'s barrier semantics depend
    /// on it): a COMPLETED round-trip whose response is a JSON-RPC error
    /// surfaces as [`TsgoApiError::Transport`]; a request that never
    /// round-trips (send failure / closed connection / relay stopped)
    /// surfaces as [`TsgoApiError::Closed`].
    fn send_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
    ) -> SinkFuture<'a, serde_json::Value>;
}

impl GatedWireSink for JsonRpcConnection {
    fn send_notify<'a>(&'a self, method: &'a str, params: serde_json::Value) -> SinkFuture<'a, ()> {
        Box::pin(self.notify(method, params))
    }

    fn send_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
    ) -> SinkFuture<'a, serde_json::Value> {
        Box::pin(self.request(method, params))
    }
}

/// The bound on the carrier-sync barrier ([`CarrierInjectionChannel::sync_overlay`]):
/// a slow or broken editor tsgo that never answers the injected pull-diagnostic
/// request cannot stall `carrierDidOpenSynced` / `carrierDidChangeSynced` — and thus
/// the LSP file lifecycle — beyond this. On elapse the barrier fails CLOSED
/// ([`TsgoApiError::Timeout`]) so the caller degrades to the OWNED baseline.
pub const CARRIER_SYNC_BARRIER_TIMEOUT: Duration = Duration::from_secs(10);

/// The gated carrier-injection write facade: the SINGLE deny-by-default
/// allowlist gate in front of a private wire sink.
///
/// The PUBLIC surface is the closed set of typed carrier ops
/// ([`Self::did_open`] / [`Self::did_change`] / [`Self::did_close`] /
/// [`Self::sync_overlay`] / [`Self::did_open_synced`] /
/// [`Self::reinitialize_api_session`]); there is NO public raw method-string
/// write. Every op runs the [`carrier_write_allowed`] `(method, kind)` gate
/// BEFORE the wire, refusing a non-admitted or kind-mismatched method with the
/// typed [`TsgoApiError::WriteGateDenied`]: the non-lifecycle ops go through the
/// private senders [`Self::gated_notify`] / [`Self::gated_request`], while the
/// stateful overlay open/close ([`Self::did_open`] / [`Self::did_close`]) apply
/// the same gate inline and send their fixed method directly, threading the
/// overlay tracker inseparably from the wire write. The channel never exposes
/// (or clones out) its underlying sink.
pub struct CarrierInjectionChannel<'a> {
    /// The private write sink. Never handed out.
    sink: &'a dyn GatedWireSink,
    /// The owner's ACTIVE-lifecycle overlay tracker (retraction state): URIs
    /// [`Self::did_open`] successfully opened, retracted again by a
    /// successful [`Self::did_close`]. A std Mutex: lock, mutate, drop the
    /// guard — NEVER held across an `.await`.
    open_overlays: &'a StdMutex<HashSet<String>>,
    /// The owner's MONOTONIC egress-taint record: every URI a carrier
    /// [`Self::did_open`] ever attempted, inserted BEFORE the `didOpen`
    /// reaches the wire and NEVER removed ([`Self::did_close`] does not
    /// touch it). Derived from the same injection lifecycle as
    /// `open_overlays` — no token/path heuristic — but on a separate
    /// lifetime axis: retraction state and egress-taint lifetime are
    /// distinct, so an in-flight server frame about a just-closed (or
    /// just-opening) carrier still classifies as carrier-attributed. Same
    /// locking discipline: never held across an `.await`.
    carrier_egress_taint: &'a StdMutex<HashSet<String>>,
}

impl<'a> CarrierInjectionChannel<'a> {
    /// Assemble the gate over a private sink + its owner's overlay tracker
    /// and monotonic egress-taint record.
    pub(crate) fn new(
        sink: &'a dyn GatedWireSink,
        open_overlays: &'a StdMutex<HashSet<String>>,
        carrier_egress_taint: &'a StdMutex<HashSet<String>>,
    ) -> Self {
        Self {
            sink,
            open_overlays,
            carrier_egress_taint,
        }
    }

    /// Send a NON-lifecycle carrier notification through the gate. Refuses the
    /// stateful overlay open/close lifecycle (reachable only through
    /// [`Self::did_open`] / [`Self::did_close`]) and any method not admitted as
    /// a notification, with [`TsgoApiError::WriteGateDenied`] before the wire.
    async fn gated_notify(&self, method: &str, params: serde_json::Value) -> TsgoApiResult<()> {
        if is_overlay_lifecycle_method(method) {
            return Err(TsgoApiError::WriteGateDenied {
                method: method.to_string(),
            });
        }
        if !carrier_write_allowed(method, CarrierWriteKind::Notification) {
            return Err(TsgoApiError::WriteGateDenied {
                method: method.to_string(),
            });
        }
        self.sink.send_notify(method, params).await
    }

    /// Send a carrier request through the gate: a method not admitted as a
    /// request (not allowlisted, or allowlisted only as a notification) is
    /// refused with [`TsgoApiError::WriteGateDenied`] before the wire.
    async fn gated_request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> TsgoApiResult<serde_json::Value> {
        if !carrier_write_allowed(method, CarrierWriteKind::Request) {
            return Err(TsgoApiError::WriteGateDenied {
                method: method.to_string(),
            });
        }
        self.sink.send_request(method, params).await
    }

    /// Inject an off-disk carrier as an LSP `textDocument/didOpen` overlay.
    /// The URI is tainted for egress BEFORE the wire send and tracked in the
    /// owner's overlay set (after a successful notify) so a non-owning
    /// teardown can retract exactly the overlays Verter opened.
    pub async fn did_open(
        &self,
        uri: &str,
        language_id: &str,
        version: i64,
        text: &str,
    ) -> TsgoApiResult<()> {
        // The overlay open is the ONLY path that sends `didOpen`: gate, taint,
        // send, then track — bookkeeping is inseparable from the wire write.
        // The inline gate keeps deny-by-default UNIFORM (every wire write is
        // gated, even this fixed, always-allowlisted method).
        if !carrier_write_allowed("textDocument/didOpen", CarrierWriteKind::Notification) {
            return Err(TsgoApiError::WriteGateDenied {
                method: "textDocument/didOpen".to_string(),
            });
        }
        {
            // Taint BEFORE the wire send (fail-closed at open): the very
            // first server frame emitted in response to this didOpen must
            // already classify as carrier-attributed when the egress pump
            // reads it. The taint is monotonic — a failed notify leaves it
            // in place (never fail open). Lock, insert, drop the guard —
            // never held across the await below.
            let mut taint = self.carrier_egress_taint.lock().unwrap();
            taint.insert(uri.to_string());
        }
        let params = serde_json::json!({
            "textDocument": {
                "uri": uri,
                "languageId": language_id,
                "version": version,
                "text": text,
            }
        });
        self.sink
            .send_notify("textDocument/didOpen", params)
            .await?;
        {
            // Track for RETRACTION only AFTER the notify succeeded — a failed
            // open must not leave a phantom overlay for retraction. Lock,
            // insert, drop the guard — never held across the await above.
            let mut overlays = self.open_overlays.lock().unwrap();
            overlays.insert(uri.to_string());
        }
        Ok(())
    }

    /// Update an open carrier overlay via `textDocument/didChange` (full content).
    pub async fn did_change(&self, uri: &str, version: i64, text: &str) -> TsgoApiResult<()> {
        let params = serde_json::json!({
            "textDocument": { "uri": uri, "version": version },
            "contentChanges": [{ "text": text }],
        });
        self.gated_notify("textDocument/didChange", params).await
    }

    /// Retract a carrier overlay via `textDocument/didClose`. The URI leaves
    /// the owner's overlay set only AFTER the notify succeeded. The egress
    /// taint is NOT touched — it is monotonic, so an in-flight server frame
    /// about the just-closed carrier still classifies as carrier-attributed.
    pub async fn did_close(&self, uri: &str) -> TsgoApiResult<()> {
        // The overlay close is the ONLY path that sends `didClose`: gate (the
        // same uniform deny-by-default guard), send, then untrack. Retraction
        // removes ONLY the active-lifecycle entry (`open_overlays`), never
        // the `carrier_egress_taint` entry.
        if !carrier_write_allowed("textDocument/didClose", CarrierWriteKind::Notification) {
            return Err(TsgoApiError::WriteGateDenied {
                method: "textDocument/didClose".to_string(),
            });
        }
        let params = serde_json::json!({ "textDocument": { "uri": uri } });
        self.sink
            .send_notify("textDocument/didClose", params)
            .await?;
        {
            // Lock, remove, drop the guard — never held across the await above.
            let mut overlays = self.open_overlays.lock().unwrap();
            overlays.remove(uri);
        }
        Ok(())
    }

    /// Barrier: force the `--lsp` server to drain pending document
    /// notifications (`didOpen` / `didChange`) BEFORE an `--api`
    /// `updateSnapshot` enumerates roots on the shared `project.Session`.
    ///
    /// The two surfaces ride DIFFERENT transports (the `--lsp` wire and the
    /// `--api` pipe), so a fire-and-forget `didOpen` notification can
    /// otherwise race behind an `updateSnapshot` on the pipe and the
    /// just-opened overlay would not yet be a Program member. LSP processes
    /// messages in order ON ONE connection, so awaiting a `--lsp` REQUEST for
    /// `uri` after the `didOpen` guarantees the overlay is registered by the
    /// time it returns. The pull `textDocument/diagnostic` request serves as
    /// that barrier (its result is discarded — the OWNED diagnostics
    /// authority is the `--api` checker; a server that does not implement
    /// pull diagnostics still processes the queued didOpen before answering
    /// or erroring here).
    ///
    /// Returns `Ok` exactly when the barrier round-trip COMPLETED: a success
    /// result, or a completed JSON-RPC error response (e.g. "method not
    /// found" from a server without pull diagnostics) — the server processed
    /// the queued notifications in order either way. A request that never
    /// round-trips (a send failure / closed connection) means the ordering
    /// guarantee did NOT hold, and the failure propagates. This depends on
    /// the [`GatedWireSink::send_request`] contract (mirroring
    /// [`JsonRpcConnection::request`]): a completed JSON-RPC error response
    /// surfaces as [`TsgoApiError::Transport`]; a request with no round-trip
    /// surfaces as [`TsgoApiError::Closed`].
    ///
    /// The barrier is BOUNDED by [`CARRIER_SYNC_BARRIER_TIMEOUT`]: a slow or broken
    /// editor tsgo that never answers the pull-diagnostic request cannot stall the
    /// carrier lifecycle indefinitely — on timeout the barrier returns
    /// [`TsgoApiError::Timeout`] (fail-closed; the caller degrades to the OWNED baseline)
    /// rather than blocking forever.
    pub async fn sync_overlay(&self, uri: &str) -> TsgoApiResult<()> {
        self.sync_overlay_with_timeout(uri, CARRIER_SYNC_BARRIER_TIMEOUT)
            .await
    }

    /// [`Self::sync_overlay`] with an explicit `timeout` bound (the production entry
    /// uses [`CARRIER_SYNC_BARRIER_TIMEOUT`]; tests drive a small bound against a
    /// never-answering sink to prove the fail-closed timeout).
    pub async fn sync_overlay_with_timeout(
        &self,
        uri: &str,
        timeout: Duration,
    ) -> TsgoApiResult<()> {
        let params = serde_json::json!({ "textDocument": { "uri": uri } });
        let barrier = self.gated_request("textDocument/diagnostic", params);
        match tokio::time::timeout(timeout, barrier).await {
            // The round-trip completed (the diagnostic RESULT is discarded;
            // a JSON-RPC error response still proves in-order consumption of
            // the queued didOpen/didChange): the barrier held.
            Ok(Ok(_)) | Ok(Err(TsgoApiError::Transport(_))) => Ok(()),
            // No round-trip: the ordering guarantee did NOT hold — propagate.
            Ok(Err(e)) => Err(e),
            // The barrier exceeded its bound (a slow/broken editor tsgo never
            // answered): the ordering guarantee did NOT hold — fail CLOSED with a
            // Timeout rather than block the carrier lifecycle indefinitely.
            Err(_elapsed) => Err(TsgoApiError::Timeout(format!(
                "carrier-sync barrier for {uri} exceeded {}ms",
                timeout.as_millis()
            ))),
        }
    }

    /// Open an off-disk carrier overlay and synchronize it (the common path):
    /// [`Self::did_open`] followed by [`Self::sync_overlay`], so a subsequent
    /// `--api` `updateSnapshot` sees the carrier as a Program member.
    pub async fn did_open_synced(
        &self,
        uri: &str,
        language_id: &str,
        version: i64,
        text: &str,
    ) -> TsgoApiResult<()> {
        self.did_open(uri, language_id, version, text).await?;
        self.sync_overlay(uri).await
    }

    /// Re-emit `custom/initializeAPISession` through the gate and parse the
    /// `{ sessionId, pipe }` result into an [`ApiSessionHandle`].
    pub async fn reinitialize_api_session(&self) -> TsgoApiResult<ApiSessionHandle> {
        let value = self
            .gated_request(INITIALIZE_API_SESSION_METHOD, serde_json::json!({}))
            .await?;
        parse_api_session_handle(&value)
    }
}

impl std::fmt::Debug for CarrierInjectionChannel<'_> {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("CarrierInjectionChannel")
            .field(
                "open_overlays",
                &self.open_overlays.lock().map(|o| o.len()).unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

/// The RESERVED injected-request-id namespace. Every Verter-injected request
/// id is minted as `verter:<n>`; the namespace is validated on BOTH pumps —
/// an editor frame carrying a `verter:*` id is a reservation violation
/// (dropped + recorded, never forwarded), only responses carrying a
/// `verter:*` id demux to Verter's pending table (never to the editor), and
/// a server→client REQUEST carrying a `verter:*` id is an anomaly answered
/// back to the server with a synthesized negative (never forwarded — the
/// editor could only answer it under a reserved id this relay drops).
pub(crate) const VERTER_ID_NAMESPACE: &str = "verter:";

/// The pending table for Verter-injected requests, keyed by their reserved
/// `verter:*` string id.
type VerterPending = Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>>;

/// The ingress record of in-flight EDITOR-originated requests: canonical id
/// key ([`canonical_id_key`]) → JSON-RPC method. Written by the
/// editor→server pump when it forwards an editor request; read AND removed
/// by the server→editor pump when the matching response arrives, so the
/// egress classifier knows which method a suppressed carrier-referencing response
/// would strand ([`crate::egress`]'s `AnswerEditor` liveness completion).
/// Removing on every response bounds the table on the common path; a
/// `$/cancelRequest` notification additionally prunes the cancelled id's
/// pending record (the editor→server pump), so a cancelled-but-never-responded
/// request does not hold an entry.
type EditorPendingMethods = Arc<StdMutex<HashMap<String, String>>>;

/// The canonical map key for a JSON-RPC request id: the id's JSON
/// serialization for a string or number id (`"5"` and `"\"5\""` stay
/// distinct, exactly as JSON-RPC distinguishes them); `None` for a null id
/// (a null-id frame is not a correlatable request/response). Both pumps key
/// through this ONE helper so the ingress record and the egress lookup
/// always agree on the key form.
fn canonical_id_key(id: &serde_json::Value) -> Option<String> {
    if id.is_null() {
        return None;
    }
    serde_json::to_string(id).ok()
}

/// Whether a frame's `id` sits in the reserved `verter:*` namespace.
fn frame_carries_verter_id(msg: &serde_json::Value) -> bool {
    msg.get("id")
        .and_then(|v| v.as_str())
        .is_some_and(|s| s.starts_with(VERTER_ID_NAMESPACE))
}

/// A waiter guard that removes its pending entry on drop (abandon-only
/// cancel), mirroring the connection layer: if the injected-request future is
/// dropped before the response demuxes, the entry is pruned so a late
/// response is discarded instead of leaking.
struct VerterPendingGuard {
    id: String,
    pending: VerterPending,
    armed: bool,
}

impl Drop for VerterPendingGuard {
    fn drop(&mut self) {
        if self.armed {
            self.pending.lock().remove(&self.id);
        }
    }
}

/// The relay's private injection port: a [`GatedWireSink`] whose writes ride
/// the SAME serialized server-writer channel as forwarded editor frames and
/// the server-bound synthesized egress responses (the `AnswerServer`
/// negatives and the reserved-id request-anomaly answers), so no two
/// server-bound writes interleave mid-frame. Reached ONLY through
/// [`LspRelay::injection_channel`] — the deny-by-default gate.
struct RelayInjectPort {
    server_tx: mpsc::Sender<Vec<u8>>,
    verter_pending: VerterPending,
    next_inject_id: AtomicU64,
}

impl RelayInjectPort {
    /// Mint the next reserved injected-request id (`verter:<n>`).
    fn mint_injected_id(&self) -> String {
        let n = self.next_inject_id.fetch_add(1, Ordering::Relaxed);
        format!("{VERTER_ID_NAMESPACE}{n}")
    }
}

impl GatedWireSink for RelayInjectPort {
    fn send_notify<'a>(&'a self, method: &'a str, params: serde_json::Value) -> SinkFuture<'a, ()> {
        Box::pin(async move {
            let msg = serde_json::json!({
                "jsonrpc": "2.0",
                "method": method,
                "params": params,
            });
            self.server_tx
                .send(encode_message(&msg))
                .await
                .map_err(|_| TsgoApiError::Closed)
        })
    }

    fn send_request<'a>(
        &'a self,
        method: &'a str,
        params: serde_json::Value,
    ) -> SinkFuture<'a, serde_json::Value> {
        Box::pin(async move {
            let id = self.mint_injected_id();
            let (tx, rx) = oneshot::channel();
            self.verter_pending.lock().insert(id.clone(), tx);
            let mut guard = VerterPendingGuard {
                id: id.clone(),
                pending: Arc::clone(&self.verter_pending),
                armed: true,
            };

            let msg = serde_json::json!({
                "jsonrpc": "2.0",
                "id": id,
                "method": method,
                "params": params,
            });
            self.server_tx
                .send(encode_message(&msg))
                .await
                .map_err(|_| TsgoApiError::Closed)?;

            match rx.await {
                Ok(value) => {
                    guard.armed = false; // response demuxed; nothing to prune
                    if let Some(err) = value.get("error") {
                        let message = err
                            .get("message")
                            .and_then(|m| m.as_str())
                            .unwrap_or("unknown error");
                        return Err(TsgoApiError::Transport(format!(
                            "jsonrpc error response: {message}"
                        )));
                    }
                    Ok(value
                        .get("result")
                        .cloned()
                        .unwrap_or(serde_json::Value::Null))
                }
                // The pump dropped the sender (relay stopped) — the guard
                // prunes on drop.
                Err(_) => Err(TsgoApiError::Closed),
            }
        })
    }
}

/// The in-band `initialize` witness the relay observed as the editor↔server
/// handshake passed through it: the engine's self-reported `serverInfo.version`
/// plus the editor's `initialize` request id and workspace params.
///
/// A non-owning attach reuses the editor-originated `initialize` (Verter never
/// re-`initialize`s an editor-owned engine), so the accepted engine version can
/// only be observed IN-BAND — exactly this witness. A [`LspRelay`] captures it
/// once, when the editor→tsgo `initialize` response passes the server→editor
/// pump; [`LspRelay::wait_initialized`] blocks until then.
#[derive(Debug, Clone)]
pub struct InitializedWitness {
    /// The engine `serverInfo.version` read from the `initialize` response
    /// (`None` if the server reported none — the caller's gate decides).
    pub server_info_version: Option<String>,
    /// The JSON-RPC id of the editor's `initialize` request the relay observed.
    pub observed_initialize_id: serde_json::Value,
    /// The `rootUri` the editor sent in `initialize`.
    pub root_uri: Option<String>,
    /// The `workspaceFolders` the editor sent in `initialize`, if any.
    pub workspace_folders: Option<serde_json::Value>,
}

/// The editor→server pump's capture of the `initialize` REQUEST — the half of
/// [`InitializedWitness`] that only appears on the request. The server→editor
/// pump joins it with the response's `serverInfo.version` when the correlated
/// `initialize` response passes.
#[derive(Debug, Clone)]
struct InitializeRequestCapture {
    id: serde_json::Value,
    root_uri: Option<String>,
    workspace_folders: Option<serde_json::Value>,
}

/// The single-slot capture the two pumps share to assemble the
/// [`InitializedWitness`]. The editor→server pump writes the request half; the
/// server→editor pump reads it and publishes the joined witness on `tx`.
type InitializeCaptureSlot = Arc<StdMutex<Option<InitializeRequestCapture>>>;

/// Extract `rootUri` (a string) from an `initialize` request's params.
fn extract_root_uri(params: Option<&serde_json::Value>) -> Option<String> {
    params?
        .get("rootUri")
        .and_then(|v| v.as_str())
        .map(str::to_string)
}

/// Extract `workspaceFolders` from an `initialize` request's params.
fn extract_workspace_folders(params: Option<&serde_json::Value>) -> Option<serde_json::Value> {
    params?.get("workspaceFolders").cloned()
}

/// A transport-agnostic bidirectional `--lsp` FRAME relay between an editor
/// and a `tsgo --lsp` server, with a gated Verter injection port.
///
/// Editor→server frames are forwarded unless they violate the reserved
/// `verter:*` id namespace (dropped + recorded, never misrouted).
/// Server→editor frames first demux responses to Verter-injected `verter:*`
/// requests back to Verter's pending table (never to the editor), and a
/// server→client request carrying a reserved `verter:*` id — an anomaly no
/// editor answer could ever resolve — is answered back to the server with a
/// synthesized negative, never editor-routed; every
/// other frame then runs the deny-by-default carrier egress policy
/// (`classify_egress`, whose carrier authority is exactly the relay's
/// monotonic egress-taint set — every URI Verter itself ever opened via an
/// injected `didOpen`, tainted before the wire send and never removed on
/// `didClose`): carrier-FREE frames pass byte-identical — the RAW
/// original bytes (parsed only for inspection, never re-encoded, so object
/// key order and whitespace are preserved) — while carrier-contaminated
/// frames are suppressed (counted by [`LspRelay::suppressed_egress`]) or
/// JSON re-encoded after carrier entries are removed; EVERY
/// carrier-referencing server→client request (a mixed or all-carrier
/// `workspace/applyEdit` included) is answered on the server's behalf with
/// a synthesized negative response (protocol liveness) — never routed to
/// the editor, raw or filtered, and never through the Verter write-gate;
/// and a carrier-referencing unfilterable response to a TRACKED editor request
/// (the editor→server pump records every forwarded editor request's
/// `id → method`) is completed with a method-valid carrier-free neutral to
/// the editor under the original id, so the editor's pending request
/// resolves — an untracked one still drops whole, fail-closed.
/// Verter-injected frames, forwarded editor frames, and SERVER-BOUND
/// synthesized responses (the `AnswerServer` negatives and the reserved-id
/// request-anomaly answers) serialize through ONE server-writer channel, so
/// they never interleave mid-frame; the `AnswerEditor` synthesized neutrals
/// are written to the editor transport, not the server-writer channel.
///
/// The relay does not own the server engine: stopping it never sends `exit`
/// (or any other lifecycle write). Every Verter-authored carrier-injection
/// / carrier-overlay write enters this relay exclusively through
/// [`LspRelay::injection_channel`] — the deny-by-default
/// [`CarrierInjectionChannel`] gate. The relay's direct bypass writes are
/// not carrier injection: editor-originated frames are forwarded as raw,
/// editor-owned bytes; server→editor `Forward` writes are carrier-free, and
/// `FilterCarrierEntries` writes are re-encoded only after carrier entries
/// are removed. The synthesized protocol-liveness responses — server-bound
/// `AnswerServer` negatives and reserved-id request-anomaly answers, plus
/// editor-bound `AnswerEditor` neutrals — use fixed sanitized bodies and
/// only echo the original JSON-RPC `id`; they bypass the carrier gate
/// because they do not author carrier overlays.
pub struct LspRelay {
    port: RelayInjectPort,
    /// The ACTIVE-lifecycle overlay tracker (retraction state) for
    /// [`LspRelay::injection_channel`]: URIs a successful injected
    /// `did_open` recorded, removed again by a successful `did_close` (see
    /// [`CarrierInjectionChannel::did_open`] /
    /// [`CarrierInjectionChannel::did_close`]).
    open_overlays: Arc<StdMutex<HashSet<String>>>,
    /// The MONOTONIC egress-taint set: every URI a carrier `did_open` ever
    /// attempted, inserted BEFORE the `didOpen` reaches the wire and never
    /// removed (`did_close` retracts only `open_overlays`). Shared with the
    /// server→editor pump, whose egress policy consults it as the SOLE
    /// carrier authority (`classify_egress`) — so an in-flight server frame
    /// about a just-closed (or just-opening) carrier still suppresses,
    /// fail-closed on both races.
    carrier_egress_taint: Arc<StdMutex<HashSet<String>>>,
    /// The in-flight editor request record (`id → method`), written by the
    /// editor→server pump and consumed by the server→editor pump so a
    /// suppressed carrier-referencing response to a tracked editor request can be
    /// completed with a method-valid neutral instead of stranding the
    /// request (see [`EditorPendingMethods`]).
    pending_editor_requests: EditorPendingMethods,
    /// Count of dropped editor frames that carried a reserved `verter:*` id.
    reservation_violations: Arc<AtomicU64>,
    /// Count of server→editor frames the deny-by-default egress policy kept
    /// from the editor whole (a demuxed `verter:*` response is also not
    /// forwarded, but is a separate mechanism and is not counted here): frames
    /// dropped outright by the egress policy, server requests answered on
    /// the server's behalf (carrier-referencing and reserved-id anomalies
    /// alike), and carrier-referencing responses to tracked editor requests
    /// replaced by the synthesized neutral.
    suppressed_egress: Arc<AtomicU64>,
    /// The watch receiver for the in-band `initialize` witness: `None` until
    /// the editor→tsgo `initialize` response passes the server→editor pump,
    /// then `Some(witness)`. Level-triggered, so a waiter that arrives after
    /// the handshake still observes it (see [`LspRelay::wait_initialized`]).
    witness_rx: watch::Receiver<Option<InitializedWitness>>,
    /// The watch receiver for the relay-stopped signal: flips to `true` when
    /// either pump ends (editor / server stream EOF or error), so a shim can
    /// tear down on editor disconnect or engine exit (see
    /// [`LspRelay::wait_stopped`]).
    stopped_rx: watch::Receiver<bool>,
    /// The three pump/writer tasks; aborted on shutdown/drop.
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl LspRelay {
    /// Start the relay over split editor and server transports: three tasks —
    /// the editor→server pump, the server→editor pump, and the serialized
    /// server-writer — plus the injection port riding the same writer (the
    /// one channel that also carries the server→editor pump's server-bound
    /// synthesized answers).
    pub fn start<ER, EW, SR, SW>(
        editor_read: ER,
        editor_write: EW,
        server_read: SR,
        server_write: SW,
    ) -> LspRelay
    where
        ER: AsyncRead + Unpin + Send + 'static,
        EW: AsyncWrite + Unpin + Send + 'static,
        SR: AsyncRead + Unpin + Send + 'static,
        SW: AsyncWrite + Unpin + Send + 'static,
    {
        let (server_tx, server_rx) = mpsc::channel::<Vec<u8>>(256);
        let verter_pending: VerterPending = Arc::new(Mutex::new(HashMap::new()));
        let reservation_violations = Arc::new(AtomicU64::new(0));
        let suppressed_egress = Arc::new(AtomicU64::new(0));
        let open_overlays = Arc::new(StdMutex::new(HashSet::new()));
        let carrier_egress_taint = Arc::new(StdMutex::new(HashSet::new()));
        let pending_editor_requests: EditorPendingMethods = Arc::new(StdMutex::new(HashMap::new()));
        let initialize_capture: InitializeCaptureSlot = Arc::new(StdMutex::new(None));
        let (witness_tx, witness_rx) = watch::channel::<Option<InitializedWitness>>(None);
        // A single `stopped` signal shared by both pumps: either direction
        // ending (editor / server EOF or stream error) flips it, so the shim
        // tears down on an editor disconnect as well as an engine exit.
        let (stopped_tx, stopped_rx) = watch::channel::<bool>(false);

        let tasks = vec![
            tokio::spawn(server_writer_task(server_write, server_rx)),
            tokio::spawn(editor_to_server_pump(
                editor_read,
                server_tx.clone(),
                Arc::clone(&reservation_violations),
                Arc::clone(&pending_editor_requests),
                Arc::clone(&initialize_capture),
                stopped_tx.clone(),
            )),
            tokio::spawn(server_to_editor_pump(
                server_read,
                editor_write,
                server_tx.clone(),
                Arc::clone(&verter_pending),
                Arc::clone(&carrier_egress_taint),
                Arc::clone(&pending_editor_requests),
                Arc::clone(&suppressed_egress),
                Arc::clone(&initialize_capture),
                witness_tx,
                stopped_tx,
            )),
        ];

        LspRelay {
            port: RelayInjectPort {
                server_tx,
                verter_pending,
                next_inject_id: AtomicU64::new(0),
            },
            open_overlays,
            carrier_egress_taint,
            pending_editor_requests,
            reservation_violations,
            suppressed_egress,
            witness_rx,
            stopped_rx,
            tasks,
        }
    }

    /// The gated write surface over the relay's injection port. The
    /// deny-by-default allowlist applies — see [`CarrierInjectionChannel`].
    #[must_use]
    pub fn injection_channel(&self) -> CarrierInjectionChannel<'_> {
        CarrierInjectionChannel::new(
            &self.port,
            self.open_overlays.as_ref(),
            self.carrier_egress_taint.as_ref(),
        )
    }

    /// Block until the editor→tsgo `initialize` response has passed the relay,
    /// returning the captured in-band [`InitializedWitness`] (the engine's
    /// `serverInfo.version` + the editor's `initialize` id and workspace
    /// params). Level-triggered: if the handshake already completed, this
    /// returns immediately. Returns `None` only if the relay stops before any
    /// `initialize` response is seen (the witness sender dropped).
    ///
    /// This is the SOLE in-band version witness on the SHARED path — a
    /// non-owning attach never re-`initialize`s the editor-owned engine, so the
    /// accepted version must be read from the pass-through handshake here.
    pub async fn wait_initialized(&self) -> Option<InitializedWitness> {
        let mut rx = self.witness_rx.clone();
        if let Some(witness) = rx.borrow().clone() {
            return Some(witness);
        }
        loop {
            // `changed()` errors only when every sender dropped (relay gone).
            if rx.changed().await.is_err() {
                return None;
            }
            if let Some(witness) = rx.borrow().clone() {
                return Some(witness);
            }
        }
    }

    /// The in-band `initialize` witness if it has already been observed, without
    /// blocking (`None` while the handshake is still in flight).
    #[must_use]
    pub fn initialized_witness(&self) -> Option<InitializedWitness> {
        self.witness_rx.borrow().clone()
    }

    /// Block until the relay stops pumping — either stream direction ending
    /// (editor stdin EOF / editor disconnect, or the `tsgo` server stream
    /// closing). A shim awaits this to tear down on an editor disconnect that
    /// did not route through the engine's own exit. Level-triggered: if the
    /// relay has already stopped, this returns immediately.
    pub async fn wait_stopped(&self) {
        let mut rx = self.stopped_rx.clone();
        if *rx.borrow() {
            return;
        }
        while rx.changed().await.is_ok() {
            if *rx.borrow() {
                return;
            }
        }
    }

    /// How many editor frames were dropped for carrying a reserved
    /// `verter:*` id (the namespace-reservation violations).
    #[must_use]
    pub fn reservation_violations(&self) -> u64 {
        self.reservation_violations.load(Ordering::Relaxed)
    }

    /// How many server→editor frames the deny-by-default egress policy kept
    /// from the editor (a demuxed `verter:*` response is also not forwarded,
    /// but is a separate mechanism and is not counted here):
    /// whole-frame drops (carrier-attributed frames with no filterable
    /// editor-correlated remainder), plus server→client requests answered
    /// on the server's behalf (carrier-referencing ones and reserved-id
    /// anomalies alike), plus carrier-referencing responses to tracked editor
    /// requests replaced by the synthesized neutral. Per-entry filtering
    /// re-encodes and forwards instead, and is not counted here.
    #[must_use]
    pub fn suppressed_egress(&self) -> u64 {
        self.suppressed_egress.load(Ordering::Relaxed)
    }

    /// Stop the relay tasks and unblock every injected-request waiter. The
    /// relay does not own the server engine, so this never sends `exit` (or
    /// any other write) — it only stops pumping.
    pub async fn shutdown(&self) {
        for task in &self.tasks {
            task.abort();
        }
        self.port.verter_pending.lock().clear();
    }
}

impl Drop for LspRelay {
    fn drop(&mut self) {
        for task in &self.tasks {
            task.abort();
        }
    }
}

impl std::fmt::Debug for LspRelay {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspRelay")
            .field(
                "reservation_violations",
                &self.reservation_violations.load(Ordering::Relaxed),
            )
            .field(
                "suppressed_egress",
                &self.suppressed_egress.load(Ordering::Relaxed),
            )
            .field(
                "pending_editor_requests",
                &self
                    .pending_editor_requests
                    .lock()
                    .map(|p| p.len())
                    .unwrap_or(0),
            )
            .finish_non_exhaustive()
    }
}

/// The serialized server-writer task: drains EVERY server-bound write onto
/// the server transport in channel order. Three source classes ride the one
/// channel — forwarded editor frames (the editor→server pump's raw
/// pass-through), Verter-injected carrier frames (the injection port), and
/// server-bound synthesized egress responses (the `AnswerServer` negatives —
/// `workspace/applyEdit` → `{"applied": false}`, every other method → the
/// sanitized `-32803` "request failed" error — and the reserved-id
/// request-anomaly answers) — so no two writes ever interleave mid-frame.
async fn server_writer_task<W>(mut server_write: W, mut server_rx: mpsc::Receiver<Vec<u8>>)
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    while let Some(bytes) = server_rx.recv().await {
        if server_write.write_all(&bytes).await.is_err() {
            break;
        }
        if server_write.flush().await.is_err() {
            break;
        }
    }
    let _ = server_write.shutdown().await;
}

/// The editor→server pump: frame editor bytes and forward each frame's RAW
/// original bytes to the serialized server writer (byte-faithful
/// pass-through — the parsed value is inspected ONLY for the reserved-id
/// check and the pending-request record, never re-encoded) — UNLESS the
/// frame carries a reserved `verter:*` id, which is a namespace-reservation
/// violation: the frame is dropped and recorded, never forwarded, never
/// misrouted into Verter's pending table. A forwarded editor REQUEST
/// (non-null `id` + `method`) records `id → method` into
/// `pending_editor_requests` BEFORE the forward, so the server→editor pump
/// can complete a suppressed carrier-referencing response to it (the record always
/// exists by the time the server can have seen the request). A
/// `$/cancelRequest` notification prunes the cancelled id's pending record
/// (bounding the table even when a server never answers) and still forwards
/// raw.
async fn editor_to_server_pump<R>(
    mut editor_read: R,
    server_tx: mpsc::Sender<Vec<u8>>,
    reservation_violations: Arc<AtomicU64>,
    pending_editor_requests: EditorPendingMethods,
    initialize_capture: InitializeCaptureSlot,
    stopped_tx: watch::Sender<bool>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut framer = MessageFramer::new();
    let mut chunk = [0u8; 8192];
    'outer: loop {
        match editor_read.read(&mut chunk).await {
            Ok(0) | Err(_) => break, // EOF (editor stdin closed / disconnect)
            Ok(n) => framer.push(&chunk[..n]),
        }
        loop {
            match framer.next_frame() {
                Ok(Some((msg, raw))) => {
                    if frame_carries_verter_id(&msg) {
                        reservation_violations.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    // Capture the editor's `initialize` REQUEST half of the
                    // in-band witness (its id + workspace params). The
                    // server→editor pump joins it with the response's
                    // `serverInfo.version` when the correlated response passes.
                    // The relay forwards this frame raw below — the capture is
                    // observation-only, never a mutation.
                    if msg.get("method").and_then(|m| m.as_str()) == Some("initialize") {
                        if let Some(id) = msg.get("id").filter(|v| !v.is_null()) {
                            let params = msg.get("params");
                            let capture = InitializeRequestCapture {
                                id: id.clone(),
                                root_uri: extract_root_uri(params),
                                workspace_folders: extract_workspace_folders(params),
                            };
                            let mut slot = match initialize_capture.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            *slot = Some(capture);
                        }
                    }
                    // A `$/cancelRequest` prunes the cancelled request from the
                    // pending table. (A server usually answers a cancelled
                    // request with a RequestCancelled error, which removes the
                    // entry via the response path; this bounds the table even
                    // when a server never responds.) The notification still
                    // forwards raw below.
                    if msg.get("method").and_then(|m| m.as_str()) == Some("$/cancelRequest") {
                        if let Some(key) = msg
                            .get("params")
                            .and_then(|p| p.get("id"))
                            .and_then(canonical_id_key)
                        {
                            let mut pending = match pending_editor_requests.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            pending.remove(&key);
                        }
                    }
                    // Record an editor REQUEST's id → method BEFORE the
                    // forward (lock, insert, drop the guard — never held
                    // across the await below; a poisoned lock recovers the
                    // tracked data). Bounded by remove-on-response in the
                    // server→editor pump and by the `$/cancelRequest` prune
                    // above.
                    if let (Some(key), Some(method)) = (
                        msg.get("id").and_then(canonical_id_key),
                        msg.get("method").and_then(|m| m.as_str()),
                    ) {
                        let mut pending = match pending_editor_requests.lock() {
                            Ok(guard) => guard,
                            Err(poisoned) => poisoned.into_inner(),
                        };
                        pending.insert(key, method.to_string());
                    }
                    // Forward the RAW original bytes — never a re-encode of
                    // the parsed value (which would reorder object keys and
                    // recompact whitespace).
                    if server_tx.send(raw).await.is_err() {
                        break 'outer; // writer gone
                    }
                }
                Ok(None) => break,
                // A malformed editor frame is unrecoverable on a framed
                // stream: fail closed, stop pumping.
                Err(_) => break 'outer,
            }
        }
    }
    // Signal that the relay's editor→server direction has stopped (editor
    // disconnect / stream error), so a shim can tear down.
    let _ = stopped_tx.send(true);
}

/// The server→editor pump: frame server bytes; a RESPONSE (`id` present, no
/// `method`) carrying a reserved `verter:*` id demuxes to Verter's pending
/// table (never forwarded — with no registered waiter it is discarded, never
/// leaked to the editor); a server→client REQUEST (`id` + `method`)
/// carrying a reserved `verter:*` id is a protocol anomaly answered
/// straight back to the SERVER with the synthesized negative
/// (`synthesize_server_response`, original id, sent on the serialized
/// server-writer channel, counted on `suppressed_egress`) — never
/// classified, never forwarded (a forwarded reserved-id request could only
/// be answered under a reserved id the editor→server pump drops, hanging
/// the server); EVERY other frame then runs the deny-by-default
/// carrier egress policy (`classify_egress`) against a snapshot of the
/// MONOTONIC carrier egress-taint set (every URI a carrier `did_open` ever
/// attempted — never removed on `didClose`, so an in-flight frame about a
/// just-closed carrier still suppresses): the `Forward` arm writes the RAW
/// original bytes,
/// byte-identical (the parsed value is inspected, never re-encoded), the
/// `Suppress` arm drops the frame and bumps the suppressed-egress counter,
/// a filtered frame is JSON re-encoded with its carrier entries removed,
/// the `AnswerServer` arm sends the synthesized response for a suppressed
/// server→client request back to the SERVER on the serialized
/// server-writer channel (never to the editor, never through the Verter
/// write-gate) and bumps the same counter, and the `AnswerEditor` arm
/// writes the synthesized carrier-free method-valid neutral (original id;
/// fail-closed — the sanitized `-32803` error by default, `result: null`
/// ONLY for a method on the explicit null-valid allowlist; see
/// `synthesize_editor_response`) to the
/// EDITOR so a tracked editor request whose carrier-referencing response was kept
/// back still resolves, bumping the same counter. Before classification, a
/// non-demuxed RESPONSE looks up and removes its `pending_editor_requests`
/// record in one lock —
/// the correlated method the classifier's response branch consults.
/// Carrier-FREE frames stay byte-identical; positions are NOT mapped —
/// carrier-referencing entries drop fail-closed until a live
/// `ProviderPositionMapper` can present source locations.
#[allow(clippy::too_many_arguments)]
async fn server_to_editor_pump<R, W>(
    mut server_read: R,
    mut editor_write: W,
    server_tx: mpsc::Sender<Vec<u8>>,
    verter_pending: VerterPending,
    carrier_egress_taint: Arc<StdMutex<HashSet<String>>>,
    pending_editor_requests: EditorPendingMethods,
    suppressed_egress: Arc<AtomicU64>,
    initialize_capture: InitializeCaptureSlot,
    witness_tx: watch::Sender<Option<InitializedWitness>>,
    stopped_tx: watch::Sender<bool>,
) where
    R: AsyncRead + Unpin + Send + 'static,
    W: AsyncWrite + Unpin + Send + 'static,
{
    let mut framer = MessageFramer::new();
    let mut chunk = [0u8; 8192];
    'outer: loop {
        match server_read.read(&mut chunk).await {
            Ok(0) | Err(_) => break, // EOF
            Ok(n) => framer.push(&chunk[..n]),
        }
        loop {
            match framer.next_frame() {
                Ok(Some((msg, raw))) => {
                    let is_response = msg.get("id").map(|v| !v.is_null()).unwrap_or(false)
                        && msg.get("method").is_none();
                    if is_response && frame_carries_verter_id(&msg) {
                        // The namespace check above proved the id is a
                        // `verter:*` string; extract it without panicking (a
                        // long-running pump must never panic) and demux to
                        // the pending waiter — a response with no registered
                        // waiter is discarded, never forwarded.
                        let waiter = msg
                            .get("id")
                            .and_then(|v| v.as_str())
                            .and_then(|id| verter_pending.lock().remove(id));
                        if let Some(tx) = waiter {
                            let _ = tx.send(msg);
                        }
                        continue;
                    }
                    // A server→client REQUEST (`id` + `method`) whose id
                    // sits in the reserved `verter:*` namespace is a
                    // protocol anomaly: Verter mints reserved ids only for
                    // its own injected requests, and forwarding this frame
                    // would hang the server — the editor's answer would
                    // carry the same reserved id, which the editor→server
                    // pump drops as a reservation violation, so the
                    // server's request could never resolve. Answer the
                    // SERVER with the synthesized negative under the
                    // ORIGINAL id instead — the frame never reaches the
                    // egress classifier or the editor.
                    let is_request = msg.get("id").map(|v| !v.is_null()).unwrap_or(false)
                        && msg.get("method").is_some();
                    if is_request && frame_carries_verter_id(&msg) {
                        let resp = synthesize_server_response(&msg);
                        let _ = server_tx.send(encode_message(&resp)).await;
                        suppressed_egress.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    // A (non-demuxed) RESPONSE correlates with a tracked
                    // editor request: look up AND remove its recorded method
                    // in one lock (remove-on-response bounds the table). The
                    // method feeds the classifier's response branch so a
                    // suppressed carrier-referencing response can complete as a
                    // neutral instead of stranding the editor. A poisoned
                    // lock recovers the tracked data.
                    let editor_pending_method = if is_response {
                        msg.get("id").and_then(canonical_id_key).and_then(|key| {
                            let mut pending = match pending_editor_requests.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            pending.remove(&key)
                        })
                    } else {
                        None
                    };
                    // The correlated `initialize` RESPONSE: join its
                    // `serverInfo.version` with the editor→server pump's
                    // request-half capture and PUBLISH the in-band witness
                    // (once — the watch holds the first observed value). This
                    // rides the existing editor-request correlation; the frame
                    // still forwards to the editor untouched below.
                    if editor_pending_method.as_deref() == Some("initialize") {
                        let server_info_version = msg
                            .get("result")
                            .and_then(|r| r.get("serverInfo"))
                            .and_then(|s| s.get("version"))
                            .and_then(|v| v.as_str())
                            .map(str::to_string);
                        let request = {
                            let slot = match initialize_capture.lock() {
                                Ok(guard) => guard,
                                Err(poisoned) => poisoned.into_inner(),
                            };
                            slot.clone()
                        };
                        let observed_initialize_id = request
                            .as_ref()
                            .map(|r| r.id.clone())
                            .or_else(|| msg.get("id").cloned())
                            .unwrap_or(serde_json::Value::Null);
                        let witness = InitializedWitness {
                            server_info_version,
                            observed_initialize_id,
                            root_uri: request.as_ref().and_then(|r| r.root_uri.clone()),
                            workspace_folders: request
                                .as_ref()
                                .and_then(|r| r.workspace_folders.clone()),
                        };
                        // Publish only the FIRST observed handshake (a benign
                        // no-op if a later spurious `initialize` response
                        // arrives — the shared witness is already set).
                        if witness_tx.borrow().is_none() {
                            let _ = witness_tx.send(Some(witness));
                        }
                    }
                    // Snapshot the MONOTONIC egress-taint set: lock → clone →
                    // drop the guard BEFORE any await (the std Mutex is never
                    // held across an await). A poisoned lock recovers the
                    // tracked data — it never fails OPEN with an empty set.
                    let carriers = match carrier_egress_taint.lock() {
                        Ok(guard) => guard.clone(),
                        Err(poisoned) => poisoned.into_inner().clone(),
                    };
                    match classify_egress(&msg, &carriers, editor_pending_method.as_deref()) {
                        EgressDecision::Forward => {
                            // Forward the RAW original bytes — never a
                            // re-encode of the parsed value (which would
                            // reorder object keys and recompact whitespace).
                            if editor_write.write_all(&raw).await.is_err() {
                                break 'outer;
                            }
                            if editor_write.flush().await.is_err() {
                                break 'outer;
                            }
                        }
                        EgressDecision::Suppress => {
                            // Whole-frame drop: nothing is written; the drop
                            // is observable through the counter.
                            suppressed_egress.fetch_add(1, Ordering::Relaxed);
                        }
                        EgressDecision::FilterCarrierEntries(filtered) => {
                            // The carrier entries were removed; the frame is
                            // re-encoded (the one transparency exception —
                            // carrier-contaminated frames are never
                            // byte-identical).
                            let bytes = encode_message(&filtered);
                            if editor_write.write_all(&bytes).await.is_err() {
                                break 'outer;
                            }
                            if editor_write.flush().await.is_err() {
                                break 'outer;
                            }
                        }
                        EgressDecision::AnswerServer(resp) => {
                            // A suppressed server→client REQUEST is answered
                            // on the editor's behalf: the synthesized
                            // response rides the one serialized server-writer
                            // channel (in channel order, no mid-frame
                            // interleave with the port's other server-bound
                            // writes) — to the SERVER, never the editor,
                            // and never through the Verter write-gate. A
                            // failed send means the writer is gone; the pump
                            // then ends on EOF.
                            let _ = server_tx.send(encode_message(&resp)).await;
                            suppressed_egress.fetch_add(1, Ordering::Relaxed);
                        }
                        EgressDecision::AnswerEditor(resp) => {
                            // A carrier-referencing response to a TRACKED editor
                            // request: the real frame is kept from the
                            // editor, and the synthesized carrier-FREE
                            // neutral (original id; fail-closed — the
                            // sanitized `-32803` error by default,
                            // `result: null` ONLY for a method on the
                            // explicit null-valid allowlist) is written to
                            // the EDITOR so
                            // the pending request resolves — never `raw`
                            // carrier bytes, never routed to the server.
                            let bytes = encode_message(&resp);
                            if editor_write.write_all(&bytes).await.is_err() {
                                break 'outer;
                            }
                            if editor_write.flush().await.is_err() {
                                break 'outer;
                            }
                            suppressed_egress.fetch_add(1, Ordering::Relaxed);
                        }
                    }
                }
                Ok(None) => break,
                // A malformed server frame is unrecoverable: fail closed.
                Err(_) => break 'outer,
            }
        }
    }
    // EOF / error: unblock every injected-request waiter.
    verter_pending.lock().clear();
    // Signal that the relay's server→editor direction has stopped (engine
    // exit / stream error), so a shim can tear down.
    let _ = stopped_tx.send(true);
}

#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;
