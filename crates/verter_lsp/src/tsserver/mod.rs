//! TypeScript language service provider via tsserver.
//!
//! Uses the standard `tsserver.js` protocol (newline-delimited JSON over stdio)
//! with resolver-managed provider files supplied by the LSP.
//!
//! This is an alternative to TSGO for users who don't have the Go-based
//! TypeScript server available. Engines are PROJECT-BOUND: production serving
//! runs through [`project_router::ProjectTsserverProvider`], which owns one
//! tsserver per `(owning tsconfig, real tsserver.js)` identity, so each
//! configured project is served by the TypeScript IT installs rather than by one
//! workspace-level engine.

pub mod ipc;
pub mod project_router;
pub mod resilient;

// Re-export discovery helpers from verter_type_runtime
pub use verter_type_runtime::discovery::{find_node, find_tsserver};
