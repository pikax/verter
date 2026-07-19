//! Rust-native client for the tsgo (`typescript-go`) `--api` protocol.
//!
//! This crate drives the tsgo `--api` MessagePack protocol DIRECTLY from Rust.
//! The official generated JavaScript `--api` client (shipped in the rc
//! `typescript` package) is used ONLY as a test/parity oracle and is never
//! referenced from production code here.
//!
//! # Wire protocol (the source of truth)
//!
//! The wire is the tsgo "tuple protocol": every frame is a MessagePack
//! 3-element fixarray `[MessageType, name, payload]` where `MessageType` is a
//! `u8`, `name` is the method/callback name as a `bin` field, and `payload` is
//! a `bin` field carrying a UTF-8 JSON document for the high-level ops. This
//! crate hand-writes the MessagePack subset and the frame codec by mirroring
//! the shipped rc `typescript` reference implementation; the mirrored source
//! lines are cited in [`proto`].
//!
//! # Module layout
//!
//! - [`proto`] — hand-written wire codec: the MessagePack subset, the tuple
//!   frame, the typed request/response DTOs, and the maintained wire pin.
//! - [`jsonrpc`] — the vscode-jsonrpc transport the `--api` ATTACH path speaks
//!   (`Content-Length` + JSON-RPC 2.0 over a server-minted pipe/UDS), reusing the
//!   [`proto::types`] DTOs. Distinct from, and alongside, the standalone wire.
//! - [`relay`] — the gated carrier-injection write surface
//!   ([`relay::CarrierInjectionChannel`], deny-by-default) and the
//!   bidirectional editor↔server `--lsp` frame relay ([`relay::LspRelay`]).
//! - `egress` (crate-private) — the deny-by-default server→editor carrier
//!   egress policy the relay's server→editor pump enforces.
//! - [`gate`] — the runtime fail-closed wire gate (refuses a diverged tsgo).
//! - [`error`] — typed crate errors.

pub mod actor;
pub mod api_attach;
pub mod attach;
pub mod client;
pub mod control;
mod egress;
pub mod error;
#[cfg(feature = "test-fake-engine")]
pub mod fake_engine;
pub mod gate;
pub mod jsonrpc;
pub mod lane;
pub mod offset;
pub mod proto;
pub mod relay;
pub mod snapshot;
pub mod toolchain;
pub mod transport;

pub use actor::{ClientHandle, RequestOptions};
pub use api_attach::{ApiAttachClient, AttachSnapshot};
pub use attach::{
    ApiSessionHandle, AttachOwnership, NonOwning, Owned, SpawnOwnTsgoLsp, TsgoAttach,
    TsgoLspConnection, TsgoLspConnectionSource,
};
pub use client::TsgoClient;
pub use error::{TsgoApiError, TsgoApiResult};
pub use lane::Lane;
pub use offset::{api_offset_to_byte, api_offset_to_line_col, diagnostic_byte_span};
pub use relay::{CarrierInjectionChannel, InitializedWitness, LspRelay};
pub use snapshot::{
    AccessibleEntries, OverlaySnapshot, OverlaySnapshotBuilder, ReadFileResult, RealDirSource,
};
