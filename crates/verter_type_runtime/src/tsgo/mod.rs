//! TSGO transport: LSP JSON-RPC over stdio.

pub mod ipc;

// Re-export the main types and functions
pub use ipc::{
    find_tsgo_binary, offset_to_position_with_encoding, position_to_offset_with_encoding,
    TsgoBinaryLookupError, TsgoTypeProvider,
};
