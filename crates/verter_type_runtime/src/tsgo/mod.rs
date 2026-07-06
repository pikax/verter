//! TSGO transport: LSP JSON-RPC over stdio, plus the OWNED one-instance
//! dual-surface provider that attaches an `--api` checker to the same process.

pub mod ipc;
pub mod owned;

// Re-export the main types and functions
pub use ipc::{
    find_tsgo_binary, find_tsgo_binary_canonical, find_tsgo_binary_under_node_modules,
    offset_to_position_with_encoding, position_to_offset_with_encoding, TsgoApiSession,
    TsgoBinaryLookupError, TsgoTypeProvider, TSGO_BINARY_ENV,
};
pub use owned::{
    position_carrier_diagnostics, select_configured_project_carrier, TsgoOwnedProvider,
};
