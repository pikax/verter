//! Typed errors for the tsgo `--api` client.

use std::fmt;

/// Result alias for crate operations.
pub type TsgoApiResult<T> = Result<T, TsgoApiError>;

/// Errors produced by the tsgo `--api` client.
#[derive(Debug)]
#[non_exhaustive]
pub enum TsgoApiError {
    /// A MessagePack/frame decode failed because the bytes did not match the
    /// expected wire shape (wrong marker, truncated field, unexpected tag).
    Codec(String),
    /// A JSON payload inside a frame failed to (de)serialize.
    Json(String),
    /// The installed tsgo wire did not match the maintained pin, so the client
    /// refuses to start (fail-closed wire gate).
    UnsupportedTsgoWire(String),
    /// The tsgo binary could not be discovered or spawned.
    Spawn(String),
    /// Low-level transport I/O failure (pipe read/write, child exit).
    Transport(String),
    /// The request was cancelled before (or while) it was in flight.
    Cancelled,
    /// A bounded operation exceeded its timeout before its transport round-trip
    /// completed — e.g. the carrier-sync barrier
    /// ([`crate::relay::CarrierInjectionChannel::sync_overlay`]) did not round-trip
    /// within its bound. The ordering guarantee could NOT be established, so the caller
    /// fails CLOSED (degrades to the OWNED baseline) rather than blocking indefinitely.
    Timeout(String),
    /// The client/actor was shut down and can no longer serve requests.
    Closed,
    /// A write through the gated carrier-injection channel was refused
    /// BEFORE reaching the wire: the `(method, kind)` pair is not admitted by
    /// the deny-by-default allowlist — the method is not a carrier method, or
    /// it was sent as the wrong JSON-RPC kind (a notification-only method as a
    /// request, or a request-only method as a notification), or it is a
    /// stateful overlay open/close reached outside `did_open`/`did_close`.
    WriteGateDenied {
        /// The refused JSON-RPC method name.
        method: String,
    },
}

impl fmt::Display for TsgoApiError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            TsgoApiError::Codec(m) => write!(f, "tsgo wire codec error: {m}"),
            TsgoApiError::Json(m) => write!(f, "tsgo payload JSON error: {m}"),
            TsgoApiError::UnsupportedTsgoWire(m) => {
                write!(f, "unsupported tsgo wire (refusing to start): {m}")
            }
            TsgoApiError::Spawn(m) => write!(f, "tsgo spawn error: {m}"),
            TsgoApiError::Transport(m) => write!(f, "tsgo transport error: {m}"),
            TsgoApiError::Cancelled => write!(f, "tsgo request cancelled"),
            TsgoApiError::Timeout(m) => write!(f, "tsgo operation timed out: {m}"),
            TsgoApiError::Closed => write!(f, "tsgo client closed"),
            TsgoApiError::WriteGateDenied { method } => write!(
                f,
                "carrier-injection write-gate denied method `{method}`: not \
                 admitted by the deny-by-default allowlist"
            ),
        }
    }
}

impl std::error::Error for TsgoApiError {}

impl From<serde_json::Error> for TsgoApiError {
    fn from(e: serde_json::Error) -> Self {
        TsgoApiError::Json(e.to_string())
    }
}
