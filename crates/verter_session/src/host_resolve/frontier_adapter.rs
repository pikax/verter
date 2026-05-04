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
use crate::VerterHost;

/// Adapter connecting the frontier engine to the real host.
///
/// Wraps a `VerterHost` reference with an optional `HostStoreView` for
/// snapshot-consistent resolution.
///
/// Consumed by component-meta resolution and frontier integration tests.
pub(crate) struct HostFrontierAdapter<'a> {
    pub host: &'a VerterHost,
    pub materialize_symbols: bool,
    pub route_exports_only: bool,
    /// Request-scoped memoisation of route-only [`ShallowFileState`] entries
    /// for the duration of a single frontier traversal. **NOT a host-side
    /// mirror** of the host's `route_owned_shallow` cache (the
    /// host cache lives on `ProjectTypeStore.route_owned_shallow`); this
    /// `RefCell<...>` exists only to dedupe repeated reads of the same
    /// canonical within one request, so request-level callers do not
    /// repeatedly clone the host-cached `Arc`. Lifetime bounded to the
    /// adapter (`'a`). classification: `scratch`. See sub-plan
    /// §6b.2.F9.
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

        if self.route_exports_only {
            return self.host.route_shallow_state(
                canonical.as_str(),
                &mut self.route_shallow_cache.borrow_mut(),
            );
        }

        // IndexedReadyDb fast path.
        if let Some(facts) = self
            .host
            .project_type_store
            .indexed()
            .get_any(canonical.as_str())
        {
            if facts.shallow_state.has_resolvable_surface() || !self.materialize_symbols {
                if facts.shallow_state.has_wildcard_reexports() {
                    self.host
                        .provenance
                        .resolver_barrel_fact_reuse
                        .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
                }
                return Some(facts.shallow_state.clone());
            }
        }

        // Materialize through ensure_indexed_ready.
        let facts = self.host.ensure_indexed_ready(canonical.as_str())?;
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
