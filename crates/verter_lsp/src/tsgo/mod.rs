//! TSGO-specific transport and respawn strategy.
//!
//! Provider-neutral integration logic (traits, protocol, merging, auto-import,
//! project sync, mock) lives in [`crate::type_provider`]. Only the TSGO IPC
//! transport and the TSGO-specific resilient respawn wrapper live here.

pub mod ipc;
pub mod resilient;
