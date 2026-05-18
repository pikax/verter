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
//! - `view.overlay_content_hash_for(canonical)` is `Some` (an explicit
//!   overlay covers the canonical) → publish an overlay-content
//!   [`IndexedReady`](crate::project_type_store::IndexedReady) candidate
//!   under an
//!   [`overlay_scoped`](crate::file_artifact_store::FileArtifactKey::overlay_scoped)
//!   key (overlay content hash + overlay-set discriminator), then
//!   return it. Base-host reads stay on the
//!   [`legacy`](crate::file_artifact_store::FileArtifactKey::legacy)
//!   key and never reach the candidate — even when the overlay bytes
//!   are identical to the base file.
//! - View has no overlay for the canonical → fall through to the
//!   host's own `ensure_indexed_ready` / `ensure_loaded` path.

use std::sync::Arc;

use crate::project_type_store::IndexedReady;
use crate::resolver_core::resolver_context::ResolverContext;
use crate::session_view::SessionView;
use crate::VerterHost;

/// Overlay-priority `ensure_loaded` helper.
///
/// When `view` tombstones the canonical, returns `false` without
/// consulting the host. When `view` carries an overlay for the
/// canonical, the overlay source is sufficient — the host's scheduler
/// does not need to load anything (returns `true`). Otherwise falls
/// through to the host via [`VerterHost::ensure_loaded`].
pub(crate) fn ensure_loaded_with_view(
    host: &VerterHost,
    view: &dyn SessionView,
    canonical_id: &str,
) -> bool {
    if view.is_tombstoned(canonical_id) {
        return false;
    }
    // Distinguish an explicit overlay-Upsert from a base-passthrough
    // source: the base-only views (`HostView`, `HostViewRef`) delegate
    // `source` to the base host, so `source(canonical).is_some()`
    // fires for every loaded canonical and would otherwise bypass the
    // host's `ensure_loaded` accounting (and the underlying
    // scheduler-load path).
    //
    // Overlay detection uses the **strict** `overlay_content_hash_for`,
    // NOT a `content_hash_for`-vs-base hash comparison. `content_hash_for`
    // falls through to the base host's `FileArtifactStore`-derived
    // content hash for an unmasked canonical — the same content-agnostic
    // scan as `get_any`, which can surface a STALE lingering artifact's
    // hash once the own-canonical drain is retired; a stale hash that
    // differs from the scheduler's current `base_hash` would misreport
    // an overlay for a canonical with NONE. `overlay_content_hash_for`
    // reports `Some` ONLY for an actual overlay-Upsert — for which the
    // overlay source is the content authority and the scheduler does
    // not need to load anything; an unmasked canonical correctly falls
    // through to `host.ensure_loaded`.
    if view.overlay_content_hash_for(canonical_id).is_some() {
        return true;
    }
    host.ensure_loaded(canonical_id)
}

/// Overlay-priority `ensure_indexed_ready` helper.
///
/// When `view` tombstones the canonical, returns `None`. When `view`
/// carries an **explicit overlay** for the canonical, materialises a
/// parallel IndexedReady from the overlay source and publishes it under
/// the overlay's content hash via
/// [`VerterHost::materialize_overlay_indexed_ready_with_view`].
/// Otherwise falls through to [`VerterHost::ensure_indexed_ready`].
pub(crate) fn ensure_indexed_ready_with_view(
    host: &VerterHost,
    view: &dyn SessionView,
    canonical_id: &str,
) -> Option<Arc<IndexedReady>> {
    if view.is_tombstoned(canonical_id) {
        return None;
    }
    // Overlay-priority: if the view carries an **explicit overlay**
    // candidate for the canonical, prefer it. The host's
    // `materialize_overlay_indexed_ready_with_view` entry point
    // publishes the candidate to the file-artifact store under the
    // overlay's content hash so future reads of the same overlay reuse
    // the cached candidate. Base-passthrough views (`HostView`,
    // `HostViewRef`) carry no overlay and fall through to the host's
    // standard `ensure_indexed_ready`.
    //
    // Overlay detection uses the **strict** `overlay_content_hash_for`,
    // NOT the permissive `content_hash_for`. `content_hash_for` falls
    // through to the base host's `FileArtifactStore`-derived content
    // hash for an unmasked canonical — the same content-agnostic,
    // canonical-only scan as `get_any`, which can surface a STALE
    // lingering artifact's hash once the own-canonical drain is
    // retired. Comparing that stale hash against the scheduler's
    // current `base_hash` would read `view_hash != base_hash` for a
    // canonical with NO overlay and re-route materialisation through
    // the overlay path keyed on the stale hash — resurrecting the
    // stale `IndexedReady`. `overlay_content_hash_for` reports `Some`
    // ONLY when the session installed an actual overlay-Upsert (the
    // overlay source's own hash), so an unmasked canonical correctly
    // falls through to `ensure_indexed_ready`.
    if let Some(overlay_hash) = view.overlay_content_hash_for(canonical_id) {
        if let Some(overlay_source) = view.source(canonical_id) {
            if let Some(indexed) = host.materialize_overlay_indexed_ready_with_view(
                canonical_id,
                &overlay_source,
                overlay_hash,
                view,
            ) {
                return Some(indexed);
            }
        }
    }
    host.ensure_indexed_ready(canonical_id)
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
/// Threads the (host, view) pair through a
/// [`crate::resolver_core::SessionResolverContext`] so the trait-routed
/// `ensure_indexed_ready` is the path of record for overlay-priority
/// materialisation (R18 — view passed explicitly via the
/// `ResolverContext` trait surface, not a thread-local).
pub(crate) fn prewarm_view_overlays(host: &VerterHost, view: &dyn SessionView) {
    let session_ctx = crate::resolver_core::SessionResolverContext::new(host, view);
    for canonical in view.overlay_canonicals() {
        let _ = ResolverContext::ensure_indexed_ready(&session_ctx, canonical.as_str());
    }
}
