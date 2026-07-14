//! The `verter_lsp`-side control client: the stable client half of the control
//! protocol.
//!
//! A client discovers a shim by its advertisement (never "attach to the first
//! live shim"), connects the advertised control endpoint, and drives the
//! semantic attach surface: `verter/hello` (presenting the advertised nonce),
//! `verter/waitInitialized` (the in-band witness barrier), the carrier
//! lifecycle, `verter/initializeApiSession` (→ the minted `--api` endpoint the
//! client connects DIRECTLY with the crate's attach client), and
//! `verter/detach`. It never touches a raw editor↔tsgo wire — the shim owns
//! that.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use tokio::io::{AsyncRead, AsyncWrite};

use crate::error::{TsgoApiError, TsgoApiResult};
use crate::jsonrpc::{JsonRpcConnection, NotificationHandler};

use super::messages::{
    CarrierDidChangeSyncedParams, CarrierDidCloseParams, CarrierDidOpenSyncedParams, DetachParams,
    HelloParams, HelloResult, InitializeApiSessionResult, StatusResult, WaitInitializedResult,
    METHOD_CARRIER_DID_CHANGE_SYNCED, METHOD_CARRIER_DID_CLOSE, METHOD_CARRIER_DID_OPEN_SYNCED,
    METHOD_DETACH, METHOD_FATAL, METHOD_HELLO, METHOD_INITIALIZE_API_SESSION, METHOD_STATUS,
    METHOD_WAIT_INITIALIZED, PROTOCOL_VERSION,
};
use super::transport::connect_control_endpoint;

/// The stable control client. Wraps a JSON-RPC connection over the shim's
/// advertised control endpoint.
pub struct ControlClient {
    conn: JsonRpcConnection,
    session: Option<HelloResult>,
    /// Flipped to `true` when the shim emits a `verter/fatal` notification (the
    /// relay/engine is going away). Combined with the connection-closed signal, this
    /// is the control-attach liveness a caller reads to EVICT a dead SHARED transport.
    fatal: Arc<AtomicBool>,
}

impl ControlClient {
    /// Connect to a shim's advertised control endpoint (a `\\.\pipe\…` name on
    /// Windows, a UDS path on Unix). Does NOT run the handshake — call
    /// [`Self::hello`] next.
    pub async fn connect(endpoint: &str) -> TsgoApiResult<Self> {
        let (read, write) = connect_control_endpoint(endpoint).await?;
        Ok(Self::from_transport(read, write))
    }

    /// Build a control client over a split control transport, installing the
    /// `verter/fatal` liveness handler. The [`Self::connect`] path is the production
    /// entry; this is the reusable transport-level constructor (also drives the
    /// in-process duplex tests).
    pub fn from_transport<R, W>(read: R, write: W) -> Self
    where
        R: AsyncRead + Unpin + Send + 'static,
        W: AsyncWrite + Unpin + Send + 'static,
    {
        let fatal = Arc::new(AtomicBool::new(false));
        let notification: NotificationHandler = {
            let fatal = Arc::clone(&fatal);
            Arc::new(move |method: &str, _params: &serde_json::Value| {
                // A `verter/fatal` notification (relay death / engine exit) marks the
                // control attach dead so the SHARED overlay evicts and fails closed.
                if method == METHOD_FATAL {
                    fatal.store(true, Ordering::Release);
                }
            })
        };
        let conn = JsonRpcConnection::connect_with_handlers(
            read,
            write,
            Arc::new(|_method, _params| serde_json::Value::Null),
            notification,
        );
        Self {
            conn,
            session: None,
            fatal,
        }
    }

    /// Perform the `verter/hello` handshake: present the advertised `nonce`
    /// (the server refuses a mismatch) and the client's [`PROTOCOL_VERSION`].
    /// Fails closed if the server reports a different protocol version.
    pub async fn hello(&mut self, nonce: &str, client: &str) -> TsgoApiResult<HelloResult> {
        let params = to_params(&HelloParams {
            protocol: PROTOCOL_VERSION,
            nonce: nonce.to_string(),
            client: client.to_string(),
        })?;
        let value = self.conn.request(METHOD_HELLO, params).await?;
        let result: HelloResult = from_result(value)?;
        if result.protocol != PROTOCOL_VERSION {
            return Err(TsgoApiError::UnsupportedTsgoWire(format!(
                "control server speaks protocol {}, client speaks {PROTOCOL_VERSION}",
                result.protocol
            )));
        }
        self.session = Some(result.clone());
        Ok(result)
    }

    /// Wrap an already-connected control transport (an in-process loopback for
    /// tests, or any pre-established [`JsonRpcConnection`]). The `connect` path
    /// is the documented production entry.
    #[must_use]
    pub fn from_connection(conn: JsonRpcConnection) -> Self {
        Self {
            conn,
            session: None,
            fatal: Arc::new(AtomicBool::new(false)),
        }
    }

    /// The hello result from a completed handshake, if any.
    #[must_use]
    pub fn session(&self) -> Option<&HelloResult> {
        self.session.as_ref()
    }

    /// Whether the control attach is still LIVE: no `verter/fatal` was received AND
    /// the underlying connection is not closed (peer EOF / transport error). A caller
    /// — the SHARED overlay — reads this to EVICT a dead transport: a fatal/closed
    /// control channel means the shim's relay / engine is gone, so the SHARED provider
    /// must fall closed to the OWNED baseline rather than keep hitting a dead path.
    #[must_use]
    pub fn is_alive(&self) -> bool {
        !self.fatal.load(Ordering::Acquire) && !self.conn.is_closed()
    }

    /// Block until the shim's relay has observed the editor→tsgo `initialize`
    /// response, returning the in-band witness (`serverInfo.version` + the
    /// editor's `initialize` id + workspace params).
    pub async fn wait_initialized(&self) -> TsgoApiResult<WaitInitializedResult> {
        let value = self
            .conn
            .request(METHOD_WAIT_INITIALIZED, serde_json::json!({}))
            .await?;
        from_result(value)
    }

    /// Inject an off-disk carrier overlay and synchronize it (didOpen + barrier)
    /// through the shim's gated injection channel.
    pub async fn carrier_did_open_synced(
        &self,
        uri: &str,
        language_id: &str,
        version: i64,
        text: &str,
    ) -> TsgoApiResult<()> {
        let params = to_params(&CarrierDidOpenSyncedParams {
            uri: uri.to_string(),
            language_id: language_id.to_string(),
            version,
            text: text.to_string(),
        })?;
        self.conn
            .request(METHOD_CARRIER_DID_OPEN_SYNCED, params)
            .await?;
        Ok(())
    }

    /// Update an open carrier overlay (didChange + barrier).
    pub async fn carrier_did_change_synced(
        &self,
        uri: &str,
        version: i64,
        text: &str,
    ) -> TsgoApiResult<()> {
        let params = to_params(&CarrierDidChangeSyncedParams {
            uri: uri.to_string(),
            version,
            text: text.to_string(),
        })?;
        self.conn
            .request(METHOD_CARRIER_DID_CHANGE_SYNCED, params)
            .await?;
        Ok(())
    }

    /// Retract a carrier overlay (didClose).
    pub async fn carrier_did_close(&self, uri: &str) -> TsgoApiResult<()> {
        let params = to_params(&CarrierDidCloseParams {
            uri: uri.to_string(),
        })?;
        self.conn.request(METHOD_CARRIER_DID_CLOSE, params).await?;
        Ok(())
    }

    /// Ask the shim to mint an `--api` session and return its endpoint (the
    /// server-minted pipe/UDS path the client connects DIRECTLY with the
    /// crate's attach client).
    pub async fn initialize_api_session(&self) -> TsgoApiResult<InitializeApiSessionResult> {
        let value = self
            .conn
            .request(METHOD_INITIALIZE_API_SESSION, serde_json::json!({}))
            .await?;
        from_result(value)
    }

    /// A control-session status snapshot.
    pub async fn status(&self) -> TsgoApiResult<StatusResult> {
        let value = self
            .conn
            .request(METHOD_STATUS, serde_json::json!({}))
            .await?;
        from_result(value)
    }

    /// Detach: optionally retract Verter's carriers, then ask the shim to tear
    /// down. A closed connection AFTER the request is sent is treated as
    /// success — the shim going away is the intended effect of detach.
    pub async fn detach(&self, close_carriers: bool) -> TsgoApiResult<()> {
        // The client sends the preference EXPLICITLY (`Some`), so the server's
        // fail-closed default (an omitted/malformed body ⇒ retract) never applies here.
        let params = to_params(&DetachParams {
            close_carriers: Some(close_carriers),
        })?;
        match self.conn.request(METHOD_DETACH, params).await {
            Ok(_) | Err(TsgoApiError::Closed) => Ok(()),
            Err(e) => Err(e),
        }
    }

    /// Close the control connection (client side). Does NOT tear the shim down —
    /// use [`Self::detach`] for that.
    pub async fn close(&self) -> TsgoApiResult<()> {
        self.conn.close().await
    }
}

/// Serialize a control param struct into a JSON-RPC `params` value.
fn to_params<T: serde::Serialize>(value: &T) -> TsgoApiResult<serde_json::Value> {
    serde_json::to_value(value)
        .map_err(|e| TsgoApiError::Json(format!("control params encode: {e}")))
}

/// Deserialize a JSON-RPC `result` value into a typed control result.
fn from_result<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> TsgoApiResult<T> {
    serde_json::from_value(value)
        .map_err(|e| TsgoApiError::Json(format!("control result decode: {e}")))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::jsonrpc::framing::encode_message;
    use tokio::io::AsyncWriteExt;

    /// The control attach's liveness (`is_alive`) flips DEAD when the shim emits a
    /// `verter/fatal` notification: the SHARED overlay reads this to evict the dead
    /// transport and fall closed to OWNED. Discriminating: without the notification
    /// handler `is_alive()` would stay `true` on a dead relay.
    #[tokio::test]
    async fn is_alive_flips_dead_on_fatal_notification() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (cr, cw) = tokio::io::split(client);
        let (_sr, mut sw) = tokio::io::split(server);

        let ctl = ControlClient::from_transport(cr, cw);
        assert!(ctl.is_alive(), "a fresh control attach is alive");

        // The shim emits `verter/fatal` (relay death / engine exit).
        let fatal = serde_json::json!({
            "jsonrpc": "2.0",
            "method": METHOD_FATAL,
            "params": { "reason": "relay_death", "detail": "relay stopped pumping" }
        });
        sw.write_all(&encode_message(&fatal)).await.unwrap();
        sw.flush().await.unwrap();

        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            !ctl.is_alive(),
            "a verter/fatal notification must mark the control attach DEAD"
        );
    }

    /// The control attach's liveness also flips DEAD when the control connection
    /// closes (the shim process died / the pipe dropped), independent of any
    /// `verter/fatal` notification.
    #[tokio::test]
    async fn is_alive_flips_dead_on_connection_close() {
        let (client, server) = tokio::io::duplex(64 * 1024);
        let (cr, cw) = tokio::io::split(client);
        let (sr, sw) = tokio::io::split(server);

        let ctl = ControlClient::from_transport(cr, cw);
        assert!(ctl.is_alive(), "a fresh control attach is alive");

        drop(sr);
        drop(sw);
        tokio::time::sleep(std::time::Duration::from_millis(50)).await;
        assert!(
            !ctl.is_alive(),
            "a closed control connection must mark the attach DEAD"
        );
    }
}
