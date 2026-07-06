//! TSGO-specific transport and respawn strategy.
//!
//! Provider-neutral integration logic (traits, protocol, merging, auto-import,
//! project sync, mock) lives in [`crate::type_provider`]. Only the TSGO IPC
//! transport, the TSGO-specific resilient respawn wrapper, and the SHARED
//! editor-attach provider live here.

pub mod composite;
pub mod ipc;
mod overlay_core;
pub mod resilient;
pub mod shared;
pub mod transport_cell;
