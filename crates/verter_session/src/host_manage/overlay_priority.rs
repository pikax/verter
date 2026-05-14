//! Overlay-priority resolver-tier helpers used by
//! [`SessionResolverContext`](crate::resolver_core::SessionResolverContext).
//!
//! The wrapper trait impl delegates `ensure_loaded` and
//! `ensure_indexed_ready` here so the overlay-source path is shared
//! between every session-bearing query entry point. Base-only calls
//! continue through the host's own `ensure_*` paths without entering
//! this module.
//!
//! ## Authority chain
//!
//! - `view.is_tombstoned(canonical)` → short-circuit `None` / `false`.
//! - `view.source(canonical)` → publish an overlay-content
//!   [`IndexedReady`](crate::project_type_store::IndexedReady) candidate
//!   keyed by the overlay's content hash, then return it. Base-host
//!   reads never see the candidate because the
//!   [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore)
//!   slot is content-addressed.
//! - View has no overlay → fall through to the inner
//!   [`ResolverContext::ensure_indexed_ready`] / `ensure_loaded` path.

use std::sync::Arc;

use crate::project_type_store::IndexedReady;
use crate::resolver_core::resolver_context::ResolverContext;
use crate::session_view::SessionView;

/// Overlay-priority [`ResolverContext::ensure_loaded`] helper.
///
/// When `view` tombstones the canonical, returns `false` without
/// consulting the inner host. When `view` carries an overlay for the
/// canonical, the overlay source is sufficient — the inner host's
/// scheduler does not need to load anything (returns `true`).
/// Otherwise falls through to the inner host via
/// [`ResolverContext::ensure_loaded`].
pub(crate) fn ensure_loaded_with_view(
    inner: &dyn ResolverContext,
    view: &dyn SessionView,
    canonical_id: &str,
) -> bool {
    if view.is_tombstoned(canonical_id) {
        return false;
    }
    if view.source(canonical_id).is_some() {
        // Overlay source is the authority — the inner host's scheduler
        // load is not required for the overlay candidate. The
        // overlay's IndexedReady is materialised lazily on the first
        // `ensure_indexed_ready_with_view` call below.
        return true;
    }
    inner.ensure_loaded(canonical_id)
}

/// Overlay-priority [`ResolverContext::ensure_indexed_ready`] helper.
///
/// When `view` tombstones the canonical, returns `None`. When `view`
/// has an overlay whose content hash differs from any cached candidate
/// in the [`FileArtifactStore`](crate::file_artifact_store::FileArtifactStore),
/// materialises a parallel IndexedReady from the overlay source and
/// publishes it under the overlay's content hash via
/// [`materialize_overlay_indexed_ready`]. Otherwise falls through to
/// [`ResolverContext::ensure_indexed_ready`].
pub(crate) fn ensure_indexed_ready_with_view(
    inner: &dyn ResolverContext,
    view: &dyn SessionView,
    canonical_id: &str,
) -> Option<Arc<IndexedReady>> {
    if view.is_tombstoned(canonical_id) {
        return None;
    }
    // Overlay-priority: if the view carries an overlay source for the
    // canonical, prefer the overlay candidate. The host's
    // `materialize_overlay_indexed_ready` entry point publishes the
    // candidate to the file-artifact store under the overlay's
    // content hash so future reads of the same overlay reuse the
    // cached candidate.
    if let Some(overlay_source) = view.source(canonical_id) {
        if let Some(overlay_hash) = view.content_hash_for(canonical_id) {
            if let Some(indexed) =
                inner.materialize_overlay_indexed_ready(canonical_id, &overlay_source, overlay_hash)
            {
                return Some(indexed);
            }
        }
    }
    inner.ensure_indexed_ready(canonical_id)
}

/// Pre-warm overlay [`IndexedReady`](crate::project_type_store::IndexedReady)
/// candidates for every canonical the view carries an overlay for.
///
/// Called from session-bearing query entry points before the cold
/// compute runs so the resolver-tier reads on cross-file deps see
/// overlay content via [`FileArtifactStore::get`](crate::file_artifact_store::FileArtifactStore::get).
/// The pre-warm is idempotent — a candidate already published under
/// the overlay's content hash short-circuits.
///
/// Threads the (inner, view) pair through a
/// [`crate::resolver_core::SessionResolverContext`] so the trait-routed
/// `ensure_indexed_ready` is the path of record for overlay-priority
/// materialisation (R18 — view passed explicitly via the
/// `ResolverContext` trait surface, not a thread-local).
pub(crate) fn prewarm_view_overlays(inner: &dyn ResolverContext, view: &dyn SessionView) {
    let session_ctx = crate::resolver_core::SessionResolverContext::new(inner, view);
    for canonical in view.overlay_canonicals() {
        let _ = ResolverContext::ensure_indexed_ready(&session_ctx, canonical.as_str());
    }
}
