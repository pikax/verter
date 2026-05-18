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
        let canonical = self
            .host
            .resolve_eval_dependency_canonical(canonical_id)
            .unwrap_or_else(|| canonical_id.to_string());

        // FileArtifactStore overlay fast path. When the session view carries
        // parse artifacts for this canonical (overlay candidate), prefer
        // them so the frontier reads overlay-rooted shallow state even in
        // the `route_exports_only` branch. Without this, route-only frontier
        // closures driven from a session-bearing path would fall through to
        // the base `route_shallow_state` and materialise the wrong target
        // when an overlay changes a barrel/re-export surface or tombstones
        // a dependency.
        if let Some(view) = self.view {
            if let Some(facts) = view.parse_artifacts(canonical.as_str()) {
                if facts.indexed.shallow_state.has_resolvable_surface() || !self.materialize_symbols
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
            return self.host.route_shallow_state(
                canonical.as_str(),
                &mut self.route_shallow_cache.borrow_mut(),
            );
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
            .current_content_pinned_indexed(canonical.as_str())
            .or_else(|| self.host.artifact_current_indexed(canonical.as_str()))
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
        // session view is active).
        let facts = if let Some(view) = self.view {
            crate::host_manage::overlay_priority::ensure_indexed_ready_with_view(
                self.host,
                view,
                canonical.as_str(),
            )?
        } else {
            self.host.ensure_indexed_ready(canonical.as_str())?
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
