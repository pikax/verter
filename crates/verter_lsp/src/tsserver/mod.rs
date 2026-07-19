//! TypeScript language service provider via tsserver.
//!
//! Uses the standard `tsserver.js` protocol (newline-delimited JSON over stdio)
//! with resolver-managed provider files supplied by the LSP.
//!
//! This is an alternative to TSGO for users who don't have the Go-based
//! TypeScript server available. It uses the workspace TypeScript version.

pub mod ipc;
pub mod resilient;

// Re-export discovery helpers from verter_type_runtime
pub use verter_type_runtime::discovery::{find_node, find_tsserver};
