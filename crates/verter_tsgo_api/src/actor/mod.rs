//! The single-writer process actor.
//!
//! # Concurrency model (matched to the wire)
//!
//! The tsgo `--api` wire is single-flight and correlated by method NAME, not by
//! a request id (syncChannel.js): the client writes one request, then reads
//! frames until the matching-name `RESPONSE`/`ERROR` arrives, servicing any
//! `CALL` (host FS callback) frames inline. There is NO request id and NO
//! multiplexing, so exactly one request is outstanding at a time.
//!
//! This module makes that wire usable from async Rust without holding any lock
//! across an `.await`:
//!
//! - A cloneable [`ClientHandle`] submits requests over two bounded
//!   `tokio::mpsc` lanes (interactive / batch) to a single **actor task**.
//! - The actor task owns the [`DuplexTransport`] (both directions). Its loop
//!   pops the next request (interactive lane drained first), writes the request
//!   frame, then reads frames: a `CALL` is serviced SYNCHRONOUSLY from the
//!   published [`OverlaySnapshot`] and answered with `CALL_RESPONSE`/`CALL_ERROR`
//!   (never awaiting the client — the deadlock guard); the matching
//!   `RESPONSE`/`ERROR` completes the request's oneshot.
//! - All per-request state (the in-flight method name, its oneshot) is LOCAL to
//!   the actor loop, so no mutex is ever held across `.await`
//!   (`clippy::await_holding_lock` passes).
//!
//! # Cancellation
//!
//! A request carries a cancellation flag ([`RequestOptions::cancel`]). A request
//! cancelled before it is written is skipped (never sent). The single-flight
//! wire cannot abort a request already being read; for that case the handle
//! future still resolves to [`TsgoApiError::Cancelled`] promptly and the actor
//! discards the eventual response. True preemption of a long in-flight request
//! is a process restart ([`ClientHandle::restart`]). This is the honest
//! capability of the protocol — see the crate-level docs.

mod transport;

#[cfg(test)]
mod tests;

pub use transport::{read_one_frame, DuplexTransport, FrameStream};

use std::sync::Arc;

use arc_swap::ArcSwap;
use tokio::sync::{mpsc, oneshot};

use crate::error::{TsgoApiError, TsgoApiResult};
use crate::lane::Lane;
use crate::proto::frame::{decode_frame, encode_frame, MessageType};
use crate::snapshot::{OverlaySnapshot, ReadFileResult};

/// Options controlling a single request.
#[derive(Debug, Default, Clone)]
pub struct RequestOptions {
    /// The scheduling lane.
    pub lane: Lane,
    /// An optional cancellation handle. When set and tripped before the request
    /// is written, the request is skipped; when tripped while in flight, the
    /// handle future resolves to [`TsgoApiError::Cancelled`] and the response is
    /// discarded.
    pub cancel: Option<CancelToken>,
}

/// A cheap, cloneable cancellation flag.
#[derive(Debug, Clone, Default)]
pub struct CancelToken {
    flag: Arc<std::sync::atomic::AtomicBool>,
}

impl CancelToken {
    /// Create a fresh, un-cancelled token.
    pub fn new() -> Self {
        Self::default()
    }

    /// Mark the token cancelled.
    pub fn cancel(&self) {
        self.flag.store(true, std::sync::atomic::Ordering::SeqCst);
    }

    /// Whether the token has been cancelled.
    pub fn is_cancelled(&self) -> bool {
        self.flag.load(std::sync::atomic::Ordering::SeqCst)
    }
}

/// An internal request envelope passed from a handle to the actor.
struct ActorRequest {
    method: String,
    payload: Vec<u8>,
    cancel: Option<CancelToken>,
    reply: oneshot::Sender<TsgoApiResult<Vec<u8>>>,
}

/// A control message to the actor (separate from data requests).
enum ActorControl {
    /// Shut the actor down: stop reading, drop the transport.
    Shutdown(oneshot::Sender<()>),
}

/// A cloneable handle to the single-writer actor.
///
/// Submitting a request returns a future that resolves when the engine replies
/// (or the request is cancelled / the actor is closed). The handle is cheap to
/// clone and share across tasks.
#[derive(Clone)]
pub struct ClientHandle {
    interactive_tx: mpsc::Sender<ActorRequest>,
    batch_tx: mpsc::Sender<ActorRequest>,
    control_tx: mpsc::Sender<ActorControl>,
    snapshot: Arc<ArcSwap<OverlaySnapshot>>,
}

impl ClientHandle {
    /// Publish a new overlay snapshot. The actor's callback servicing reads the
    /// latest published snapshot lock-free on the next callback.
    pub fn publish_snapshot(&self, snapshot: OverlaySnapshot) {
        self.snapshot.store(Arc::new(snapshot));
    }

    /// Submit a request with the given JSON payload bytes and options. Resolves
    /// to the raw response payload bytes (the caller decodes the typed result).
    pub async fn request(
        &self,
        method: &str,
        payload: Vec<u8>,
        opts: RequestOptions,
    ) -> TsgoApiResult<Vec<u8>> {
        // Fast cancellation: a request cancelled before submission never enters
        // the queue.
        if let Some(tok) = &opts.cancel {
            if tok.is_cancelled() {
                return Err(TsgoApiError::Cancelled);
            }
        }

        let (reply_tx, reply_rx) = oneshot::channel();
        let req = ActorRequest {
            method: method.to_string(),
            payload,
            cancel: opts.cancel.clone(),
            reply: reply_tx,
        };

        let tx = match opts.lane {
            Lane::Interactive => &self.interactive_tx,
            Lane::Batch => &self.batch_tx,
        };
        tx.send(req).await.map_err(|_| TsgoApiError::Closed)?;

        // Await either the actor's reply or, if a cancel token is provided, a
        // prompt cancellation. We do not hold any lock here.
        match &opts.cancel {
            Some(tok) => {
                tokio::select! {
                    biased;
                    res = reply_rx => res.map_err(|_| TsgoApiError::Closed)?,
                    () = wait_cancelled(tok.clone()) => Err(TsgoApiError::Cancelled),
                }
            }
            None => reply_rx.await.map_err(|_| TsgoApiError::Closed)?,
        }
    }

    /// Restart is the only way to preempt a request already in flight on the
    /// single-flight wire. It shuts the current actor down; the caller then
    /// spawns a fresh actor (a new [`ClientHandle`]). Returns once the old actor
    /// has stopped.
    pub async fn restart(&self) -> TsgoApiResult<()> {
        let (tx, rx) = oneshot::channel();
        self.control_tx
            .send(ActorControl::Shutdown(tx))
            .await
            .map_err(|_| TsgoApiError::Closed)?;
        rx.await.map_err(|_| TsgoApiError::Closed)
    }

    /// Shut the actor down and release the transport/child.
    pub async fn close(&self) -> TsgoApiResult<()> {
        self.restart().await
    }
}

/// Poll a cancellation token to completion (resolves when cancelled). Uses a
/// short yielding poll loop — cheap because cancellation is rare and the wire
/// response usually wins the `select!`.
async fn wait_cancelled(tok: CancelToken) {
    loop {
        if tok.is_cancelled() {
            return;
        }
        tokio::time::sleep(std::time::Duration::from_millis(2)).await;
    }
}

/// Spawn the actor over a given transport, returning a cloneable handle. The
/// `queue_depth` bounds each lane's in-flight request backlog (backpressure).
///
/// The returned handle drives the transport; dropping ALL handles closes the
/// lanes, which ends the actor loop.
pub fn spawn_actor<T>(transport: T, snapshot: OverlaySnapshot, queue_depth: usize) -> ClientHandle
where
    T: DuplexTransport + Send + 'static,
{
    let (interactive_tx, interactive_rx) = mpsc::channel(queue_depth.max(1));
    let (batch_tx, batch_rx) = mpsc::channel(queue_depth.max(1));
    let (control_tx, control_rx) = mpsc::channel(4);
    let snapshot = Arc::new(ArcSwap::from_pointee(snapshot));

    let actor = Actor {
        transport,
        interactive_rx,
        batch_rx,
        control_rx,
        snapshot: Arc::clone(&snapshot),
    };
    tokio::spawn(actor.run());

    ClientHandle {
        interactive_tx,
        batch_tx,
        control_tx,
        snapshot,
    }
}

/// The single actor task.
struct Actor<T: DuplexTransport> {
    transport: T,
    interactive_rx: mpsc::Receiver<ActorRequest>,
    batch_rx: mpsc::Receiver<ActorRequest>,
    control_rx: mpsc::Receiver<ActorControl>,
    snapshot: Arc<ArcSwap<OverlaySnapshot>>,
}

impl<T: DuplexTransport> Actor<T> {
    async fn run(mut self) {
        loop {
            // Drain a control message first (shutdown) without blocking.
            if let Ok(ctrl) = self.control_rx.try_recv() {
                match ctrl {
                    ActorControl::Shutdown(ack) => {
                        let _ = ack.send(());
                        return;
                    }
                }
            }

            // Pick the next request: interactive lane has priority. We bias the
            // select toward the interactive lane and toward control.
            let next = tokio::select! {
                biased;
                ctrl = self.control_rx.recv() => {
                    match ctrl {
                        Some(ActorControl::Shutdown(ack)) => { let _ = ack.send(()); return; }
                        None => None,
                    }
                }
                req = self.interactive_rx.recv() => req,
                req = self.batch_rx.recv() => req,
            };

            let Some(req) = next else {
                // An lane closed and yielded None. If BOTH data lanes are closed
                // (all handles dropped), stop. Otherwise keep serving.
                if self.interactive_rx.is_closed() && self.batch_rx.is_closed() {
                    return;
                }
                continue;
            };

            // Before sending: honour a pre-send cancellation.
            if let Some(tok) = &req.cancel {
                if tok.is_cancelled() {
                    let _ = req.reply.send(Err(TsgoApiError::Cancelled));
                    continue;
                }
            }

            if let Err(e) = self.serve_one(req).await {
                // A transport-level failure ends the actor; the in-flight
                // request was already answered with the error inside serve_one.
                let _ = e;
                return;
            }
        }
    }

    /// Write one request and read frames until its matching response, servicing
    /// callbacks inline. All state is local — no lock crosses an await.
    async fn serve_one(&mut self, req: ActorRequest) -> TsgoApiResult<()> {
        let frame = encode_frame(MessageType::Request, req.method.as_bytes(), &req.payload);
        if let Err(e) = self.transport.send_frame(&frame).await {
            let _ = req.reply.send(Err(e));
            return Err(TsgoApiError::Transport("send failed".into()));
        }

        loop {
            let raw = match self.transport.recv_frame().await {
                Ok(Some(bytes)) => bytes,
                Ok(None) => {
                    let _ = req.reply.send(Err(TsgoApiError::Transport(
                        "engine closed the connection".into(),
                    )));
                    return Err(TsgoApiError::Transport("eof".into()));
                }
                Err(e) => {
                    let _ = req.reply.send(Err(e));
                    return Err(TsgoApiError::Transport("recv failed".into()));
                }
            };

            let (decoded, _) = match decode_frame(&raw, 0) {
                Ok(d) => d,
                Err(e) => {
                    let _ = req.reply.send(Err(e));
                    return Err(TsgoApiError::Codec("frame decode".into()));
                }
            };

            match decoded.msg_type {
                MessageType::Response => {
                    // Name correlation: the response name must match the request
                    // method (syncChannel.js:208-214).
                    if decoded.name != req.method.as_bytes() {
                        let _ = req.reply.send(Err(TsgoApiError::Codec(format!(
                            "response name mismatch: expected `{}`",
                            req.method
                        ))));
                        return Err(TsgoApiError::Codec("name mismatch".into()));
                    }
                    // If the request was cancelled mid-flight, discard the result
                    // (the handle future already resolved to Cancelled).
                    let payload = decoded.payload.to_vec();
                    if req
                        .cancel
                        .as_ref()
                        .map(|t| t.is_cancelled())
                        .unwrap_or(false)
                    {
                        // Drop the reply (cancelled); the response is consumed
                        // and the wire is clean for the next request.
                        return Ok(());
                    }
                    let _ = req.reply.send(Ok(payload));
                    return Ok(());
                }
                MessageType::Error => {
                    let msg = String::from_utf8_lossy(decoded.payload).into_owned();
                    let _ = req.reply.send(Err(TsgoApiError::Transport(format!(
                        "engine error for `{}`: {msg}",
                        req.method
                    ))));
                    return Ok(());
                }
                MessageType::Call => {
                    // Service the host FS callback synchronously from the
                    // published snapshot. NEVER awaits the client.
                    let name = String::from_utf8_lossy(decoded.name).into_owned();
                    let reply_frame = self.service_callback(&name, decoded.payload);
                    if let Err(e) = self.transport.send_frame(&reply_frame).await {
                        let _ = req.reply.send(Err(e));
                        return Err(TsgoApiError::Transport("callback send failed".into()));
                    }
                    // Continue reading frames for the same request.
                }
                other => {
                    let _ = req.reply.send(Err(TsgoApiError::Codec(format!(
                        "unexpected message type from engine: {other:?}"
                    ))));
                    return Err(TsgoApiError::Codec("unexpected frame".into()));
                }
            }
        }
    }

    /// Build the `CALL_RESPONSE`/`CALL_ERROR` frame for a host callback, reading
    /// the published overlay snapshot synchronously. Mirrors the host-side wrap
    /// semantics in `sync/client.js:34-44`.
    fn service_callback(&self, name: &str, arg_payload: &[u8]) -> Vec<u8> {
        let snapshot = self.snapshot.load();
        match service_fs_callback(&snapshot, name, arg_payload) {
            Ok(result_json) => encode_frame(
                MessageType::CallResponse,
                name.as_bytes(),
                result_json.as_bytes(),
            ),
            Err(err_msg) => {
                encode_frame(MessageType::CallError, name.as_bytes(), err_msg.as_bytes())
            }
        }
    }
}

/// Pure callback servicing: given the snapshot, a callback name, and the JSON
/// argument bytes (a JSON-encoded path string), produce the JSON response
/// string the host must send back. Mirrors `sync/client.js:34-44`.
///
/// Extracted as a free function so it is unit-testable without an actor.
pub(crate) fn service_fs_callback(
    snapshot: &OverlaySnapshot,
    name: &str,
    arg_payload: &[u8],
) -> Result<String, String> {
    // The wire arg is a JSON-encoded string path (client.js:34 `JSON.parse(arg)`).
    let path: String = serde_json::from_slice(arg_payload)
        .map_err(|e| format!("callback `{name}`: bad argument JSON: {e}"))?;

    match name {
        "readFile" => {
            // readFile wraps: undefined -> "" (fall through), null -> {content:null},
            // string -> {content:"…"} (client.js:36-42).
            match snapshot.read_file(&path) {
                ReadFileResult::FallThrough => Ok(String::new()),
                ReadFileResult::NotFound => Ok(r#"{"content":null}"#.to_string()),
                ReadFileResult::Found(content) => serde_json::to_string(&ReadFileWrap {
                    content: Some(content),
                })
                .map_err(|e| e.to_string()),
            }
        }
        "fileExists" => encode_optional_bool(snapshot.file_exists(&path)),
        "directoryExists" => encode_optional_bool(snapshot.directory_exists(&path)),
        "realpath" => match snapshot.realpath(&path) {
            Some(p) => serde_json::to_string(&p).map_err(|e| e.to_string()),
            None => Ok(String::new()),
        },
        "getAccessibleEntries" => match snapshot.get_accessible_entries(&path) {
            Some(entries) => serde_json::to_string(&AccessibleEntriesWire {
                files: entries.files,
                directories: entries.directories,
            })
            .map_err(|e| e.to_string()),
            None => Ok(String::new()),
        },
        other => Err(format!("unknown callback `{other}`")),
    }
}

/// Encode an `Option<bool>` callback result: `Some` -> `"true"`/`"false"`,
/// `None` -> empty string (fall through). Mirrors the generic
/// `JSON.stringify(result) ?? ""` path (client.js:43).
fn encode_optional_bool(v: Option<bool>) -> Result<String, String> {
    match v {
        Some(b) => Ok(if b {
            "true".to_string()
        } else {
            "false".to_string()
        }),
        None => Ok(String::new()),
    }
}

#[derive(serde::Serialize)]
struct ReadFileWrap {
    content: Option<String>,
}

#[derive(serde::Serialize)]
struct AccessibleEntriesWire {
    files: Vec<String>,
    directories: Vec<String>,
}
