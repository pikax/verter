//! An id-correlated JSON-RPC 2.0 connection over an async byte stream.
//!
//! This is the connection layer the tsgo `--api` ATTACH path drives. It is
//! GENERIC over the underlying transport (`AsyncRead + AsyncWrite`), so the SAME
//! connection type serves BOTH the `tsgo --lsp` stdio side (where the attach
//! handshake `custom/initializeAPISession` is sent) AND the `--api` checker pipe
//! side (a server-minted named pipe / UDS). It is therefore a reusable primitive:
//! it operates on "a connection", never a hardcoded process or pipe source.
//!
//! Unlike the standalone MessagePack actor ([`crate::actor`]) — which is
//! NAME-correlated single-flight — vscode-jsonrpc carries a request `id`, so this
//! connection is `id`-correlated and may have multiple requests in flight.
//!
//! ## Cancellation
//!
//! The `--api` checker has NO wire-level cancellation (verified against the
//! shipped client). Cancellation here is therefore ABANDON-ONLY: dropping the
//! future returned by [`JsonRpcConnection::request`] removes its pending waiter so
//! a late response is discarded; the engine may still compute the obsolete request
//! to completion. (The `--lsp` side additionally has `$/cancelRequest`, sent as a
//! notification via [`JsonRpcConnection::notify`].)

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicI64, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::io::{AsyncRead, AsyncReadExt, AsyncWrite, AsyncWriteExt};
use tokio::sync::{mpsc, oneshot};

use crate::error::{TsgoApiError, TsgoApiResult};
use crate::jsonrpc::framing::{encode_message, MessageFramer};

/// Build a JSON-RPC message body, OMITTING the `params` key entirely when the
/// caller has none (`Value::Null`). LSP methods like `shutdown`/`exit` declare
/// NO params; sending `"params": null` makes strict engines (tsgo) log
/// `InvalidParams: expected no params, got null` while handling every teardown.
fn jsonrpc_message(id: Option<i64>, method: &str, params: &serde_json::Value) -> serde_json::Value {
    let mut msg = serde_json::Map::new();
    msg.insert("jsonrpc".into(), serde_json::Value::from("2.0"));
    if let Some(id) = id {
        msg.insert("id".into(), serde_json::Value::from(id));
    }
    msg.insert("method".into(), serde_json::Value::from(method));
    if !params.is_null() {
        msg.insert("params".into(), params.clone());
    }
    serde_json::Value::Object(msg)
}

/// A handler invoked for each server→client request the peer sends (e.g. the
/// `tsgo --lsp` server's `workspace/configuration` / `client/registerCapability`).
/// It returns the `result` value to answer with. The default
/// ([`JsonRpcConnection::connect`]) answers every server→client request with
/// `null`, which is what the attach handshake needs (Verter drives documents via
/// `--lsp` didOpen, not via server callbacks on the `--api` pipe).
pub type ServerRequestHandler =
    Arc<dyn Fn(&str, &serde_json::Value) -> serde_json::Value + Send + Sync>;

/// A handler invoked for each server→client NOTIFICATION (a frame with a `method`
/// but no `id`) the peer sends — e.g. the control server's `verter/fatal` liveness
/// signal. The default ([`JsonRpcConnection::connect`] /
/// [`JsonRpcConnection::connect_with_handler`]) is a no-op (notifications ignored);
/// a caller that must react to a peer notification installs one via
/// [`JsonRpcConnection::connect_with_handlers`].
pub type NotificationHandler = Arc<dyn Fn(&str, &serde_json::Value) + Send + Sync>;

/// A pending-request table shared between the public handle and the read task.
type Pending = Arc<Mutex<HashMap<i64, oneshot::Sender<serde_json::Value>>>>;

/// One outbound frame queued for the writer task.
enum Outbound {
    Frame(Vec<u8>),
    Close,
}

/// A live JSON-RPC 2.0 connection. Cloneable handle; the read + write tasks run in
/// the background and shut down when the last handle is dropped or
/// [`JsonRpcConnection::close`] is called.
#[derive(Clone)]
pub struct JsonRpcConnection {
    out_tx: mpsc::Sender<Outbound>,
    pending: Pending,
    next_id: Arc<AtomicI64>,
    /// Flipped to `true` when the reader task ends (peer EOF / transport error /
    /// malformed frame) or the connection is closed — the connection-death liveness
    /// signal a caller (e.g. the SHARED overlay) reads to evict a dead transport.
    closed: Arc<AtomicBool>,
}

impl std::fmt::Debug for JsonRpcConnection {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("JsonRpcConnection")
            .field("pending", &self.pending.lock().len())
            .finish_non_exhaustive()
    }
}

/// A waiter guard that removes its pending entry on drop (abandon-only cancel): if
/// the caller drops the request future before the response arrives, the entry is
/// pruned so the read task discards the late response instead of leaking it.
struct PendingGuard {
    id: i64,
    pending: Pending,
    armed: bool,
}

impl Drop for PendingGuard {
    fn drop(&mut self) {
        if self.armed {
            self.pending.lock().remove(&self.id);
        }
    }
}

impl JsonRpcConnection {
    /// Wrap a split async transport in a connection, answering every
    /// server→client request with `null`.
    pub fn connect<R, W>(reader: R, writer: W) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::connect_with_handler(
            reader,
            writer,
            Arc::new(|_method, _params| serde_json::Value::Null),
        )
    }

    /// Wrap a split async transport, using `handler` to answer server→client
    /// requests. Notifications (no `id`) from the peer are ignored.
    pub fn connect_with_handler<R, W>(reader: R, writer: W, handler: ServerRequestHandler) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        Self::connect_with_handlers(reader, writer, handler, Arc::new(|_method, _params| {}))
    }

    /// Wrap a split async transport with BOTH a server→client request `handler` and
    /// a peer-`notification` handler. The notification handler is invoked for each
    /// peer notification (a `method` with no `id`) — e.g. the control server's
    /// `verter/fatal` liveness signal — instead of the frame being ignored.
    pub fn connect_with_handlers<R, W>(
        reader: R,
        writer: W,
        handler: ServerRequestHandler,
        notification: NotificationHandler,
    ) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let pending: Pending = Arc::new(Mutex::new(HashMap::new()));
        let (out_tx, out_rx) = mpsc::channel::<Outbound>(256);
        let closed = Arc::new(AtomicBool::new(false));

        // Writer task: drains the outbound queue onto the transport.
        tokio::spawn(writer_task(writer, out_rx));
        // Reader task: frames inbound bytes, routes responses to waiters,
        // auto-answers server→client requests through `handler`, routes peer
        // notifications to `notification`, and flips `closed` on EOF/error.
        tokio::spawn(reader_task(
            reader,
            Arc::clone(&pending),
            out_tx.clone(),
            handler,
            notification,
            Arc::clone(&closed),
        ));

        Self {
            out_tx,
            pending,
            next_id: Arc::new(AtomicI64::new(1)),
            closed,
        }
    }

    /// Whether the connection is dead: the reader task ended (peer EOF / transport
    /// error / malformed frame) or [`Self::close`] was called. A caller reads this as
    /// a liveness signal — a dead connection can serve no further requests.
    #[must_use]
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    /// Send a JSON-RPC request and await its result. Dropping the returned future
    /// abandons the request (the late response is discarded).
    pub async fn request(
        &self,
        method: &str,
        params: serde_json::Value,
    ) -> TsgoApiResult<serde_json::Value> {
        let id = self.next_id.fetch_add(1, Ordering::Relaxed);
        let (tx, rx) = oneshot::channel();
        self.pending.lock().insert(id, tx);
        let mut guard = PendingGuard {
            id,
            pending: Arc::clone(&self.pending),
            armed: true,
        };

        let msg = jsonrpc_message(Some(id), method, &params);
        self.out_tx
            .send(Outbound::Frame(encode_message(&msg)))
            .await
            .map_err(|_| TsgoApiError::Closed)?;

        match rx.await {
            Ok(value) => {
                guard.armed = false; // response arrived; nothing to prune
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
            // The read task dropped the sender (connection closed) — the guard
            // prunes on drop.
            Err(_) => Err(TsgoApiError::Closed),
        }
    }

    /// Send a JSON-RPC notification (no response expected), e.g. the `--lsp`
    /// `initialized` lifecycle notification or `$/cancelRequest`.
    pub async fn notify(&self, method: &str, params: serde_json::Value) -> TsgoApiResult<()> {
        let msg = jsonrpc_message(None, method, &params);
        self.out_tx
            .send(Outbound::Frame(encode_message(&msg)))
            .await
            .map_err(|_| TsgoApiError::Closed)
    }

    /// Close the connection: stop the writer task and fail every in-flight waiter.
    pub async fn close(&self) -> TsgoApiResult<()> {
        self.closed.store(true, Ordering::Release);
        let _ = self.out_tx.send(Outbound::Close).await;
        // Fail any remaining waiters so callers unblock with `Closed`.
        let mut pending = self.pending.lock();
        pending.clear();
        Ok(())
    }
}

/// The writer task: serialize outbound frames onto the transport in order.
async fn writer_task<W>(mut writer: W, mut out_rx: mpsc::Receiver<Outbound>)
where
    W: AsyncWrite + Unpin + Send + 'static,
{
    while let Some(item) = out_rx.recv().await {
        match item {
            Outbound::Frame(bytes) => {
                if writer.write_all(&bytes).await.is_err() {
                    break;
                }
                if writer.flush().await.is_err() {
                    break;
                }
            }
            Outbound::Close => break,
        }
    }
    let _ = writer.shutdown().await;
}

/// The reader task: frame inbound bytes, route responses to waiters, auto-answer
/// server→client requests, route peer notifications to `notification`, and flip
/// `closed` on EOF / transport error / malformed frame.
async fn reader_task<R>(
    mut reader: R,
    pending: Pending,
    out_tx: mpsc::Sender<Outbound>,
    handler: ServerRequestHandler,
    notification: NotificationHandler,
    closed: Arc<AtomicBool>,
) where
    R: AsyncRead + Unpin + Send + 'static,
{
    let mut framer = MessageFramer::new();
    let mut chunk = [0u8; 8192];
    loop {
        match reader.read(&mut chunk).await {
            Ok(0) => break, // EOF
            Ok(n) => framer.push(&chunk[..n]),
            Err(_) => break,
        }
        loop {
            match framer.next_message() {
                Ok(Some(msg)) => {
                    route_message(&msg, &pending, &out_tx, &handler, &notification).await
                }
                Ok(None) => break,
                // A malformed frame is unrecoverable: mark closed, drop every waiter,
                // and stop.
                Err(_) => {
                    closed.store(true, Ordering::Release);
                    pending.lock().clear();
                    return;
                }
            }
        }
    }
    // EOF / error: the connection is dead — mark closed and unblock every waiter.
    closed.store(true, Ordering::Release);
    pending.lock().clear();
}

/// Route one decoded message: a response (has `id` + `result`/`error`) goes to its
/// waiter; a server→client request (has `id` + `method`) is auto-answered; a peer
/// notification (only `method`) is dispatched to `notification`.
async fn route_message(
    msg: &serde_json::Value,
    pending: &Pending,
    out_tx: &mpsc::Sender<Outbound>,
    handler: &ServerRequestHandler,
    notification: &NotificationHandler,
) {
    let has_id = msg.get("id").map(|v| !v.is_null()).unwrap_or(false);
    let method = msg.get("method").and_then(|m| m.as_str());

    match (has_id, method) {
        // Server→client request: answer it.
        (true, Some(method)) => {
            let id = msg.get("id").cloned().unwrap_or(serde_json::Value::Null);
            let params = msg
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            let result = handler(method, &params);
            let reply = serde_json::json!({ "jsonrpc": "2.0", "id": id, "result": result });
            let _ = out_tx.send(Outbound::Frame(encode_message(&reply))).await;
        }
        // Response to one of our requests.
        (true, None) => {
            if let Some(id) = msg.get("id").and_then(serde_json::Value::as_i64) {
                if let Some(tx) = pending.lock().remove(&id) {
                    let _ = tx.send(msg.clone());
                }
            }
        }
        // Peer notification (no id): dispatch to the notification handler.
        (false, Some(method)) => {
            let params = msg
                .get("params")
                .cloned()
                .unwrap_or(serde_json::Value::Null);
            notification(method, &params);
        }
        // A frame with neither id nor method: nothing to route.
        (false, None) => {}
    }
}

#[cfg(test)]
#[path = "connection_tests.rs"]
mod tests;
