//! Pooled runtime with isolated sessions for component-meta.
//!
//! `MetaProject` wraps a single [`VerterHost`] and manages shared base state.
//! `MetaSession` provides isolated overlay views on top of the shared base.
//!
//! # Overlay Isolation Model
//!
//! Session overlays are private to the session. `upsert()` and `delete()` in
//! one session never affect another session's view. Queries are resolved
//! against `session overlay → shared base` via context-switching: before a
//! query, the project applies the requesting session's overlays to the shared
//! host and reverts any previously-applied session's overlays.
//!
//! # Concurrency
//!
//! A `Mutex<OverlayState>` serializes overlay-aware queries. Overlay-free
//! sessions query the shared host directly without acquiring the gate.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use crate::types::{FileKind, UpsertRequest};
use crate::VerterHost;

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from MetaProject / MetaSession operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum MetaError {
    #[error("project has been shut down")]
    Shutdown,
    #[error("session is closed")]
    SessionClosed,
    #[error("host error: {0}")]
    Host(String),
}

fn component_meta_expansion_budget_exceeded(
    types: &verter_semantic::analysis::type_expand::ExpandedComponentTypes,
) -> bool {
    use verter_semantic::analysis::type_expand::ExpansionStopReason;

    let field_has_budget = |field: &verter_semantic::analysis::type_expand::ExpandedField| {
        field
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.reason == ExpansionStopReason::BudgetExceeded)
    };
    let macro_has_budget =
        |shape: &verter_semantic::analysis::type_expand::ExpandedMacroObjectShape| {
            shape
                .result
                .diagnostics
                .iter()
                .any(|diagnostic| diagnostic.reason == ExpansionStopReason::BudgetExceeded)
        };
    let props_has_budget = |shape: &verter_semantic::analysis::type_expand::ExpandedMacroProps| {
        shape
            .result
            .diagnostics
            .iter()
            .any(|diagnostic| diagnostic.reason == ExpansionStopReason::BudgetExceeded)
    };

    types.props.iter().any(field_has_budget)
        || types.emits.iter().any(field_has_budget)
        || types.slot_bindings.iter().any(field_has_budget)
        || types.bindings.iter().any(field_has_budget)
        || types.define_props.iter().any(props_has_budget)
        || types.define_emits.iter().any(macro_has_budget)
        || types.define_slots.iter().any(macro_has_budget)
}

fn component_meta_symbolic_budget_is_fatal(
    analysis: Option<&verter_semantic::analysis::component_meta::ComponentMetaAnalysis>,
) -> bool {
    let Some(analysis) = analysis else {
        return true;
    };

    analysis.props.is_empty()
        && analysis.events.is_empty()
        && analysis.slots.is_empty()
        && analysis.models.is_empty()
        && analysis.exposed.is_empty()
}

fn component_meta_resolution_budget_error(
    canonical_or_alias: &str,
    analysis: Option<&verter_semantic::analysis::component_meta::ComponentMetaAnalysis>,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
) -> Option<MetaError> {
    // Walker overflow is no longer meaningful — the solver path replaced
    // the legacy imported-eval walker. Only check symbolic expansion budget.
    if resolved
        .evaluated_types
        .as_ref()
        .is_some_and(component_meta_expansion_budget_exceeded)
        && component_meta_symbolic_budget_is_fatal(analysis)
    {
        return Some(MetaError::Host(format!(
            "component-meta symbolic expansion budget exceeded (maxSteps={}) for '{}'",
            crate::host_manage::component_meta_symbolic_step_budget(),
            canonical_or_alias,
        )));
    }

    None
}

// ---------------------------------------------------------------------------
// SessionOverlay
// ---------------------------------------------------------------------------

/// A session-local file mutation.
#[derive(Debug, Clone)]
pub enum SessionOverlay {
    /// File content was replaced by the session.
    Upsert { source: String },
    /// File was deleted (tombstoned) by the session.
    Delete,
}

// ---------------------------------------------------------------------------
// OverlayState — tracks which session's overlays are currently applied
// ---------------------------------------------------------------------------

struct OverlayState {
    /// Which session's overlays are currently applied in the host, if any.
    active_session: Option<u64>,
}

// ---------------------------------------------------------------------------
// SessionState — per-session overlay tracking
// ---------------------------------------------------------------------------

struct SessionState {
    overlays: HashMap<String, SessionOverlay>,
    generation: u64,
}

// ---------------------------------------------------------------------------
// MetaProject
// ---------------------------------------------------------------------------

/// Shared project state wrapping one [`VerterHost`].
///
/// Multiple [`MetaSession`]s can be opened against the same project.
/// The project owns the host, base file cache, and overlay context gate.
pub struct MetaProject {
    host: VerterHost,
    /// Cached base sources for overlay revert. Key = canonical ID.
    base_sources: parking_lot::RwLock<HashMap<String, Arc<str>>>,
    /// Set of canonical IDs in the base file index.
    base_file_ids: parking_lot::RwLock<std::collections::HashSet<String>>,
    /// Serializes overlay-aware queries (context-switching gate).
    overlay_gate: Mutex<OverlayState>,
    /// Per-session state, keyed by session ID.
    sessions: parking_lot::RwLock<HashMap<u64, SessionState>>,
    /// Monotonic session ID counter.
    next_session_id: AtomicU64,
    /// Terminal shutdown flag.
    shutdown: AtomicBool,
}

impl MetaProject {
    /// Create a new project wrapping the given host.
    pub fn new(host: VerterHost) -> Arc<Self> {
        Arc::new(Self {
            host,
            base_sources: parking_lot::RwLock::new(HashMap::new()),
            base_file_ids: parking_lot::RwLock::new(std::collections::HashSet::new()),
            overlay_gate: Mutex::new(OverlayState {
                active_session: None,
            }),
            sessions: parking_lot::RwLock::new(HashMap::new()),
            next_session_id: AtomicU64::new(1),
            shutdown: AtomicBool::new(false),
        })
    }

    /// Check the shutdown flag and return an error if set.
    fn check_alive(&self) -> Result<(), MetaError> {
        if self.shutdown.load(Ordering::Acquire) {
            Err(MetaError::Shutdown)
        } else {
            Ok(())
        }
    }

    fn remove_base_file(&self, canonical_id: &str) {
        self.base_sources.write().remove(canonical_id);
        self.base_file_ids.write().remove(canonical_id);
    }

    fn sync_base_file_from_host(&self, canonical_id: &str) -> bool {
        let Some(source) = self.host.get_source(canonical_id) else {
            self.remove_base_file(canonical_id);
            return false;
        };
        self.base_sources
            .write()
            .insert(canonical_id.to_string(), source);
        self.base_file_ids.write().insert(canonical_id.to_string());
        true
    }

    fn enter_base_context(&self) -> Result<std::sync::MutexGuard<'_, OverlayState>, MetaError> {
        let mut gate = self
            .overlay_gate
            .lock()
            .map_err(|_| MetaError::Host("overlay lock poisoned".into()))?;
        if let Some(prev_id) = gate.active_session.take() {
            self.revert_session_overlays(prev_id);
        }
        Ok(gate)
    }

    /// Load a file into the base project. This is the shared state that
    /// all sessions see when they don't have an overlay for the file.
    pub fn upsert_base(
        self: &Arc<Self>,
        canonical_id: &str,
        source: &str,
    ) -> Result<(), MetaError> {
        self.check_alive()?;
        let _gate = self.enter_base_context()?;
        let req = UpsertRequest {
            canonical_id: Some(canonical_id.to_string()),
            input_id: canonical_id.to_string(),
            source: Arc::from(source),
            file_kind: FileKind::from_path(canonical_id),
            aliases: Vec::new(),
        };
        let _ = self
            .host
            .upsert(req)
            .map_err(|e| MetaError::Host(e.to_string()))?;

        self.base_sources
            .write()
            .insert(canonical_id.to_string(), Arc::from(source));
        self.base_file_ids.write().insert(canonical_id.to_string());
        Ok(())
    }

    /// Ensure a workspace-backed file is loaded into the shared base project.
    pub fn ensure_loaded(self: &Arc<Self>, canonical_id: &str) -> Result<bool, MetaError> {
        self.check_alive()?;
        let _gate = self.enter_base_context()?;

        #[cfg(not(target_arch = "wasm32"))]
        let loaded = self.host.ensure_loaded(canonical_id);

        #[cfg(target_arch = "wasm32")]
        let loaded = self.host.get_source(canonical_id).is_some();

        if !loaded {
            self.remove_base_file(canonical_id);
            return Ok(false);
        }

        Ok(self.sync_base_file_from_host(canonical_id))
    }

    /// Refresh a workspace-backed base file from the current native workspace.
    pub fn refresh_base(self: &Arc<Self>, canonical_id: &str) -> Result<bool, MetaError> {
        self.check_alive()?;
        let _gate = self.enter_base_context()?;

        #[cfg(not(target_arch = "wasm32"))]
        self.host.evict(canonical_id);

        #[cfg(not(target_arch = "wasm32"))]
        let loaded = self.host.ensure_loaded(canonical_id);

        #[cfg(target_arch = "wasm32")]
        let loaded = self.host.get_source(canonical_id).is_some();

        if !loaded {
            self.remove_base_file(canonical_id);
            return Ok(false);
        }

        Ok(self.sync_base_file_from_host(canonical_id))
    }

    /// Configure project-scoped path alias resolution.
    pub fn configure_projects(
        &self,
        projects: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig>,
    ) -> Result<(), MetaError> {
        self.check_alive()?;
        let _gate = self.enter_base_context()?;
        self.host.configure_projects(projects);
        Ok(())
    }

    /// Open a new session against this project.
    pub fn open_session(self: &Arc<Self>) -> Result<MetaSession, MetaError> {
        self.check_alive()?;
        let id = self.next_session_id.fetch_add(1, Ordering::Relaxed);
        self.sessions.write().insert(
            id,
            SessionState {
                overlays: HashMap::new(),
                generation: 0,
            },
        );
        Ok(MetaSession {
            id,
            project: Arc::clone(self),
            closed: AtomicBool::new(false),
        })
    }

    /// Clear shared analysis caches without shutting down.
    /// Active sessions keep their overlays; only base caches are flushed.
    pub fn clear_caches(&self) -> Result<(), MetaError> {
        self.check_alive()?;
        // Clear the overlay gate so stale applied state doesn't persist
        let mut gate = self
            .overlay_gate
            .lock()
            .map_err(|_| MetaError::Host("overlay lock poisoned".into()))?;
        if let Some(prev_id) = gate.active_session.take() {
            self.revert_session_overlays(prev_id);
        }
        // Clear host compile caches
        self.host.clear_compile_cache();
        Ok(())
    }

    /// Terminal shutdown. Stops the host and marks the project as dead.
    /// All sessions become invalid after this call.
    pub fn shutdown(&self) {
        if self
            .shutdown
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return; // Already shut down
        }
        // Revert any applied overlays before closing
        if let Ok(mut gate) = self.overlay_gate.lock() {
            if let Some(prev_id) = gate.active_session.take() {
                self.revert_session_overlays(prev_id);
            }
        }
        self.host.close();
        self.base_sources.write().clear();
        self.base_file_ids.write().clear();
        self.sessions.write().clear();
    }

    /// Returns whether this project has been shut down.
    pub fn is_shutdown(&self) -> bool {
        self.shutdown.load(Ordering::Acquire)
    }

    /// Get a reference to the underlying host (for advanced use).
    pub fn host(&self) -> &VerterHost {
        &self.host
    }

    /// Returns the set of canonical IDs in the base file index.
    pub fn base_file_ids(&self) -> std::collections::HashSet<String> {
        self.base_file_ids.read().clone()
    }

    /// Number of active (non-closed) sessions.
    pub fn session_count(&self) -> usize {
        self.sessions.read().len()
    }

    // -----------------------------------------------------------------------
    // Overlay context-switching internals
    // -----------------------------------------------------------------------

    /// Apply a session's overlays to the shared host.
    fn apply_session_overlays(&self, session_id: u64) {
        let sessions = self.sessions.read();
        let Some(state) = sessions.get(&session_id) else {
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
                    let _ = self.host.upsert(req);
                }
                SessionOverlay::Delete => {
                    self.host.remove(file_id);
                }
            }
        }
    }

    /// Revert a session's overlays from the shared host, restoring base state.
    fn revert_session_overlays(&self, session_id: u64) {
        let sessions = self.sessions.read();
        let Some(state) = sessions.get(&session_id) else {
            return;
        };
        let base = self.base_sources.read();
        for (file_id, overlay) in &state.overlays {
            match overlay {
                SessionOverlay::Upsert { .. } => {
                    if let Some(base_source) = base.get(file_id) {
                        // Restore base version
                        let req = UpsertRequest {
                            canonical_id: Some(file_id.clone()),
                            input_id: file_id.clone(),
                            source: Arc::clone(base_source),
                            file_kind: FileKind::from_path(file_id),
                            aliases: Vec::new(),
                        };
                        let _ = self.host.upsert(req);
                    } else {
                        // File was added by overlay, remove it
                        self.host.remove(file_id);
                    }
                }
                SessionOverlay::Delete => {
                    if let Some(base_source) = base.get(file_id) {
                        // Restore base version that was tombstoned
                        let req = UpsertRequest {
                            canonical_id: Some(file_id.clone()),
                            input_id: file_id.clone(),
                            source: Arc::clone(base_source),
                            file_kind: FileKind::from_path(file_id),
                            aliases: Vec::new(),
                        };
                        let _ = self.host.upsert(req);
                    }
                }
            }
        }
    }

    /// Ensure the given session's overlays are active in the host.
    /// Must be called with the overlay_gate lock held.
    fn ensure_session_context(&self, gate: &mut OverlayState, session_id: u64) {
        if gate.active_session == Some(session_id) {
            return;
        }
        // Revert previous session
        if let Some(prev_id) = gate.active_session.take() {
            self.revert_session_overlays(prev_id);
        }
        // Apply this session
        self.apply_session_overlays(session_id);
        gate.active_session = Some(session_id);
    }

    /// Release a session: remove its overlays if active, remove its state.
    fn release_session(&self, session_id: u64) {
        // If this session's overlays are applied, revert them
        if let Ok(mut gate) = self.overlay_gate.lock() {
            if gate.active_session == Some(session_id) {
                self.revert_session_overlays(session_id);
                gate.active_session = None;
            }
        }
        // Remove session state
        self.sessions.write().remove(&session_id);
    }

    /// Check if a session has any overlays.
    fn session_has_overlays(&self, session_id: u64) -> bool {
        let sessions = self.sessions.read();
        sessions
            .get(&session_id)
            .is_some_and(|s| !s.overlays.is_empty())
    }

    fn reapply_overlay_target(&self, session_id: u64, canonical_id: &str) {
        let sessions = self.sessions.read();
        let Some(state) = sessions.get(&session_id) else {
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
        let _ = self.host.upsert(req);
    }
}

// ---------------------------------------------------------------------------
// MetaSession
// ---------------------------------------------------------------------------

/// A lightweight session handle with isolated file overlays.
///
/// Overlays are private to this session. Queries resolve through
/// `session overlay → shared base`.
pub struct MetaSession {
    id: u64,
    project: Arc<MetaProject>,
    closed: AtomicBool,
}

impl MetaSession {
    fn check_alive(&self) -> Result<(), MetaError> {
        if self.closed.load(Ordering::Acquire) {
            return Err(MetaError::SessionClosed);
        }
        self.project.check_alive()
    }

    /// Session ID.
    #[cfg(test)]
    pub fn id(&self) -> u64 {
        self.id
    }

    /// Resolve an alias to its canonical ID inside this session's overlay view.
    #[allow(dead_code)]
    pub fn resolve_alias_or_canonical(
        &self,
        canonical_or_alias: &str,
    ) -> Result<String, MetaError> {
        self.check_alive()?;
        self.with_overlay_target_context(canonical_or_alias, |host| {
            host.resolve_alias_or_canonical(canonical_or_alias)
        })
    }

    /// Store a file overlay in this session.
    pub fn upsert(&self, canonical_id: &str, source: String) -> Result<(), MetaError> {
        self.check_alive()?;

        let mut gate = self
            .project
            .overlay_gate
            .lock()
            .map_err(|_| MetaError::Host("overlay lock poisoned".into()))?;
        if gate.active_session == Some(self.id) {
            self.project.revert_session_overlays(self.id);
            gate.active_session = None;
        }

        let mut sessions = self.project.sessions.write();
        let state = sessions.get_mut(&self.id).ok_or(MetaError::SessionClosed)?;
        state
            .overlays
            .insert(canonical_id.to_string(), SessionOverlay::Upsert { source });
        state.generation += 1;

        Ok(())
    }

    /// Tombstone a file in this session.
    pub fn delete(&self, canonical_id: &str) -> Result<(), MetaError> {
        self.check_alive()?;

        let mut gate = self
            .project
            .overlay_gate
            .lock()
            .map_err(|_| MetaError::Host("overlay lock poisoned".into()))?;
        if gate.active_session == Some(self.id) {
            self.project.revert_session_overlays(self.id);
            gate.active_session = None;
        }

        let mut sessions = self.project.sessions.write();
        let state = sessions.get_mut(&self.id).ok_or(MetaError::SessionClosed)?;
        state
            .overlays
            .insert(canonical_id.to_string(), SessionOverlay::Delete);
        state.generation += 1;

        Ok(())
    }

    /// Clear any session-local overlay for a file, revealing the shared base
    /// state again.
    pub fn reset(&self, canonical_id: &str) -> Result<(), MetaError> {
        self.check_alive()?;

        let mut gate = self
            .project
            .overlay_gate
            .lock()
            .map_err(|_| MetaError::Host("overlay lock poisoned".into()))?;
        if gate.active_session == Some(self.id) {
            self.project.revert_session_overlays(self.id);
            gate.active_session = None;
        }

        let mut sessions = self.project.sessions.write();
        let state = sessions.get_mut(&self.id).ok_or(MetaError::SessionClosed)?;
        if state.overlays.remove(canonical_id).is_some() {
            state.generation += 1;
        }

        Ok(())
    }

    /// Get the analysis snapshot for a file, resolved through this session's overlay.
    pub fn get_analysis(
        &self,
        canonical_or_alias: &str,
    ) -> Result<Option<crate::types::FileAnalysisSnapshot>, MetaError> {
        self.check_alive()?;
        self.with_overlay_target_context(canonical_or_alias, |host| {
            host.get_analysis(canonical_or_alias)
        })
    }

    /// Evaluate component metadata types through this session's overlay view.
    #[allow(dead_code)]
    pub fn evaluate_types(
        &self,
        canonical_or_alias: &str,
    ) -> Result<Option<verter_semantic::analysis::type_expand::ExpandedComponentTypes>, MetaError>
    {
        self.check_alive()?;
        self.with_overlay_target_context(canonical_or_alias, |host| {
            host.evaluate_types(canonical_or_alias)
        })
    }

    /// Single native component-meta query through this session's overlay view.
    ///
    /// Combines enriched analysis + type evaluation in one call.
    /// Does NOT re-enter the legacy getAnalysis/evaluateTypes workflow.
    pub fn get_component_meta(
        &self,
        canonical_or_alias: &str,
    ) -> Result<Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis>, MetaError>
    {
        self.check_alive()?;
        let resolved = self.with_overlay_target_context(canonical_or_alias, |host| {
            host.get_component_meta_with_resolution(canonical_or_alias)
        })?;

        match resolved {
            Some((analysis, resolved)) => {
                if let Some(err) = component_meta_resolution_budget_error(
                    canonical_or_alias,
                    Some(&analysis),
                    &resolved,
                ) {
                    Err(err)
                } else {
                    Ok(Some(analysis))
                }
            }
            None => Ok(None),
        }
    }

    /// Single native declared-surface component-meta query through this
    /// session's overlay view.
    ///
    /// This skips accepted-surface and fallthrough resolution while preserving
    /// the same budget/error behavior as `get_component_meta()`.
    pub fn get_declared_component_meta(
        &self,
        canonical_or_alias: &str,
    ) -> Result<Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis>, MetaError>
    {
        self.get_declared_component_meta_with_resolution(canonical_or_alias)
            .map(|result| result.map(|(analysis, _resolved)| analysis))
    }

    /// Declared-surface component-meta plus the resolved-meta sidecar in one
    /// overlay-aware query. This preserves declared-only component semantics
    /// while keeping the shared resolved type registry available to callers.
    pub fn get_declared_component_meta_with_resolution(
        &self,
        canonical_or_alias: &str,
    ) -> Result<
        Option<(
            verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
            crate::meta_resolve::ResolvedComponentMetaState,
        )>,
        MetaError,
    > {
        self.check_alive()?;
        self.with_overlay_target_context_view(canonical_or_alias, |host, store_view| {
            let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
            let Some(resolved) = host.resolve_component_meta_in_view(
                canonical.as_str(),
                crate::types::ResolverMode::Expanded,
                store_view,
            ) else {
                return Ok(None);
            };
            let analysis = crate::host_manage::extract_component_meta_from_resolved(
                host,
                canonical.as_str(),
                &resolved,
                false,
                Some(store_view),
            );

            if let Some(err) = component_meta_resolution_budget_error(
                canonical.as_str(),
                Some(&analysis),
                &resolved,
            ) {
                return Err(err);
            }

            Ok(Some((analysis, resolved)))
        })?
    }

    /// Combined component-meta query: returns BOTH the analysis projection AND
    /// the resolved-meta sidecar in a single call, avoiding a duplicate
    /// resolved-state query when callers need both views.
    #[allow(dead_code)]
    pub fn get_component_meta_with_resolution(
        &self,
        canonical_or_alias: &str,
    ) -> Result<
        Option<(
            verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
            crate::meta_resolve::ResolvedComponentMetaState,
        )>,
        MetaError,
    > {
        self.check_alive()?;
        let resolved = self.with_overlay_target_context(canonical_or_alias, |host| {
            host.get_component_meta_with_resolution(canonical_or_alias)
        })?;
        match resolved {
            Some((analysis, resolved)) => {
                if let Some(err) = component_meta_resolution_budget_error(
                    canonical_or_alias,
                    Some(&analysis),
                    &resolved,
                ) {
                    Err(err)
                } else {
                    Ok(Some((analysis, resolved)))
                }
            }
            None => Ok(None),
        }
    }

    // ───────────────────────────────────────────────────────────────────────
    // Payload cache helpers (shared by NAPI/WASM — skip encode on cache hit)
    // ───────────────────────────────────────────────────────────────────────

    /// Attempt to return a cached declared-meta payload, or compute + encode
    /// via the provided `encode_fn`. The caller owns the FFI conversion and
    /// protobuf encoding (which live in downstream crates).
    pub fn get_declared_component_meta_payload(
        &self,
        canonical_or_alias: &str,
        encode_fn: impl FnOnce(
            verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
            &crate::meta_resolve::ResolvedComponentMetaState,
        ) -> Vec<u8>,
    ) -> Result<Option<Vec<u8>>, MetaError> {
        use std::sync::atomic::Ordering::Relaxed;
        self.check_alive()?;
        self.with_overlay_target_context_view(canonical_or_alias, |host, store_view| {
            let canonical = host.resolve_alias_or_canonical(canonical_or_alias);

            // Attempt payload cache hit
            if let Some(cached) = host.try_get_cached_meta_payload(
                canonical.as_str(),
                crate::types::MetaPayloadKind::Declared,
                store_view,
            ) {
                host.provenance().payload_cache_hits.fetch_add(1, Relaxed);
                return Ok(Some(cached));
            }
            host.provenance().payload_cache_misses.fetch_add(1, Relaxed);

            // Miss — compute from scratch
            let Some(resolved) = host.resolve_component_meta_in_view(
                canonical.as_str(),
                crate::types::ResolverMode::Expanded,
                store_view,
            ) else {
                return Ok(None);
            };
            let analysis = crate::host_manage::extract_component_meta_from_resolved(
                host,
                canonical.as_str(),
                &resolved,
                false,
                Some(store_view),
            );

            if let Some(err) = component_meta_resolution_budget_error(
                canonical.as_str(),
                Some(&analysis),
                &resolved,
            ) {
                return Err(err);
            }

            let payload = encode_fn(analysis, &resolved);
            host.provenance().payload_encodes.fetch_add(1, Relaxed);

            // Store in cache
            host.store_meta_payload(
                canonical.as_str(),
                crate::types::MetaPayloadKind::Declared,
                &resolved.fact_versions,
                payload.clone(),
            );

            Ok(Some(payload))
        })?
    }

    /// Attempt to return a cached full-meta payload, or compute + encode
    /// via the provided `encode_fn`.
    ///
    /// Full payloads are validated against fallthrough fact versions (which
    /// include both resolved-state and child-component dependency facts).
    pub fn get_component_meta_payload(
        &self,
        canonical_or_alias: &str,
        encode_fn: impl FnOnce(
            verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
            &crate::meta_resolve::ResolvedComponentMetaState,
        ) -> Vec<u8>,
    ) -> Result<Option<Vec<u8>>, MetaError> {
        use std::sync::atomic::Ordering::Relaxed;
        self.check_alive()?;
        self.with_overlay_target_context_view(canonical_or_alias, |host, store_view| {
            let canonical = host.resolve_alias_or_canonical(canonical_or_alias);

            // Attempt payload cache hit (Full reuses the same slot as Resolved)
            if let Some(cached) = host.try_get_cached_meta_payload(
                canonical.as_str(),
                crate::types::MetaPayloadKind::Full,
                store_view,
            ) {
                host.provenance().payload_cache_hits.fetch_add(1, Relaxed);
                return Ok(Some(cached));
            }
            host.provenance().payload_cache_misses.fetch_add(1, Relaxed);

            // Miss — resolve + build with fallthrough in-view so we capture
            // the fallthrough fact versions for cache validation.
            let Some(resolved) = host.resolve_component_meta_in_view(
                canonical.as_str(),
                crate::types::ResolverMode::Expanded,
                store_view,
            ) else {
                return Ok(None);
            };

            // Build full analysis with fallthrough and capture the fallthrough
            // fact versions for the payload cache key.
            let (analysis, fallthrough_fact_versions) =
                crate::host_manage::extract_component_meta_from_resolved_with_facts(
                    host,
                    canonical.as_str(),
                    &resolved,
                    Some(store_view),
                );

            if let Some(err) = component_meta_resolution_budget_error(
                canonical.as_str(),
                Some(&analysis),
                &resolved,
            ) {
                return Err(err);
            }

            let payload = encode_fn(analysis, &resolved);
            host.provenance().payload_encodes.fetch_add(1, Relaxed);

            // Store with fallthrough fact versions — these include both
            // resolved-state facts and child-component dependency facts.
            let facts = fallthrough_fact_versions.unwrap_or_else(|| resolved.fact_versions.clone());
            host.store_meta_payload(
                canonical.as_str(),
                crate::types::MetaPayloadKind::Full,
                &facts,
                payload.clone(),
            );

            Ok(Some(payload))
        })?
    }

    /// Return provenance counters for this session's host.
    pub fn get_provenance(&self) -> Result<crate::types::MetaProvenanceSnapshot, MetaError> {
        self.check_alive()?;
        Ok(self.project.host.provenance_snapshot())
    }

    /// Get the effective source for a file (overlay → base).
    pub fn get_effective_source(&self, canonical_id: &str) -> Result<Option<String>, MetaError> {
        self.check_alive()?;
        // Check session overlay first
        let sessions = self.project.sessions.read();
        if let Some(state) = sessions.get(&self.id) {
            match state.overlays.get(canonical_id) {
                Some(SessionOverlay::Upsert { source }) => return Ok(Some(source.clone())),
                Some(SessionOverlay::Delete) => return Ok(None),
                None => {}
            }
        }
        drop(sessions);
        // Fall back to base source
        let base = self.project.base_sources.read();
        Ok(base.get(canonical_id).map(|s| s.to_string()))
    }

    /// Check if a file is visible in this session (not tombstoned).
    pub fn has_file(&self, canonical_id: &str) -> Result<bool, MetaError> {
        self.check_alive()?;
        let sessions = self.project.sessions.read();
        if let Some(state) = sessions.get(&self.id) {
            match state.overlays.get(canonical_id) {
                Some(SessionOverlay::Upsert { .. }) => return Ok(true),
                Some(SessionOverlay::Delete) => return Ok(false),
                None => {}
            }
        }
        drop(sessions);
        Ok(self.project.base_file_ids.read().contains(canonical_id))
    }

    /// Iterate canonical IDs visible to this session.
    pub fn visible_file_ids(&self) -> Result<Vec<String>, MetaError> {
        self.check_alive()?;
        let mut ids: std::collections::HashSet<String> = self.project.base_file_ids.read().clone();
        let sessions = self.project.sessions.read();
        if let Some(state) = sessions.get(&self.id) {
            for (file_id, overlay) in &state.overlays {
                match overlay {
                    SessionOverlay::Upsert { .. } => {
                        ids.insert(file_id.clone());
                    }
                    SessionOverlay::Delete => {
                        ids.remove(file_id);
                    }
                }
            }
        }
        Ok(ids.into_iter().collect())
    }

    /// The overlay generation counter for this session.
    pub fn overlay_generation(&self) -> u64 {
        self.project
            .sessions
            .read()
            .get(&self.id)
            .map_or(0, |s| s.generation)
    }

    /// Close the session, releasing the overlay and lease.
    /// Idempotent — safe to call multiple times.
    pub fn close(&self) {
        if self
            .closed
            .compare_exchange(false, true, Ordering::AcqRel, Ordering::Acquire)
            .is_err()
        {
            return; // Already closed
        }
        self.project.release_session(self.id);
    }

    /// Whether this session has been closed.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    // -----------------------------------------------------------------------
    // Internal: run a closure with this session's overlay context applied
    // -----------------------------------------------------------------------

    fn with_overlay_target_context<T>(
        &self,
        canonical_or_alias: &str,
        f: impl FnOnce(&VerterHost) -> T,
    ) -> Result<T, MetaError> {
        self.with_overlay_target_context_view(canonical_or_alias, |host, _store_view| f(host))
    }

    fn with_overlay_target_context_view<T>(
        &self,
        canonical_or_alias: &str,
        f: impl FnOnce(&VerterHost, &crate::host_request_view::RequestStoreView) -> T,
    ) -> Result<T, MetaError> {
        let mut gate = self
            .project
            .overlay_gate
            .lock()
            .map_err(|_| MetaError::Host("overlay lock poisoned".into()))?;

        let has_overlays = self.project.session_has_overlays(self.id);

        if !has_overlays {
            if let Some(prev_id) = gate.active_session.take() {
                self.project.revert_session_overlays(prev_id);
            }
        } else {
            let owner_needs_reapply = gate.active_session != Some(self.id);
            self.project.ensure_session_context(&mut gate, self.id);
            if owner_needs_reapply && !canonical_or_alias.is_empty() {
                // Session overlays come from a HashMap, so the owner file can be
                // upserted before its overlay-only helpers. Reapplying the owner
                // after the session switch keeps its cached import/dependency
                // state aligned with the already-applied helper overlays.
                let canonical = self
                    .project
                    .host
                    .resolve_alias_or_canonical(canonical_or_alias);
                self.project
                    .reapply_overlay_target(self.id, canonical.as_str());
            }
        }

        // §4.2 RequestStoreView install: every meta request runs under a
        // captured view + request-private extension store. The thread-local
        // `CURRENT_REQUEST_VIEW` is populated for the duration of `f` so
        // mid-request `ensure_loaded` calls push into the extension store
        // (via `VerterHost::record_current_request_extension_for`), and
        // `is_evalable` / `external_inputs_memo` consult the view instead of
        // live host state.
        let request_view = self.project.host.build_request_store_view();
        let _request_guard = request_view.install();
        let result = f(&self.project.host, &request_view);
        Ok(result)
    }
}

impl Drop for MetaSession {
    fn drop(&mut self) {
        self.close();
    }
}

#[cfg(test)]
#[path = "meta_tests.rs"]
mod meta_tests;
