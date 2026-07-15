//! Session-owned runtime for query execution against a shared host.
//!
//! `SessionRuntime` is the reusable owner for a session's query surface.
//! It owns the session's identity and acts as a thin facade that
//! routes queries to the host's shared caches.
//!
//! Sessions never mutate the host (R17): overlay-aware reads route
//! through [`crate::session_view::SessionView`] and the host's shared
//! caches are the source of truth. Cross-session concurrency is
//! separated by the existing `StoreViewCompatToken`-keyed
//! singleflight on the resolver runtime (R19, R20).

use crate::meta::MetaProject;
use crate::meta_resolve::ResolvedComponentMetaState;
use crate::types::ProjectionMode;
use std::sync::Arc;

/// Reusable runtime owner for a session's query execution.
///
/// Owns a back-reference to the project. All query state lives on the
/// host's shared caches; the runtime is a thin facade that forwards
/// reads through the host's existing accessors. (The session identity
/// is owned by `MetaSession`; the retired session-scoped request-host
/// substrate was the runtime's only identity consumer.)
pub(crate) struct SessionRuntime {
    project: Arc<MetaProject>,
}

impl SessionRuntime {
    pub fn new(project: Arc<MetaProject>) -> Self {
        Self { project }
    }

    pub fn host(&self) -> &crate::VerterHost {
        self.project.host()
    }

    #[allow(dead_code)]
    pub fn project(&self) -> &Arc<MetaProject> {
        &self.project
    }

    // -------------------------------------------------------------------
    // Resolved-meta cache facade
    // -------------------------------------------------------------------

    /// Try to get a cached resolved-meta from the host's shared cache.
    ///
    /// There is no session-scoped `resolved_meta_cache` (R20). The
    /// host's shared cache is fact-validated against the session's
    /// overlay-aware `StoreView` (re-rooted via
    /// `HostStoreView::with_session_overlay`) so overlay-derived state
    /// cannot leak across concurrent sessions when the multi-candidate
    /// substrate is consulted. Where the multi-candidate substrate is
    /// not yet in the read path, overlay sessions transparently read
    /// base-state results.
    #[allow(dead_code)]
    pub fn try_get_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ProjectionMode,
    ) -> Option<ResolvedComponentMetaState> {
        self.host().try_get_cached_resolved_meta(canonical, mode)
    }

    /// Invalidate session-scoped caches — no-op.
    ///
    /// There is no session-scoped cache (R17 — overlay mutation is
    /// not allowed); all reads go through the host's shared cache
    /// directly. Retained as a method so the public
    /// `MetaSession::invalidate_active_overlays` API surface stays
    /// stable for callers that may still invoke it from FFI paths.
    pub fn invalidate_session_caches(&self) {}
}
