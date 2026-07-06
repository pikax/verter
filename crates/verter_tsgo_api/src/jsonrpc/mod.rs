//! The vscode-jsonrpc transport the tsgo `--api` ATTACH path speaks.
//!
//! Distinct from the crate's STANDALONE MessagePack tuple wire ([`crate::proto`]):
//! the attach path connects to a server-minted pipe (named pipe on Windows, a
//! Unix-domain socket on macOS/Linux) and exchanges JSON-RPC 2.0 envelopes framed
//! with `Content-Length` headers — the framing the shipped rc `typescript`
//! async client uses (`createMessageConnection` over a `net.Socket`).
//!
//! ## Layout
//!
//! - [`framing`] — the `Content-Length` + JSON-RPC body codec (encode / a
//!   streaming decoder) plus the base64 `{ data }` binary-result helpers.
//! - [`connection`] — an `id`-correlated JSON-RPC 2.0 connection over any async
//!   byte stream (serves BOTH the `--lsp` stdio side and the `--api` pipe side).
//!
//! The typed request/response DTOs are SHARED with the standalone path
//! ([`crate::proto::types`]) — only the framing and transport differ.

pub mod connection;
pub mod framing;

pub use connection::{JsonRpcConnection, NotificationHandler, ServerRequestHandler};
pub use framing::{decode_base64_data, encode_base64_data, encode_message, MessageFramer};
