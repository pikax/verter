//! `HostFrontierAdapter` — the bridge that lets
//! `crate::resolver_core::ExternalTypeFrontier` resolve through the real
//! `VerterHost` (or a route-only shadow projection of it).
//!
//! The adapter is request-scoped: it holds a `&VerterHost`, the two
//! traversal-mode flags (`materialize_symbols`, `route_exports_only`),
//! and a `RefCell<RouteShallowStateCache>` for de-duping repeated
//! shallow-state reads inside a single frontier traversal. The cache
//! is **not** a host-side mirror of `ProjectTypeStore.route_owned_shallow`
//! — see the field doc-comment.

use std::cell::RefCell;
use std::sync::Arc;

use super::frontier_helpers::RouteShallowStateCache;
use crate::session_view::SessionView;
use crate::VerterHost;

/// Adapter connecting the frontier engine to the real host.
///
/// Wraps a `VerterHost` reference with an optional `HostStoreView` for
/// snapshot-consistent resolution.
///
/// `view` carries the active session overlay when the frontier is driven
/// from a session-bearing cold-compute path; base-only callers leave it
/// `None` and the adapter behaves as before.
///
/// Consumed by component-meta resolution and frontier integration tests.
pub(crate) struct HostFrontierAdapter<'a> {
    pub host: &'a VerterHost,
    pub materialize_symbols: bool,
    pub route_exports_only: bool,
    /// Active session overlay (when the frontier is driven from a
    /// session-bearing path). `None` for base-only callers — the adapter
    /// then reads through the bare host as before.
    pub view: Option<&'a dyn SessionView>,
    /// Request-bound resolver context plumbed from the cold-compute
    /// entry. Carrier sites under `planned_frontier_companions` route
    /// through `ctx.resolve_imported_type_root` so the import-root
    /// resolution observes the request-bound overlay-aware view
    /// rather than rebuild a workspace snapshot per call.
    pub ctx: &'a dyn crate::resolver_core::resolver_context::ResolverContext,
    /// Request-scoped memoisation of route-only [`ShallowFileState`] entries
    /// for the duration of a single frontier traversal. **NOT a host-side
    /// mirror** of the host's `route_owned_shallow` cache (the
    /// host cache lives on `ProjectTypeStore.route_owned_shallow`); this
    /// `RefCell<...>` exists only to dedupe repeated reads of the same
    /// canonical within one request, so request-level callers do not
    /// repeatedly clone the host-cached `Arc`. Lifetime bounded to the
    /// adapter (`'a`).
    pub route_shallow_cache: RefCell<RouteShallowStateCache>,
}

impl crate::resolver_core::FrontierHost for HostFrontierAdapter<'_> {
    fn ensure_shallow_state(
        &self,
        canonical_id: &str,
    ) -> Option<Arc<crate::resolver_core::ShallowFileState>> {
        // Two-identity split. `canonical_id` is the RAW requested
        // canonical; `identity` pairs it with `analysis_canonical` (the
        // `normalized_analysis_canonical` rewrite). The overlay branches
        // below (`lookup_overlay_artifacts`,
        // `ensure_indexed_ready_with_view`) operate on the RAW owner —
        // the `SessionView` overlay maps + the overlay-detection gate
        // are raw-keyed, and normalising first would (a) miss the
        // overlay-scoped artifact key and (b) make the overlay-detection
        // gate fail. The base reads (`route_shallow_state`,
        // `current_content_pinned_indexed`, `artifact_current_indexed`)
        // key on the normalised analysis canonical.
        let identity = self.host.overlay_artifact_identity(canonical_id);
        let canonical = identity.analysis_canonical();

        // FileArtifactStore overlay fast path. When the session view has
        // a published overlay artifact for the raw owner, prefer it so
        // the frontier reads overlay-rooted shallow state even in the
        // `route_exports_only` branch. Without this, route-only frontier
        // closures driven from a session-bearing path would fall through
        // to the base `route_shallow_state` and materialise the wrong
        // target when an overlay changes a barrel/re-export surface or
        // tombstones a dependency. `lookup_overlay_artifacts` rebuilds
        // the exact `overlay_scoped` key the materialiser published
        // under, so it reaches the candidate even when
        // `normalize(raw) != raw`.
        if let Some(view) = self.view {
            if view.overlay_content_hash_for(canonical_id).is_some() {
                // GENUINELY OVERLAID canonical: route through the gated overlay
                // materialiser accessor (not a direct artifact read). It
                // re-resolves wildcard `export *` edges against the live file
                // set when the cached overlay surface is edge-stale, and
                // materialises from the overlay source — never the base surface
                // (no overlay-blindness).
                if let Some(indexed) = self
                    .host
                    .materialize_overlay_indexed_ready_with_view(canonical_id, view)
                {
                    if indexed.shallow_state.has_resolvable_surface() || !self.materialize_symbols {
                        if indexed.shallow_state.has_wildcard_reexports() {
                            self.host
                                .provenance
                                .resolver_barrel_fact_reuse
                                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                        }
                        return Some(indexed.shallow_state.clone());
                    }
                }
            } else if let Some(facts) = identity.lookup_overlay_artifacts(self.host, view) {
                // Base-passthrough view (the canonical is not overlaid): the
                // legacy-key read returns the published BASE artifact. Serve it
                // only while edge-current; an edge-stale wildcard `export *`
                // surface falls through to the gated base reads below
                // (`route_shallow_state` / `current_content_pinned_indexed` /
                // `ensure_indexed_ready`), which re-resolve the edges against
                // the live file set. Routing it through the overlay materialiser
                // would instead build a redundant overlay candidate from base
                // content.
                if (facts.indexed.shallow_state.has_resolvable_surface()
                    || !self.materialize_symbols)
                    && self.host.route_surface_is_edge_current(
                        &facts.indexed.shallow_state,
                        facts.indexed.edge_generation,
                    )
                {
                    if facts.indexed.shallow_state.has_wildcard_reexports() {
                        self.host
                            .provenance
                            .resolver_barrel_fact_reuse
                            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                    }
                    return Some(facts.indexed.shallow_state.clone());
                }
            }
        }

        if self.route_exports_only {
            return self
                .host
                .route_shallow_state(canonical, &mut self.route_shallow_cache.borrow_mut());
        }
        // `IndexedReady` fast path — **current-content-pinned** (no
        // `get_any`). The frontier resolves a dependency's symbol surface
        // from this `ShallowFileState`; with the own-canonical drain
        // retired a stale pre-edit `IndexedReady` can linger past a
        // same-canonical edit, and a `get_any` read would resolve the
        // stale symbol surface (e.g. a pre-rename type body). The pinned
        // read serves only a content-current artifact for a
        // scheduler-tracked canonical; on a miss the materialising path
        // below (`ensure_indexed_ready`, overlay-aware) rebuilds at the
        // current content. `artifact_current_indexed` covers a genuinely
        // artifact-only canonical (foreign source / test seed).
        if let Some(indexed) = self
            .host
            .current_content_pinned_indexed(canonical)
            .or_else(|| self.host.artifact_current_indexed(canonical))
        {
            if indexed.shallow_state.has_resolvable_surface() || !self.materialize_symbols {
                if indexed.shallow_state.has_wildcard_reexports() {
                    self.host
                        .provenance
                        .resolver_barrel_fact_reuse
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                return Some(indexed.shallow_state.clone());
            }
        }

        // Materialize through ensure_indexed_ready (view-aware when a
        // session view is active). The view-aware path is driven on the
        // RAW `canonical_id` — its overlay-detection gate
        // (`overlay_content_hash_for`) is raw-keyed, so a normalised id
        // would fail to detect the overlay; `ensure_indexed_ready`
        // normalises internally on the base-path fall-through. The base
        // (no-view) path takes the normalised analysis canonical
        // directly.
        let facts = if let Some(view) = self.view {
            crate::host_manage::overlay_priority::ensure_indexed_ready_with_view(
                self.host,
                view,
                canonical_id,
            )?
        } else {
            self.host.ensure_indexed_ready(canonical)?
        };
        if facts.shallow_state.has_resolvable_surface() || !self.materialize_symbols {
            if facts.shallow_state.has_wildcard_reexports() {
                self.host
                    .provenance
                    .resolver_barrel_fact_reuse
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            return Some(facts.shallow_state.clone());
        }

        None
    }
    fn route_exports_only(&self) -> bool {
        self.route_exports_only
    }

    fn resolve_type_edge_canonical(
        &self,
        owner_canonical: &str,
        source_specifier: &str,
    ) -> Option<String> {
        self.host
            .resolve_type_dependency_canonical(owner_canonical, source_specifier)
    }
}
