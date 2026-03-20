//! Published workspace state: atomic snapshot + consumer extension.
//!
//! [`PublishedRoot`] is a concrete struct wrapping an immutable
//! [`WorkspaceSnapshot`] with an opaque consumer extension (e.g., LSP views).
//! It is published atomically via `ArcSwapOption::store()`.
//!
//! # Boot-time semantics
//!
//! `ArcSwapOption` starts as `None`. Before first publish, consumers
//! observe `None` and fall back to empty/no-op behavior. After first
//! publish, the value is always `Some`.

use std::any::Any;
use std::sync::Arc;

use crate::workspace_snapshot::WorkspaceSnapshot;

/// The published workspace root: snapshot + consumer extension.
///
/// Published atomically via a single `ArcSwap::store()`. Consumers
/// load via `load()` / `load_full()` and see exactly one generation
/// per request.
///
/// # Consumer extension
///
/// `consumer_ext` is opaque — the VFS layer doesn't know what it
/// contains. The LSP downcasts it to `LspViews` (defined in verter_lsp).
/// Non-LSP consumers set `consumer_ext: None`.
///
/// `Arc<WorkspaceSnapshot>` enables LSP-view-only rebuilds: reuse the
/// existing snapshot Arc, build new views, publish new `PublishedRoot`.
pub struct PublishedRoot {
    /// The immutable workspace snapshot.
    pub snapshot: Arc<WorkspaceSnapshot>,
    /// Opaque consumer extension. LSP downcasts to `LspViews`.
    /// `None` for non-LSP consumers (VFS-only publication).
    pub consumer_ext: Option<Box<dyn Any + Send + Sync>>,
}

impl PublishedRoot {
    /// Create a VFS-only published root (no consumer extension).
    pub fn new_vfs_only(snapshot: Arc<WorkspaceSnapshot>) -> Self {
        Self {
            snapshot,
            consumer_ext: None,
        }
    }

    /// Create a published root with a consumer extension.
    pub fn with_ext(snapshot: Arc<WorkspaceSnapshot>, ext: Box<dyn Any + Send + Sync>) -> Self {
        Self {
            snapshot,
            consumer_ext: Some(ext),
        }
    }

    /// Try to downcast the consumer extension to a concrete type.
    pub fn ext<T: 'static>(&self) -> Option<&T> {
        self.consumer_ext
            .as_ref()
            .and_then(|ext| ext.downcast_ref::<T>())
    }
}

impl std::fmt::Debug for PublishedRoot {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("PublishedRoot")
            .field("generation", &self.snapshot.generation)
            .field("has_ext", &self.consumer_ext.is_some())
            .finish()
    }
}

#[cfg(test)]
#[path = "published_state_tests.rs"]
mod tests;
