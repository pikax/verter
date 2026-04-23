//! Session-owned runtime for overlay-sensitive request execution.
//!
//! `SessionRuntime` is the reusable owner for a session's overlay-aware
//! query surface. It owns the session's identity, overlay context
//! lifecycle, session-scoped resolved-meta cache, and `SessionView`
//! construction.
//!
//! `SessionView` is the session's immutable snapshot of host state,
//! used for store-view construction with session-scoped compat tokens.
//!
//! # Overlay Lifecycle (C14 + C15)
//!
//! Overlay context-switching applies overlays to the shared host via
//! the host's upsert/remove pipeline. The runtime owns this lifecycle.
//! The active overlay slot uses atomic compare_exchange for race-free
//! session switching; `ArcSwap<SessionView>` snapshots provide
//! lock-free reads, with writers serialized via `view_writer_lock`.

use std::sync::Arc;

use arc_swap::ArcSwap;
use parking_lot::RwLock;
use rustc_hash::FxHashMap;

use crate::meta::{MetaProject, SessionOverlay};
use crate::meta_resolve::ResolvedComponentMetaState;
use crate::types::{FileKind, ResolverMode, UpsertRequest};

/// Session identity — unique per session within a `MetaProject`.
pub type SessionId = u64;

/// Immutable snapshot of the session's effective host state.
///
/// Used by `HostStoreView::from_session` to build session-scoped store
/// views with session identity in the compat token. For C14 this is a
/// lightweight identity carrier; C15 expands it to a full
/// ArcSwap-backed snapshot.
#[derive(Debug, Clone)]
pub(crate) struct SessionView {
    pub session_id: SessionId,
    #[allow(dead_code)]
    pub mutation_epoch: u64,
}

/// Reusable runtime owner for a session's overlay-aware execution.
///
/// Owns: session identity, overlay context lifecycle, session-scoped
/// resolved-meta cache, and `SessionView` construction.
pub(crate) struct SessionRuntime {
    session_id: SessionId,
    project: Arc<MetaProject>,

    /// Session-scoped resolved component-meta cache. Writes from the
    /// stable-request executor go here instead of the host's shared
    /// caches, preventing overlay-derived state from leaking across
    /// concurrent sessions.
    resolved_meta_cache: RwLock<FxHashMap<(String, ResolverMode), Arc<ResolvedComponentMetaState>>>,

    /// Current SessionView as an atomically-swappable snapshot (C15).
    /// Readers: `current_view()` via `ArcSwap::load` — lock-free.
    /// Writers: serialized via `view_writer_lock` below.
    view_snapshot: ArcSwap<SessionView>,

    /// Serializes concurrent SessionView publications for THIS session.
    /// Inter-session: zero contention (each session has its own lock).
    view_writer_lock: parking_lot::Mutex<()>,
}

impl SessionRuntime {
    pub fn new(session_id: SessionId, project: Arc<MetaProject>) -> Self {
        let epoch = project.host().current_store_view_epoch();
        Self {
            session_id,
            project,
            resolved_meta_cache: RwLock::new(FxHashMap::default()),
            view_snapshot: ArcSwap::from_pointee(SessionView {
                session_id,
                mutation_epoch: epoch,
            }),
            view_writer_lock: parking_lot::Mutex::new(()),
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

    /// Return the current session view snapshot (lock-free read via ArcSwap).
    pub fn current_view(&self) -> SessionView {
        let guard = self.view_snapshot.load();
        (**guard).clone()
    }

    /// Return the current store-view epoch from this session's perspective.
    pub fn current_store_view_epoch(&self) -> u64 {
        self.host().current_store_view_epoch()
    }

    // -------------------------------------------------------------------
    // Overlay context lifecycle (replaces MetaProject::apply_session_overlays
    // and MetaProject::revert_session_overlays)
    // -------------------------------------------------------------------

    /// Apply this session's overlays to the shared host.
    /// Called from `with_overlay_target_context` after atomic CAS claim.
    pub(crate) fn apply_own_overlays(&self) {
        let sessions = self.project.sessions_read();
        let Some(state) = sessions.get(&self.session_id) else {
            return;
        };
        for (file_id, overlay) in &state.overlays {
            match overlay {
                SessionOverlay::Upsert { source } => {
                    let req = UpsertRequest {
                        canonical_id: Some(file_id.clone()),
                        input_id: file_id.clone(),
                        source: Arc::from(source.as_str()),
                        file_kind: FileKind::from_path(file_id),
                        aliases: Vec::new(),
                    };
                    let _ = self.host().upsert(req);
                }
                SessionOverlay::Delete => {
                    self.host().remove(file_id);
                }
            }
        }
    }

    /// Revert another session's overlays from the shared host, restoring
    /// base state. Called from `with_overlay_target_context` after atomic
    /// CAS claim.
    pub(crate) fn revert_other_session_overlays(&self, prev_session_id: SessionId) {
        let sessions = self.project.sessions_read();
        let Some(state) = sessions.get(&prev_session_id) else {
            return;
        };
        let base = self.project.base_sources_read();
        for (file_id, overlay) in &state.overlays {
            match overlay {
                SessionOverlay::Upsert { .. } => {
                    if let Some(base_source) = base.get(file_id) {
                        let req = UpsertRequest {
                            canonical_id: Some(file_id.clone()),
                            input_id: file_id.clone(),
                            source: Arc::clone(base_source),
                            file_kind: FileKind::from_path(file_id),
                            aliases: Vec::new(),
                        };
                        let _ = self.host().upsert(req);
                    } else {
                        self.host().remove(file_id);
                    }
                }
                SessionOverlay::Delete => {
                    if let Some(base_source) = base.get(file_id) {
                        let req = UpsertRequest {
                            canonical_id: Some(file_id.clone()),
                            input_id: file_id.clone(),
                            source: Arc::clone(base_source),
                            file_kind: FileKind::from_path(file_id),
                            aliases: Vec::new(),
                        };
                        let _ = self.host().upsert(req);
                    }
                }
            }
        }
    }

    /// Reapply the overlay for a specific file (used after context
    /// switch to realign cached import/dependency state).
    pub(crate) fn reapply_overlay_target(&self, canonical_id: &str) {
        let sessions = self.project.sessions_read();
        let Some(state) = sessions.get(&self.session_id) else {
            return;
        };
        let Some(SessionOverlay::Upsert { source }) = state.overlays.get(canonical_id) else {
            return;
        };
        let req = UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::from(source.as_str()),
            file_kind: FileKind::from_path(canonical_id),
            aliases: Vec::new(),
        };
        let _ = self.host().upsert(req);
    }

    /// Publish a new session view snapshot from the current host state.
    /// Serialized via the per-session `view_writer_lock`; readers are
    /// unblocked (lock-free ArcSwap load).
    pub(crate) fn refresh_view(&self) {
        let _guard = self.view_writer_lock.lock();
        let epoch = self.host().current_store_view_epoch();
        self.view_snapshot.store(Arc::new(SessionView {
            session_id: self.session_id,
            mutation_epoch: epoch,
        }));
    }

    // -------------------------------------------------------------------
    // Session-scoped resolved-meta cache
    // -------------------------------------------------------------------

    /// Try to get a cached resolved-meta from the session-scoped cache
    /// (overlay-aware sessions only), falling back to the host's shared
    /// cache which properly validates via fact versions.
    pub fn try_get_cached_resolved_meta(
        &self,
        canonical: &str,
        mode: ResolverMode,
    ) -> Option<ResolvedComponentMetaState> {
        // Session-scoped cache is only authoritative when the session has
        // overlays. Overlay-free sessions rely on the host's fact-validated
        // cache to correctly reflect base-state changes.
        if self.project.session_has_overlays(self.session_id) {
            let session_cache = self.resolved_meta_cache.read();
            if let Some(cached) = session_cache.get(&(canonical.to_string(), mode)) {
                return Some(cached.as_ref().clone());
            }
        }

        self.host().try_get_cached_resolved_meta(canonical, mode)
    }

    /// Store resolved-meta in the session-scoped cache when overlays are
    /// active. Overlay-free results go to the host's shared cache
    /// (via the VerterHost impl's `store_component_meta_result`).
    pub fn store_resolved_meta(
        &self,
        canonical: &str,
        mode: ResolverMode,
        result: &ResolvedComponentMetaState,
    ) {
        if self.project.session_has_overlays(self.session_id) {
            let mut cache = self.resolved_meta_cache.write();
            cache.insert((canonical.to_string(), mode), Arc::new(result.clone()));
        } else {
            // No overlays — safe to publish to host cache which validates
            // via fact versions and is accessible to all sessions.
            self.host()
                .store_cached_resolved_meta(canonical, mode, result, &result.fact_versions);
        }
    }

    /// Invalidate session-scoped caches (e.g., when overlays change).
    pub fn invalidate_session_caches(&self) {
        self.resolved_meta_cache.write().clear();
    }

    // -------------------------------------------------------------------
    // Session-scoped component-meta resolution (replaces VerterHost paths
    // for overlay-aware callers per plan C14 item 9)
    // -------------------------------------------------------------------

    /// Session-scoped component-meta resolution. Creates a
    /// `SessionRequestHost` and routes through `run_component_meta_request`
    /// so all reads/writes are session-scoped.
    pub fn resolve_component_meta(
        &self,
        canonical_or_alias: &str,
        mode: ResolverMode,
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

        let resolved = self.resolve_component_meta(canonical.as_str(), ResolverMode::Expanded)?;
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
        let resolved = self.resolve_component_meta(canonical.as_str(), ResolverMode::Expanded)?;
        let (analysis, fallthrough_fact_versions) =
            crate::host_manage::extract_component_meta_from_resolved_with_facts(
                host,
                canonical.as_str(),
                &resolved,
            );
        Some((analysis, resolved, fallthrough_fact_versions))
    }
}
