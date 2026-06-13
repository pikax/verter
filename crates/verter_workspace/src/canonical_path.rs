//! Canonical path normalization for workspace-internal path storage.
//!
//! The normalization logic and the [`CanonicalPath`] newtype are owned by the
//! leaf crate `verter_span` so every consumer (LSP, type-runtime, scheduler,
//! workspace) shares one canonical-ID format. This module re-exports them so the
//! public API paths `verter_workspace::CanonicalPath` /
//! `verter_workspace::canonicalize_path` are unchanged.

pub use verter_span::path::{canonicalize_path, canonicalize_path_cow, CanonicalPath};
