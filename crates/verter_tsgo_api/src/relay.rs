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
//!   editor and a `tsgo --lsp` server. Editor traffic passes through
//!   untouched in both directions — forwarded frames are the RAW original
//!   bytes, byte-identical (original object key order + whitespace, never
//!   re-encoded); Verter injects its own frames onto the server stream under
//!   the reserved `verter:*` request-id namespace, and responses to those
//!   injected requests demux back to Verter (never to the editor).
//!
//! This layer does NOT claim read-side leak suppression, feature-read
//! routing, mode selection, live editor attachment, or proof that injected
//! carriers appear in the editor Program. Those concerns are OUT of this
//! layer's scope.

use std::collections::{HashMap, HashSet};
use std::future::Future;
use std::pin::Pin;
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::sync::Mutex as StdMutex;

use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

use crate::attach::{parse_api_session_handle, ApiSessionHandle, INITIALIZE_API_SESSION_METHOD};
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
    /// The owner's overlay tracker: URIs [`Self::did_open`] successfully
    /// opened (retracted again by a successful [`Self::did_close`]). A std
    /// Mutex: lock, mutate, drop the guard — NEVER held across an `.await`.
    open_overlays: &'a StdMutex<HashSet<String>>,
}

impl<'a> CarrierInjectionChannel<'a> {
    /// Assemble the gate over a private sink + its owner's overlay tracker.
    pub(crate) fn new(
        sink: &'a dyn GatedWireSink,
        open_overlays: &'a StdMutex<HashSet<String>>,
    ) -> Self {
        Self {
            sink,
            open_overlays,
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
    /// The URI is tracked in the owner's overlay set so a non-owning teardown
    /// can retract exactly the overlays Verter opened.
    pub async fn did_open(
        &self,
        uri: &str,
        language_id: &str,
        version: i64,
        text: &str,
    ) -> TsgoApiResult<()> {
        // The overlay open is the ONLY path that sends `didOpen`: gate, send,
        // then track — bookkeeping is inseparable from the wire write. The
        // inline gate keeps deny-by-default UNIFORM (every wire write is gated,
        // even this fixed, always-allowlisted method).
        if !carrier_write_allowed("textDocument/didOpen", CarrierWriteKind::Notification) {
            return Err(TsgoApiError::WriteGateDenied {
                method: "textDocument/didOpen".to_string(),
            });
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
            // Track only AFTER the notify succeeded — a failed open must not
            // leave a phantom overlay for retraction. Lock, insert, drop the
            // guard — never held across the await above.
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
    /// the owner's overlay set only AFTER the notify succeeded.
    pub async fn did_close(&self, uri: &str) -> TsgoApiResult<()> {
        // The overlay close is the ONLY path that sends `didClose`: gate (the
        // same uniform deny-by-default guard), send, then untrack.
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
    pub async fn sync_overlay(&self, uri: &str) -> TsgoApiResult<()> {
        let params = serde_json::json!({ "textDocument": { "uri": uri } });
        match self.gated_request("textDocument/diagnostic", params).await {
            // The round-trip completed (the diagnostic RESULT is discarded;
            // a JSON-RPC error response still proves in-order consumption of
            // the queued didOpen/didChange): the barrier held.
            Ok(_) | Err(TsgoApiError::Transport(_)) => Ok(()),
            // No round-trip: the ordering guarantee did NOT hold — propagate.
            Err(e) => Err(e),
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
/// (dropped + recorded, never forwarded), and only responses carrying a
/// `verter:*` id demux to Verter's pending table (never to the editor).
pub(crate) const VERTER_ID_NAMESPACE: &str = "verter:";

/// The pending table for Verter-injected requests, keyed by their reserved
/// `verter:*` string id.
type VerterPending = Arc<Mutex<HashMap<String, oneshot::Sender<serde_json::Value>>>>;

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
/// the SAME serialized server-writer channel as forwarded editor frames (so
/// injection and pass-through never interleave mid-frame). Reached ONLY
/// through [`LspRelay::injection_channel`] — the deny-by-default gate.
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

/// A transport-agnostic bidirectional `--lsp` FRAME relay between an editor
/// and a `tsgo --lsp` server, with a gated Verter injection port.
///
/// Editor→server frames are forwarded unless they violate the reserved
/// `verter:*` id namespace (dropped + recorded, never misrouted).
/// Server→client traffic passes through UNTOUCHED — transparency; forwarded
/// frames are the RAW original bytes, byte-identical (parsed only for id
/// inspection, never re-encoded, so object key order and whitespace are
/// preserved); this layer performs NO read-side suppression — except that
/// responses to Verter-injected `verter:*` requests demux to Verter's
/// pending table and never reach the editor. Verter-injected frames and
/// forwarded editor frames serialize through ONE server-writer channel, so
/// injection and pass-through never interleave mid-frame.
///
/// The relay does not own the server engine: stopping it never sends `exit`
/// (or any other lifecycle write). All Verter writes enter exclusively
/// through [`LspRelay::injection_channel`] — the deny-by-default
/// [`CarrierInjectionChannel`] gate.
pub struct LspRelay {
    port: RelayInjectPort,
    /// The overlay tracker for [`LspRelay::injection_channel`]: URIs a
    /// successful injected `did_open` recorded (see
    /// [`CarrierInjectionChannel::did_open`]).
    open_overlays: StdMutex<HashSet<String>>,
    /// Count of dropped editor frames that carried a reserved `verter:*` id.
    reservation_violations: Arc<AtomicU64>,
    /// The three pump/writer tasks; aborted on shutdown/drop.
    tasks: Vec<tokio::task::JoinHandle<()>>,
}

impl LspRelay {
    /// Start the relay over split editor and server transports: three tasks —
    /// the editor→server pump, the server→editor pump, and the serialized
    /// server-writer — plus the injection port riding the same writer.
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

        let tasks = vec![
            tokio::spawn(server_writer_task(server_write, server_rx)),
            tokio::spawn(editor_to_server_pump(
                editor_read,
                server_tx.clone(),
                Arc::clone(&reservation_violations),
            )),
            tokio::spawn(server_to_editor_pump(
                server_read,
                editor_write,
                Arc::clone(&verter_pending),
            )),
        ];

        LspRelay {
            port: RelayInjectPort {
                server_tx,
                verter_pending,
                next_inject_id: AtomicU64::new(0),
            },
            open_overlays: StdMutex::new(HashSet::new()),
            reservation_violations,
            tasks,
        }
    }

    /// The gated write surface over the relay's injection port. The
    /// deny-by-default allowlist applies — see [`CarrierInjectionChannel`].
    #[must_use]
    pub fn injection_channel(&self) -> CarrierInjectionChannel<'_> {
        CarrierInjectionChannel::new(&self.port, &self.open_overlays)
    }

    /// How many editor frames were dropped for carrying a reserved
    /// `verter:*` id (the namespace-reservation violations).
    #[must_use]
    pub fn reservation_violations(&self) -> u64 {
        self.reservation_violations.load(Ordering::Relaxed)
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
            .finish_non_exhaustive()
    }
}

/// The serialized server-writer task: drains BOTH forwarded editor frames and
/// Verter-injected frames onto the server transport in channel order, so the
/// two never interleave mid-frame.
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
/// check, never re-encoded) — UNLESS the frame carries a reserved `verter:*`
/// id, which is a namespace-reservation violation: the frame is dropped and
/// recorded, never forwarded, never misrouted into Verter's pending table.
async fn editor_to_server_pump<R>(
    mut editor_read: R,
    server_tx: mpsc::Sender<Vec<u8>>,
    reservation_violations: Arc<AtomicU64>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut framer = MessageFramer::new();
    let mut chunk = [0u8; 8192];
    loop {
        match editor_read.read(&mut chunk).await {
            Ok(0) | Err(_) => break, // EOF
            Ok(n) => framer.push(&chunk[..n]),
        }
        loop {
            match framer.next_frame() {
                Ok(Some((msg, raw))) => {
                    if frame_carries_verter_id(&msg) {
                        reservation_violations.fetch_add(1, Ordering::Relaxed);
                        continue;
                    }
                    // Forward the RAW original bytes — never a re-encode of
                    // the parsed value (which would reorder object keys and
                    // recompact whitespace).
                    if server_tx.send(raw).await.is_err() {
                        return; // writer gone
                    }
                }
                Ok(None) => break,
                // A malformed editor frame is unrecoverable on a framed
                // stream: fail closed, stop pumping.
                Err(_) => return,
            }
        }
    }
}

/// The server→editor pump: frame server bytes; a RESPONSE (`id` present, no
/// `method`) carrying a reserved `verter:*` id demuxes to Verter's pending
/// table (never forwarded — with no registered waiter it is discarded, never
/// leaked to the editor); EVERY other frame passes through to the editor
/// UNTOUCHED — its RAW original bytes, byte-identical (the parsed value is
/// inspected ONLY for the demux check, never re-encoded); transparency — no
/// read-side suppression at this layer.
async fn server_to_editor_pump<R, W>(
    mut server_read: R,
    mut editor_write: W,
    verter_pending: VerterPending,
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
                    // Forward the RAW original bytes — never a re-encode of
                    // the parsed value (which would reorder object keys and
                    // recompact whitespace).
                    if editor_write.write_all(&raw).await.is_err() {
                        break 'outer;
                    }
                    if editor_write.flush().await.is_err() {
                        break 'outer;
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
}

#[cfg(test)]
#[path = "relay_tests.rs"]
mod tests;
