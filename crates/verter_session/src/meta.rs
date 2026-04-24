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
//! # Concurrency (C15)
//!
//! Per-session isolation is structural via `SessionRuntime`'s
//! `ArcSwap<SessionView>` snapshots. No project-wide lock serializes
//! overlay-aware queries. Readers load the snapshot lock-free; writers
//! serialize via the per-session `view_writer_lock`.

use std::collections::HashMap;
use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Arc;

use crate::session_runtime::SessionRuntime;
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
// SessionState — per-session overlay tracking
// ---------------------------------------------------------------------------

pub(crate) struct SessionState {
    pub(crate) overlays: HashMap<String, SessionOverlay>,
    pub(crate) generation: u64,
}

// ---------------------------------------------------------------------------
// MetaProject
// ---------------------------------------------------------------------------

/// Shared project state wrapping one [`VerterHost`].
///
/// Multiple [`MetaSession`]s can be opened against the same project.
/// The project owns the host, base file cache, and per-session state.
/// Overlay serialization is per-session via `SessionRuntime`'s
/// `view_writer_lock` (C15); the project-wide `overlay_gate` is retired.
pub struct MetaProject {
    host: VerterHost,
    /// Cached base sources for overlay revert. Key = canonical ID.
    base_sources: parking_lot::RwLock<HashMap<String, Arc<str>>>,
    /// Set of canonical IDs in the base file index.
    base_file_ids: parking_lot::RwLock<std::collections::HashSet<String>>,
    /// Per-session state, keyed by session ID.
    sessions: parking_lot::RwLock<HashMap<u64, SessionState>>,
    /// Monotonic session ID counter.
    next_session_id: AtomicU64,
    /// Terminal shutdown flag.
    shutdown: AtomicBool,
    /// C15: lock-free tracking of which session's overlays are currently
    /// applied to the shared host. 0 = no session active. Replaces the
    /// retired `overlay_gate: Mutex<OverlayState>` — reads and writes
    /// are atomic, no Mutex contention between sessions.
    active_overlay_session: AtomicU64,
}

impl MetaProject {
    /// Create a new project wrapping the given host.
    pub fn new(host: VerterHost) -> Arc<Self> {
        Arc::new(Self {
            host,
            base_sources: parking_lot::RwLock::new(HashMap::new()),
            base_file_ids: parking_lot::RwLock::new(std::collections::HashSet::new()),
            sessions: parking_lot::RwLock::new(HashMap::new()),
            next_session_id: AtomicU64::new(1),
            shutdown: AtomicBool::new(false),
            active_overlay_session: AtomicU64::new(0),
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

    // C15: overlay_gate, acquire_overlay_gate, enter_base_context, and
    // revert_active_session_overlays are all retired. Per-session isolation
    // is structural via SessionRuntime's ArcSwap<SessionView> snapshots
    // and session-scoped caches. Base-context operations (upsert_base,
    // ensure_loaded, etc.) operate directly on the host without a gate.

    /// Load a file into the base project. This is the shared state that
    /// all sessions see when they don't have an overlay for the file.
    pub fn upsert_base(
        self: &Arc<Self>,
        canonical_id: &str,
        source: &str,
    ) -> Result<(), MetaError> {
        self.check_alive()?;
        // C15: no overlay_gate — base operations go directly to host.
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
        // C15: no overlay_gate — base operations go directly to host.

        let loaded = self.host.ensure_loaded(canonical_id);

        if !loaded {
            self.remove_base_file(canonical_id);
            return Ok(false);
        }

        Ok(self.sync_base_file_from_host(canonical_id))
    }

    /// Refresh a workspace-backed base file from the current native workspace.
    pub fn refresh_base(self: &Arc<Self>, canonical_id: &str) -> Result<bool, MetaError> {
        self.check_alive()?;
        // C15: no overlay_gate — base operations go directly to host.

        self.host.evict(canonical_id);

        let loaded = self.host.ensure_loaded(canonical_id);

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
        // C15: no overlay_gate — base operations go directly to host.
        self.host.configure_projects(projects);
        Ok(())
    }

    /// Open a new session against this project (interactive mode).
    ///
    /// LSP callers + most user-facing consumers should use this
    /// constructor. For batch workloads (test harness, MCP server),
    /// see [`Self::open_session_batch`].
    pub fn open_session(self: &Arc<Self>) -> Result<MetaSession, MetaError> {
        self.open_session_with_mode(ExecutionMode::Interactive)
    }

    /// Open a new session in batch execution mode (Path C C12).
    ///
    /// Batch mode opts into the scheduler's batched submission surface
    /// where callers submit N independent requests before any waits.
    /// The scheduler fans them out onto its Rayon pool. Test harness
    /// and MCP server callers use this path.
    #[allow(dead_code)]
    pub fn open_session_batch(self: &Arc<Self>) -> Result<MetaSession, MetaError> {
        self.open_session_with_mode(ExecutionMode::Batch)
    }

    /// Open a new session with an explicit execution mode.
    fn open_session_with_mode(
        self: &Arc<Self>,
        execution_mode: ExecutionMode,
    ) -> Result<MetaSession, MetaError> {
        self.check_alive()?;
        let id = self.next_session_id.fetch_add(1, Ordering::Relaxed);
        self.sessions.write().insert(
            id,
            SessionState {
                overlays: HashMap::new(),
                generation: 0,
            },
        );
        let runtime = SessionRuntime::new(id, Arc::clone(self));
        Ok(MetaSession {
            id,
            project: Arc::clone(self),
            closed: AtomicBool::new(false),
            execution_mode,
            runtime,
        })
    }

    /// Clear shared analysis caches without shutting down.
    /// Active sessions keep their overlays; only base caches are flushed.
    pub fn clear_caches(&self) -> Result<(), MetaError> {
        self.check_alive()?;
        // C15: no overlay_gate — just clear host caches directly.
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
        // C15: no overlay_gate to revert — session-scoped isolation is
        // structural via ArcSwap snapshots. Just close the host.
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
    // Crate-internal accessors for SessionRuntime
    // -----------------------------------------------------------------------

    /// Read-only access to per-session state (overlays, generation).
    pub(crate) fn sessions_read(
        &self,
    ) -> parking_lot::RwLockReadGuard<'_, HashMap<u64, SessionState>> {
        self.sessions.read()
    }

    /// Read-only access to cached base sources.
    pub(crate) fn base_sources_read(
        &self,
    ) -> parking_lot::RwLockReadGuard<'_, HashMap<String, Arc<str>>> {
        self.base_sources.read()
    }

    /// Check if a session has any overlays.
    pub(crate) fn session_has_overlays(&self, session_id: u64) -> bool {
        let sessions = self.sessions.read();
        sessions
            .get(&session_id)
            .is_some_and(|s| !s.overlays.is_empty())
    }

    // -----------------------------------------------------------------------
    // Internal: session lifecycle
    // -----------------------------------------------------------------------

    /// Release a session: revert its overlays if active, remove its state.
    fn release_session(&self, session_id: u64, runtime: &SessionRuntime) {
        if self
            .active_overlay_session
            .compare_exchange(session_id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            runtime.revert_other_session_overlays(session_id);
        }
        self.sessions.write().remove(&session_id);
    }
}

// ---------------------------------------------------------------------------
// MetaSession
// ---------------------------------------------------------------------------

/// Path C C12 — session execution mode (plan §2 Stage 7 Pass C12).
///
/// Separates interactive-latency callers (LSP) from batch-throughput
/// callers (test harness, MCP server). Interactive mode matches the
/// pre-C12 single-request-then-wait path; Batch mode opts into the
/// scheduler's `submit_batch` / `wait_batch` surface so N independent
/// requests fan out onto the Rayon pool.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ExecutionMode {
    /// Single-request latency-sensitive path. Default for
    /// [`MetaProject::open_session`]. LSP callers stay here.
    #[default]
    Interactive,
    /// Batch-throughput path. Test harness, MCP server. Opt-in via
    /// [`MetaProject::open_session_batch`].
    Batch,
}

/// A lightweight session handle with isolated file overlays.
///
/// Overlays are private to this session. Queries resolve through
/// `session overlay → shared base`.
pub struct MetaSession {
    id: u64,
    project: Arc<MetaProject>,
    closed: AtomicBool,
    /// Path C C12 — per-session execution mode. Scheduler dispatch
    /// branches on this to choose between interactive and batch
    /// surfaces. Currently consumed via [`Self::execution_mode`];
    /// C13 wires component-meta job dispatch to honour the flag.
    #[allow(dead_code)]
    execution_mode: ExecutionMode,
    /// Path C C14 — session-owned runtime for overlay-sensitive
    /// request execution. Owns session identity, overlay context
    /// lifecycle, and session-scoped resolved-meta cache.
    runtime: SessionRuntime,
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

    /// Access the project-level host. Needed by the audit-bundle path
    /// (plan §3 Commit 8) to call `take_audit_record` after a
    /// resolution completes.
    pub fn host(&self) -> &VerterHost {
        self.project.host()
    }

    /// Path C C12 — this session's execution mode.
    #[allow(dead_code)]
    pub fn execution_mode(&self) -> ExecutionMode {
        self.execution_mode
    }

    /// Resolve an alias to its canonical ID inside this session's overlay view.
    #[allow(dead_code)]
    pub fn resolve_alias_or_canonical(
        &self,
        canonical_or_alias: &str,
    ) -> Result<String, MetaError> {
        self.check_alive()?;
        self.with_overlay_target_context(canonical_or_alias, |runtime| {
            runtime
                .host()
                .resolve_alias_or_canonical(canonical_or_alias)
        })
    }

    /// Invalidate the active overlay state when this session's overlays change.
    /// If this session's overlays were applied to the host, revert them so the
    /// host returns to base state. The next query re-applies the updated overlays.
    fn invalidate_active_overlays(&self) {
        if self
            .project
            .active_overlay_session
            .compare_exchange(self.id, 0, Ordering::AcqRel, Ordering::Acquire)
            .is_ok()
        {
            self.runtime.revert_other_session_overlays(self.id);
        }
        self.runtime.invalidate_session_caches();
    }

    /// Store a file overlay in this session.
    pub fn upsert(&self, canonical_id: &str, source: String) -> Result<(), MetaError> {
        self.check_alive()?;

        let mut sessions = self.project.sessions.write();
        let state = sessions.get_mut(&self.id).ok_or(MetaError::SessionClosed)?;
        state
            .overlays
            .insert(canonical_id.to_string(), SessionOverlay::Upsert { source });
        state.generation += 1;
        drop(sessions);

        self.invalidate_active_overlays();
        Ok(())
    }

    /// Tombstone a file in this session.
    pub fn delete(&self, canonical_id: &str) -> Result<(), MetaError> {
        self.check_alive()?;

        let mut sessions = self.project.sessions.write();
        let state = sessions.get_mut(&self.id).ok_or(MetaError::SessionClosed)?;
        state
            .overlays
            .insert(canonical_id.to_string(), SessionOverlay::Delete);
        state.generation += 1;
        drop(sessions);

        self.invalidate_active_overlays();
        Ok(())
    }

    /// Clear any session-local overlay for a file, revealing the shared base
    /// state again.
    pub fn reset(&self, canonical_id: &str) -> Result<(), MetaError> {
        self.check_alive()?;

        // Revert BEFORE removing overlay from state — the revert reads
        // the overlay map to know which files to restore.
        let has_overlay = self
            .project
            .sessions
            .read()
            .get(&self.id)
            .is_some_and(|s| s.overlays.contains_key(canonical_id));
        if has_overlay {
            self.invalidate_active_overlays();
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
        self.with_overlay_target_context(canonical_or_alias, |runtime| {
            runtime.host().get_analysis(canonical_or_alias)
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
        self.with_overlay_target_context(canonical_or_alias, |runtime| {
            runtime.host().evaluate_types(canonical_or_alias)
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
        let resolved = self.with_overlay_target_context(canonical_or_alias, |runtime| {
            runtime.get_component_meta_with_resolution(canonical_or_alias)
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

    /// Path C C13 — Batch-mode fan-out for N independent component-meta
    /// queries through the scheduler's CPU pool.
    ///
    /// Constructs one [`SchedulerJobKind::ComponentMeta`] per requested
    /// canonical id and submits the batch to
    /// [`Scheduler::dispatch_meta_jobs`], which runs each query in
    /// parallel on the scheduler's Rayon pool. Returns per-id results
    /// in submission order. Use this from test harnesses, the MCP
    /// server, and any other Batch-mode caller that has more than one
    /// independent component-meta query in flight at a time.
    ///
    /// Interactive callers (LSP, single-request SFC fetches) should
    /// continue using [`Self::get_component_meta`] — the single-request
    /// synchronous path is the lowest-latency option for one query and
    /// avoids the Rayon scheduling overhead Batch mode introduces.
    ///
    /// Returns `Err(MetaError::Shutdown)` when the project has been
    /// shut down. Per-query budget errors surface in the per-result
    /// `Result` slot, so the caller can inspect each query
    /// independently.
    #[allow(dead_code)]
    pub fn get_component_meta_batch(
        &self,
        canonical_or_aliases: &[String],
    ) -> Result<
        Vec<
            Result<
                Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis>,
                MetaError,
            >,
        >,
        MetaError,
    > {
        use std::sync::Arc;
        self.check_alive()?;
        let scheduler = self.project.host().scheduler();
        let jobs: Vec<verter_scheduler::stage::SchedulerJobKind> = canonical_or_aliases
            .iter()
            .map(
                |canonical| verter_scheduler::stage::SchedulerJobKind::ComponentMeta {
                    canonical_id: Arc::from(canonical.as_str()),
                },
            )
            .collect();
        // Self-borrow for the closure — scheduler.dispatch_meta_jobs
        // requires `Sync + Send` on the executor. `&MetaSession` is
        // `Sync` because all interior shared state goes through
        // `Arc`/atomics. The closure body re-enters the synchronous
        // `get_component_meta` path so overlay-aware resolution stays
        // identical to the Interactive path.
        let session_ref = self;
        let results = scheduler.dispatch_meta_jobs(jobs, |job| {
            let verter_scheduler::stage::SchedulerJobKind::ComponentMeta { canonical_id } = job;
            session_ref.get_component_meta(canonical_id.as_ref())
        });
        Ok(results)
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
        self.with_overlay_target_context(canonical_or_alias, |runtime| {
            let host = runtime.host();
            let canonical = host.resolve_alias_or_canonical(canonical_or_alias);
            let Some(resolved) = runtime
                .resolve_component_meta(canonical.as_str(), crate::types::ProjectionMode::Expanded)
            else {
                return Ok(None);
            };
            let analysis = crate::host_manage::extract_component_meta_from_resolved(
                host,
                canonical.as_str(),
                &resolved,
                false,
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
        let resolved = self.with_overlay_target_context(canonical_or_alias, |runtime| {
            runtime.get_component_meta_with_resolution(canonical_or_alias)
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
        self.with_overlay_target_context(canonical_or_alias, |runtime| {
            let host = runtime.host();
            let canonical = host.resolve_alias_or_canonical(canonical_or_alias);

            if let Some(cached) = host.try_get_cached_meta_payload(
                canonical.as_str(),
                crate::types::MetaPayloadKind::Declared,
            ) {
                host.provenance().payload_cache_hits.fetch_add(1, Relaxed);
                return Ok(Some(cached));
            }
            host.provenance().payload_cache_misses.fetch_add(1, Relaxed);

            let Some(resolved) = runtime
                .resolve_component_meta(canonical.as_str(), crate::types::ProjectionMode::Expanded)
            else {
                return Ok(None);
            };
            let analysis = crate::host_manage::extract_component_meta_from_resolved(
                host,
                canonical.as_str(),
                &resolved,
                false,
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
        self.with_overlay_target_context(canonical_or_alias, |runtime| {
            let host = runtime.host();
            let canonical = host.resolve_alias_or_canonical(canonical_or_alias);

            if let Some(cached) = host.try_get_cached_meta_payload(
                canonical.as_str(),
                crate::types::MetaPayloadKind::Full,
            ) {
                host.provenance().payload_cache_hits.fetch_add(1, Relaxed);
                return Ok(Some(cached));
            }
            host.provenance().payload_cache_misses.fetch_add(1, Relaxed);

            let Some(resolved) = runtime
                .resolve_component_meta(canonical.as_str(), crate::types::ProjectionMode::Expanded)
            else {
                return Ok(None);
            };

            let (analysis, fallthrough_fact_versions) =
                crate::host_manage::extract_component_meta_from_resolved_with_facts(
                    host,
                    canonical.as_str(),
                    &resolved,
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
        self.project.release_session(self.id, &self.runtime);
    }

    /// Whether this session has been closed.
    pub fn is_closed(&self) -> bool {
        self.closed.load(Ordering::Acquire)
    }

    // -----------------------------------------------------------------------
    // Internal: run a closure with this session's overlay context applied
    // -----------------------------------------------------------------------

    /// Run a closure with this session's overlay context applied.
    ///
    /// Uses compare_exchange to atomically claim the active overlay slot,
    /// preventing TOCTOU races between concurrent sessions.
    fn with_overlay_target_context<T>(
        &self,
        canonical_or_alias: &str,
        f: impl FnOnce(&SessionRuntime) -> T,
    ) -> Result<T, MetaError> {
        let has_overlays = self.project.session_has_overlays(self.id);

        if has_overlays {
            loop {
                let current = self.project.active_overlay_session.load(Ordering::Acquire);
                if current == self.id {
                    break;
                }
                // Atomically claim: CAS current → self.id.
                if self
                    .project
                    .active_overlay_session
                    .compare_exchange(current, self.id, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    if current != 0 {
                        self.runtime.revert_other_session_overlays(current);
                    }
                    self.runtime.apply_own_overlays();
                    break;
                }
                // CAS failed — another session raced us; retry.
            }
            if !canonical_or_alias.is_empty() {
                let canonical = self
                    .project
                    .host
                    .resolve_alias_or_canonical(canonical_or_alias);
                self.runtime.reapply_overlay_target(canonical.as_str());
            }
        } else {
            loop {
                let current = self.project.active_overlay_session.load(Ordering::Acquire);
                if current == 0 {
                    break;
                }
                // Atomically clear: CAS current → 0.
                if self
                    .project
                    .active_overlay_session
                    .compare_exchange(current, 0, Ordering::AcqRel, Ordering::Acquire)
                    .is_ok()
                {
                    self.runtime.revert_other_session_overlays(current);
                    break;
                }
            }
        }

        self.runtime.refresh_view();
        Ok(f(&self.runtime))
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
