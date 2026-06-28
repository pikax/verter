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
    /// The client/actor was shut down and can no longer serve requests.
    Closed,
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
            TsgoApiError::Closed => write!(f, "tsgo client closed"),
        }
    }
}

impl std::error::Error for TsgoApiError {}

impl From<serde_json::Error> for TsgoApiError {
    fn from(e: serde_json::Error) -> Self {
        TsgoApiError::Json(e.to_string())
    }
}
