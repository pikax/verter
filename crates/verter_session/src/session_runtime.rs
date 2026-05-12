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

/// Session identity — unique per session within a `MetaProject`.
pub type SessionId = u64;

/// Reusable runtime owner for a session's query execution.
///
/// Owns: session identity and a back-reference to the project. All
/// query state lives on the host's shared caches; the runtime is a
/// thin facade that surfaces the session id to singleflight lane
/// identity (`StoreViewCompatToken`) and forwards reads through
/// the host's existing accessors.
pub(crate) struct SessionRuntime {
    session_id: SessionId,
    project: Arc<MetaProject>,
}

impl SessionRuntime {
    pub fn new(session_id: SessionId, project: Arc<MetaProject>) -> Self {
        Self {
            session_id,
            project,
        }
    }

    pub fn session_id(&self) -> SessionId {
        self.session_id
    }

    pub fn host(&self) -> &crate::VerterHost {
        self.project.host()
    }

    #[allow(dead_code)]
    pub fn project(&self) -> &Arc<MetaProject> {
        &self.project
    }

    /// Return the current store-view epoch from this session's perspective.
    pub fn current_store_view_epoch(&self) -> u64 {
        self.host().current_store_view_epoch()
    }

    // -------------------------------------------------------------------
    // Resolved-meta cache facade
    // -------------------------------------------------------------------

    /// Try to get a cached resolved-meta from the host's shared cache.
    ///
    /// There is no session-scoped `resolved_meta_cache` (R20). The
    /// host's shared cache is fact-validated (view-aware
    /// `HostFenceValidator`) so overlay-derived state cannot leak
    /// across concurrent sessions when the multi-candidate substrate
    /// is consulted. Where the multi-candidate substrate is not yet
    /// in the read path, overlay sessions transparently read
    /// base-state results.
    pub fn try_get_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ProjectionMode,
    ) -> Option<ResolvedComponentMetaState> {
        self.host().try_get_cached_resolved_meta(canonical, mode)
    }

    /// Store resolved-meta in the host's shared cache.
    ///
    /// Writes go directly to the host cache (which validates via
    /// `HostFenceValidator`). Multi-candidate admission isolates
    /// concurrent overlay variants in the same query-identity slot.
    pub fn store_resolved_meta(
        &self,
        canonical: &str,
        mode: ProjectionMode,
        result: &ResolvedComponentMetaState,
    ) {
        self.host()
            .store_cached_resolved_meta(canonical, mode, result, &result.fact_versions);
    }

    /// Invalidate session-scoped caches — no-op.
    ///
    /// There is no session-scoped cache (R17 — overlay mutation is
    /// not allowed); all reads go through the host's shared cache
    /// directly. Retained as a method so the public
    /// `MetaSession::invalidate_active_overlays` API surface stays
    /// stable for callers that may still invoke it from FFI paths.
    pub fn invalidate_session_caches(&self) {}

    // -------------------------------------------------------------------
    // Session-scoped component-meta resolution
    // -------------------------------------------------------------------

    /// Session-scoped component-meta resolution. Creates a
    /// `SessionRequestHost` and routes through `run_component_meta_request`
    /// so all reads/writes go through the session's runtime facade
    /// (which itself delegates to the host's shared cache).
    pub fn resolve_component_meta(
        &self,
        canonical_or_alias: &str,
        mode: ProjectionMode,
    ) -> Option<ResolvedComponentMetaState> {
        let host = self.host();
        let canonical = host.resolve_alias_or_canonical(canonical_or_alias);

        let request_host = crate::meta_resolve::SessionRequestHost { runtime: self };
        let result = crate::resolver_core::run_component_meta_request(
            &request_host,
            host.resolver_runtime().component_meta.singleflight(),
            &canonical,
            mode,
            None,
            crate::meta_resolve::STORE_VIEW_STABILITY_MAX_ATTEMPTS,
        );

        // Mirror provenance counters to the host for diagnostic continuity.
        use crate::resolver_core::{RequestSource, SingleflightRole};
        if matches!(result.source, RequestSource::Cache) {
            host.provenance()
                .resolver_node_cache_hits
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if !(matches!(result.source, RequestSource::Cache) && result.attempts == 1) {
            host.provenance()
                .resolver_node_cache_misses
                .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        }
        if let RequestSource::Flight { role, forked_lane } = result.source {
            if role == SingleflightRole::Follower {
                host.provenance()
                    .resolver_singleflight_coalesced
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
            if forked_lane {
                host.provenance()
                    .resolver_cross_view_lane_forks
                    .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            }
        }

        result.value
    }

    /// Session-scoped combined component-meta + resolution query.
    /// Routes through `resolve_component_meta` (session-scoped) instead
    /// of `VerterHost::get_component_meta_with_resolution`.
    pub fn get_component_meta_with_resolution(
        &self,
        canonical_or_alias: &str,
    ) -> Option<(
        verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        ResolvedComponentMetaState,
    )> {
        let host = self.host();
        host.provenance()
            .get_component_meta_calls
            .fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        let canonical = host.resolve_alias_or_canonical(canonical_or_alias);

        let resolved = self.resolve_component_meta(canonical.as_str(), ProjectionMode::Expanded)?;
        let analysis = crate::host_manage::extract_component_meta_from_resolved(
            host,
            canonical.as_str(),
            &resolved,
            true,
        );
        Some((analysis, resolved))
    }

    /// Session-scoped component-meta with fallthrough fact versions for
    /// payload cache storage.
    #[allow(dead_code)]
    pub fn get_component_meta_with_resolution_and_facts(
        &self,
        canonical_or_alias: &str,
    ) -> Option<(
        verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
        ResolvedComponentMetaState,
        Option<Vec<crate::resolver_core::FactVersionRef>>,
    )> {
        let host = self.host();
        let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
        let resolved = self.resolve_component_meta(canonical.as_str(), ProjectionMode::Expanded)?;
        let (analysis, fallthrough_fact_versions) =
            crate::host_manage::extract_component_meta_from_resolved_with_facts(
                host,
                canonical.as_str(),
                &resolved,
            );
        Some((analysis, resolved, fallthrough_fact_versions))
    }
}
