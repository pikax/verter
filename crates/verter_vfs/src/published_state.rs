//! Published workspace state: atomic snapshot + consumer extension.
//!
//! [`PublishedRoot`] is a concrete struct wrapping an immutable
//! [`WorkspaceSnapshot`] with an opaque consumer extension (e.g., LSP views).
//! It is published atomically via `ArcSwapOption::store()`.
//!
//! # Boot-time semantics
//!
//! `Engine::new()` eagerly publishes an empty bootstrap snapshot, so
//! `ArcSwapOption` is `Some` immediately after construction. The
//! bootstrap snapshot has `ownership_ready: false` — ownership queries
//! are not yet authoritative. After `background_init` builds the full
//! project graph, a real snapshot with `ownership_ready: true` is
//! published.

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
    /// `false` during bootstrap (empty project graph, ownership queries unreliable).
    /// `true` after a real snapshot with the full project graph is published
    /// (e.g., after `background_init` completes).
    pub ownership_ready: bool,
}

impl PublishedRoot {
    /// Create a VFS-only published root (no consumer extension).
    ///
    /// `ownership_ready` defaults to `false` — this is a bootstrap snapshot
    /// where ownership queries are not yet authoritative.
    pub fn new_vfs_only(snapshot: Arc<WorkspaceSnapshot>) -> Self {
        Self {
            snapshot,
            consumer_ext: None,
            ownership_ready: false,
        }
    }

    /// Create a published root with a consumer extension.
    ///
    /// `ownership_ready` defaults to `true` — the consumer extension (e.g.,
    /// `LspViews`) implies the project graph has been fully built.
    pub fn with_ext(snapshot: Arc<WorkspaceSnapshot>, ext: Box<dyn Any + Send + Sync>) -> Self {
        Self {
            snapshot,
            consumer_ext: Some(ext),
            ownership_ready: true,
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
            .field("ownership_ready", &self.ownership_ready)
            .finish()
    }
}

#[cfg(test)]
#[path = "published_state_tests.rs"]
mod tests;
