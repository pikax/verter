//! Canonical protocol schema definitions.
//!
//! These are the authoritative transport-facing DTOs for Verter's
//! NAPI/WASM/LSP/MCP boundaries. All schema types derive Serialize +
//! Deserialize and use camelCase field names for JavaScript interop.
//!
//! Consumers (verter_ffi, verter_napi, verter_wasm, verter_lsp, verter_mcp)
//! should import schema types from here rather than defining their own.

pub mod component;
pub mod query;
pub mod refs;
