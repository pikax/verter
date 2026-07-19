//! TSGO transport: LSP JSON-RPC over stdio, plus the OWNED one-instance
//! dual-surface provider that attaches an `--api` checker to the same process.
//!
//! tsgo binary DISCOVERY lives in `verter_tsgo_api::toolchain::discovery`
//! (the 4-tier resolver) — this module is the provider/transport, not
//! provisioning.

pub mod ipc;
pub mod owned;

// Facade: the toolchain resolver, re-exported for crates that cannot depend
// on `verter_tsgo_api` directly (verter_session's tsgo-generation-only guard
// bans any `tsgo`-named dependency; oracle-gen reaches the engine through
// this crate instead). This is a re-export of the SINGLE provisioning path,
// not a second implementation.
pub use verter_tsgo_api::toolchain::{discovery, validation};

// Re-export the main types and functions
pub use ipc::{
    offset_to_position_with_encoding, position_to_offset_with_encoding, TsgoApiSession,
    TsgoTypeProvider,
};
pub use owned::{
    position_carrier_diagnostics, select_configured_project_carrier, TsgoOwnedProvider,
};
