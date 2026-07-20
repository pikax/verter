//! Host-backed component-meta session layer.
//!
//! This replaces the public `MetaProject` / `MetaSession` naming, but keeps the
//! underlying behavioral contract that component-meta depends on:
//! - project/base state is shared
//! - sessions hold isolated overlays
//! - closing a session releases its overlays
//! - native component-meta queries stay on the shared host/session path

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;

#[cfg(test)]
use crate::resolver_core::{
    component_meta_resolved_macros as resolver_component_meta_resolved_macros,
    component_meta_type_registry as resolver_component_meta_type_registry,
};
#[cfg(test)]
use verter_semantic::analysis::component_meta::ComponentMetaAnalysis;
#[cfg(test)]
use verter_semantic::analysis::component_meta::{
    AcceptedSurfaceCompleteness, FallthroughSurface, RootReachability,
};
#[cfg(test)]
use verter_semantic::analysis::type_expand::ExpandedComponentTypes;
#[cfg(test)]
use verter_type_expr::{ObjectMember, TypeExpr};

use crate::host_manage::component_meta_trace_custom;
use crate::VerterHost;

// Test hosts construct via `new_standalone_with_scheduler_config` with
// `SchedulerConfig { cpu_threads: 1, .. }`. The previous global
// `HEAVY_COMPONENT_META_TEST_MUTEX` (and its `acquires` / `wait_ns`
// counters) was a workaround for OS-level CPU oversubscription when each
// per-test `Scheduler` spawned `cpu_threads = num_cpus()` worker pools;
// pinning the scheduler to one CPU thread eliminates the contention
// entirely, so no shared mutex is required.

// ---------------------------------------------------------------------------
// Error
// ---------------------------------------------------------------------------

/// Errors from ComponentMetaHost operations.
#[derive(Debug, Clone, thiserror::Error)]
pub enum ComponentMetaHostError {
    #[error("host has been shut down")]
    Shutdown,
    #[error("host error: {0}")]
    Host(String),
    /// Audit bundle requested but the host config does not enable
    /// `audit_enabled` + `footprint_capture`.
    #[error(
        "audit is not enabled on this host — set HostConfig::audit_enabled and \
         HostConfig::footprint_capture before calling get_component_meta_with_audit"
    )]
    AuditNotEnabled,
    /// The resolution returned a request_id, but no audit record was
    /// available at retrieval time. This can happen when the audit
    /// record was displaced from the bounded store by subsequent
    /// requests (store holds up to
    /// [`crate::component_meta_audit::AUDIT_RECORDS_STORE_CAPACITY`]
    /// entries).
    #[error("audit record for request_id={request_id} missing — store may have evicted it")]
    AuditRecordMissing { request_id: u64 },
    /// Typed output-materialization failure (fail-closed): a PRESENT type
    /// source the terminal output sink could not raise / shell-materialize
    /// — never silently rendered as `Unknown`.
    #[error("output materialization error: {0}")]
    OutputMaterialization(#[from] crate::meta_resolve::ComponentMetaOutputError),
}
impl From<crate::meta::MetaError> for ComponentMetaHostError {
    fn from(value: crate::meta::MetaError) -> Self {
        match value {
            crate::meta::MetaError::Shutdown => Self::Shutdown,
            crate::meta::MetaError::SessionClosed => Self::Host("session is closed".to_string()),
            crate::meta::MetaError::Host(message) => Self::Host(message),
            // The typed variant SURVIVES the host boundary: demoting it to a
            // `Host(String)` would erase the failed lane / positional index /
            // interior path a consumer needs to distinguish a fail-closed
            // output-materialization failure from an ordinary host fault.
            crate::meta::MetaError::OutputMaterialization(err) => Self::OutputMaterialization(err),
        }
    }
}

struct ComponentMetaHostInner {
    project: Arc<crate::meta::MetaProject>,
    generation: AtomicU64,
}

// ---------------------------------------------------------------------------
// ComponentMetaHost
// ---------------------------------------------------------------------------

/// Shared component-meta host with isolated overlay sessions.
pub struct ComponentMetaHost {
    inner: Arc<ComponentMetaHostInner>,
}

impl ComponentMetaHost {
    /// Create a new component-meta host with a standalone memory workspace.
    pub fn new_standalone(config: crate::types::HostConfig) -> Self {
        let project = crate::meta::MetaProject::new(VerterHost::new_standalone(config));
        Self {
            inner: Arc::new(ComponentMetaHostInner {
                project,
                generation: AtomicU64::new(0),
            }),
        }
    }

    /// Create a new component-meta host with a standalone memory workspace and
    /// an explicit [`SchedulerConfig`].
    ///
    /// Test harnesses construct hosts with
    /// `SchedulerConfig { cpu_threads: 1, ..SchedulerConfig::default() }`
    /// to avoid CPU oversubscription when many parallel test threads
    /// each spin up their own scheduler thread pools.
    pub fn new_standalone_with_scheduler_config(
        config: crate::types::HostConfig,
        scheduler_config: verter_scheduler::scheduler::SchedulerConfig,
    ) -> Self {
        let host = VerterHost::new_standalone_with_scheduler_config(config, scheduler_config);
        let project = crate::meta::MetaProject::new(host);
        Self {
            inner: Arc::new(ComponentMetaHostInner {
                project,
                generation: AtomicU64::new(0),
            }),
        }
    }

    /// Create a new component-meta host backed by an existing workspace.
    pub fn new(
        config: crate::types::HostConfig,
        workspace: Arc<dyn verter_workspace::WorkspaceAccess>,
    ) -> Self {
        let host = VerterHost::new(config, workspace);
        let project = crate::meta::MetaProject::new(host);
        Self {
            inner: Arc::new(ComponentMetaHostInner {
                project,
                generation: AtomicU64::new(0),
            }),
        }
    }

    fn check_alive(&self) -> Result<(), ComponentMetaHostError> {
        if self.inner.project.is_shutdown() {
            return Err(ComponentMetaHostError::Shutdown);
        }
        Ok(())
    }

    /// Access the underlying host.
    pub fn host(&self) -> &VerterHost {
        self.inner.project.host()
    }

    /// Current base-state revision.
    pub fn generation(&self) -> u64 {
        self.inner.generation.load(Ordering::Acquire)
    }

    /// Load a file into the shared base project.
    pub fn upsert_base(
        &self,
        canonical_id: &str,
        source: &str,
    ) -> Result<(), ComponentMetaHostError> {
        self.check_alive()?;
        self.inner
            .project
            .upsert_base(canonical_id, source)
            .map_err(ComponentMetaHostError::from)?;
        self.inner.generation.fetch_add(1, Ordering::Release);
        Ok(())
    }

    /// Ensure a workspace-backed base file is loaded.
    pub fn ensure_loaded(&self, canonical_id: &str) -> Result<bool, ComponentMetaHostError> {
        self.check_alive()?;
        let loaded = self
            .inner
            .project
            .ensure_loaded(canonical_id)
            .map_err(ComponentMetaHostError::from)?;
        if loaded {
            self.inner.generation.fetch_add(1, Ordering::Release);
        }
        Ok(loaded)
    }

    /// Refresh a workspace-backed base file from the current workspace.
    pub fn refresh_base(&self, canonical_id: &str) -> Result<bool, ComponentMetaHostError> {
        self.check_alive()?;
        let loaded = self
            .inner
            .project
            .refresh_base(canonical_id)
            .map_err(ComponentMetaHostError::from)?;
        self.inner.generation.fetch_add(1, Ordering::Release);
        Ok(loaded)
    }

    /// Open a new isolated session against this host.
    pub fn open_session(&self) -> Result<ComponentMetaSession, ComponentMetaHostError> {
        self.check_alive()?;
        Ok(ComponentMetaSession {
            inner: self
                .inner
                .project
                .open_session()
                .map_err(ComponentMetaHostError::from)?,
        })
    }

    /// Open a new isolated session in [`MetaExecutionMode::Batch`] mode.
    ///
    /// Test harness and MCP server callers use this path. LSP callers stay on
    /// [`Self::open_session`] (Interactive mode).
    pub fn open_session_batch(&self) -> Result<ComponentMetaSession, ComponentMetaHostError> {
        self.check_alive()?;
        Ok(ComponentMetaSession {
            inner: self
                .inner
                .project
                .open_session_batch()
                .map_err(ComponentMetaHostError::from)?,
        })
    }

    /// Configure project-scoped path aliases.
    pub fn configure_projects(
        &self,
        configs: Vec<verter_semantic::analysis::project_resolver::IdeProjectConfig>,
    ) -> Result<(), ComponentMetaHostError> {
        self.check_alive()?;
        self.inner
            .project
            .configure_projects(configs)
            .map_err(ComponentMetaHostError::from)
    }

    /// Clear shared compile caches.
    pub fn clear_caches(&self) -> Result<(), ComponentMetaHostError> {
        self.check_alive()?;
        self.inner
            .project
            .clear_caches()
            .map_err(ComponentMetaHostError::from)
    }

    /// Terminal shutdown.
    pub fn shutdown(&self) {
        self.inner.project.shutdown();
    }

    /// Whether this host has been shut down.
    pub fn is_shutdown(&self) -> bool {
        self.inner.project.is_shutdown()
    }

    /// Canonical IDs of files in the shared base index.
    pub fn base_file_ids(&self) -> Vec<String> {
        let mut ids: Vec<_> = self.inner.project.base_file_ids().into_iter().collect();
        ids.sort();
        ids
    }

    /// Number of active sessions.
    pub fn session_count(&self) -> usize {
        self.inner.project.session_count()
    }
}

// ---------------------------------------------------------------------------
// ComponentMetaSession
// ---------------------------------------------------------------------------

/// Isolated overlay session backed by the shared host/project.
pub struct ComponentMetaSession {
    inner: crate::meta::MetaSession,
}

impl ComponentMetaSession {
    /// Store a file overlay in this session.
    pub fn upsert(&self, canonical_id: &str, source: String) -> Result<(), ComponentMetaHostError> {
        self.inner
            .upsert(canonical_id, source)
            .map_err(ComponentMetaHostError::from)
    }

    /// Tombstone a file in this session.
    pub fn delete(&self, canonical_id: &str) -> Result<(), ComponentMetaHostError> {
        self.inner
            .delete(canonical_id)
            .map_err(ComponentMetaHostError::from)
    }

    /// Clear a session-local overlay for a file, revealing the shared base.
    pub fn reset(&self, canonical_id: &str) -> Result<(), ComponentMetaHostError> {
        self.inner
            .reset(canonical_id)
            .map_err(ComponentMetaHostError::from)
    }

    /// Get the effective source for a file in this session.
    pub fn get_effective_source(
        &self,
        canonical_id: &str,
    ) -> Result<Option<String>, ComponentMetaHostError> {
        self.inner
            .get_effective_source(canonical_id)
            .map_err(ComponentMetaHostError::from)
    }

    /// Check whether a file is visible in this session.
    pub fn has_file(&self, canonical_id: &str) -> Result<bool, ComponentMetaHostError> {
        self.inner
            .has_file(canonical_id)
            .map_err(ComponentMetaHostError::from)
    }

    /// Get component metadata in this session's overlay context.
    pub fn get_component_meta(
        &self,
        canonical_or_alias: &str,
    ) -> Result<
        Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis>,
        ComponentMetaHostError,
    > {
        component_meta_trace_custom!("component_meta_session_query", canonical_or_alias);
        self.inner
            .get_component_meta(canonical_or_alias)
            .map_err(ComponentMetaHostError::from)
    }

    /// Batch surface for [`Self::get_component_meta`]: compute metadata
    /// for `canonical_or_aliases` under one shared overlay view. Routes
    /// the batch through `HostBatchCoordinator` — on native it fans the N
    /// queries out on the host-owned `HostCpuPool`, on wasm it runs
    /// inline/sequentially — accounting the submission once per non-empty
    /// batch (skipped for an empty batch).
    ///
    /// Delegates to [`crate::meta::MetaSession::get_component_meta_batch`]
    /// so all N queries share the host-owned admission caches
    /// (`MaterializeStructureDb`, `ComponentMetaResultDb`,
    /// `SemanticGraphStore`). Per-id failures surface in each result
    /// slot; the batch does not abort.
    pub fn get_component_meta_batch(
        &self,
        canonical_or_aliases: &[String],
    ) -> Result<
        Vec<
            Result<
                Option<verter_semantic::analysis::component_meta::ComponentMetaAnalysis>,
                ComponentMetaHostError,
            >,
        >,
        ComponentMetaHostError,
    > {
        component_meta_trace_custom!(
            "component_meta_session_batch",
            format!("batch_size={}", canonical_or_aliases.len()),
        );
        let raw = self
            .inner
            .get_component_meta_batch(canonical_or_aliases)
            .map_err(ComponentMetaHostError::from)?;
        Ok(raw
            .into_iter()
            .map(|slot| slot.map_err(ComponentMetaHostError::from))
            .collect())
    }

    /// Output-envelope query in this session's overlay context: the
    /// session-owned [`crate::meta_resolve::ComponentMetaOutput`] with ALL
    /// 11 materialized wire type lanes and no resolution sidecar (the
    /// type lanes are fully resolved; the payload and audited entries
    /// seed the sidecar).
    pub fn get_component_meta_output(
        &self,
        canonical_or_alias: &str,
    ) -> Result<Option<crate::meta_resolve::ComponentMetaOutput>, ComponentMetaHostError> {
        component_meta_trace_custom!("component_meta_session_output_query", canonical_or_alias);
        self.inner
            .get_component_meta_output(canonical_or_alias)
            .map_err(ComponentMetaHostError::from)
    }

    /// Batch surface for [`Self::get_component_meta_output`]: one shared
    /// overlay view, one captured fixed view threaded into every per-job
    /// call, one host-coordinated batch submission.
    ///
    /// Returns one slot per input in input order; `None` is reserved
    /// EXCLUSIVELY for a genuinely missing canonical. A real per-id failure
    /// (budget overrun, typed output-materialization failure, per-job
    /// panic) FAILS THE CALL with the first typed error — exactly as the
    /// scalar [`Self::get_component_meta_output`] does (scalar ≡ batch) —
    /// so a failure is never collapsed onto the missing sentinel.
    pub fn get_component_meta_output_batch(
        &self,
        canonical_or_aliases: &[String],
    ) -> Result<Vec<Option<crate::meta_resolve::ComponentMetaOutput>>, ComponentMetaHostError> {
        component_meta_trace_custom!(
            "component_meta_session_output_batch",
            format!("batch_size={}", canonical_or_aliases.len()),
        );
        self.inner
            .get_component_meta_output_batch(canonical_or_aliases)
            .map_err(ComponentMetaHostError::from)?
            .into_iter()
            .map(|slot| slot.map_err(ComponentMetaHostError::from))
            .collect()
    }

    /// Get component metadata plus the resolved-state sidecar, packaged
    /// in the shared [`verter_audit::AuditedResult`] carrier alongside
    /// the per-request audit record produced by the same resolution.
    /// Synchronous — the audit record is retrievable immediately after
    /// `get_component_meta_with_resolution` returns.
    ///
    /// This is the audited component-meta host seam. It rides the same
    /// always-a-record carrier every other `*_with_audit` entry-point
    /// uses, so there is no tuple-with-record dual path:
    /// - `Ok(Some((analysis, resolution)))` — the canonical resolved
    ///   and audit capture produced a record (carried in `audit`).
    /// - `Ok(None)` — the canonical does not resolve. The carrier still
    ///   holds a cheap default-filled record so the always-a-record
    ///   contract holds.
    /// - `Err(ComponentMetaHostError)` — a genuine request fault
    ///   (`AuditNotEnabled` when the host config does not enable
    ///   `audit_enabled` + `footprint_capture`, or `AuditRecordMissing`
    ///   when the bounded store evicted the record before retrieval).
    ///   The carrier carries a cheap default-filled record marked
    ///   `AuditDisabled` / `FilteredNoop` respectively so a consumer can
    ///   still read `audit` regardless of outcome.
    pub fn get_component_meta_with_audit(
        &self,
        canonical_or_alias: &str,
    ) -> verter_audit::AuditedResult<
        Option<crate::meta_resolve::ComponentMetaOutput>,
        ComponentMetaHostError,
    > {
        let host = self.inner.host();
        if !host.config.audit_enabled || !host.config.footprint_capture {
            // No audit record is produced when capture is off — carry
            // the cheap default-filled record marked `AuditDisabled`
            // so the carrier's always-a-record contract still holds.
            return verter_audit::AuditedResult::err(
                ComponentMetaHostError::AuditNotEnabled,
                cheap_component_meta_record(
                    canonical_or_alias,
                    verter_audit::AuditCaptureState::AuditDisabled,
                ),
            );
        }
        // Audit capture is instrumented on the host-level path — it
        // installs the `RequestContext` TLS, stamps the request id, and
        // publishes the record into the bounded store. The output-bearing
        // resolution entry materializes the wire envelope INSIDE the same
        // request-bound validated view the analysis is served under, so
        // the audit bundle's payload and record describe one snapshot.
        // EVERY terminal of that entry carries the request id, so the
        // error path retrieves (and drains) the SAME real record the
        // resolution published — audited identically to success; the cheap
        // fallback applies only when no record was produced or the bounded
        // store evicted it.
        let (output, request_id) =
            match host.get_component_meta_output_with_resolution(canonical_or_alias) {
                Ok((Some(output), request_id)) => (output, request_id),
                Ok((None, request_id)) => {
                    // No analysis behind the request: a non-fault miss rides
                    // `Ok(None)`. Drain the real record when the resolution
                    // published one; otherwise carry the cheap default.
                    let record = host.take_audit_record(request_id).unwrap_or_else(|| {
                        cheap_component_meta_record(
                            canonical_or_alias,
                            verter_audit::AuditCaptureState::FilteredNoop,
                        )
                    });
                    return verter_audit::AuditedResult::ok(None, record);
                }
                Err((err, request_id)) => {
                    // Typed output-materialization failure (fail-closed) —
                    // the payload is refused, never collapsed to `Unknown`.
                    // The resolution itself completed and published its REAL
                    // record before materialization failed: drain and return
                    // THAT record (never a fabricated zero-id stand-in, and
                    // never an orphan left in the store).
                    let record = host.take_audit_record(request_id).unwrap_or_else(|| {
                        cheap_component_meta_record(
                            canonical_or_alias,
                            verter_audit::AuditCaptureState::FilteredNoop,
                        )
                    });
                    return verter_audit::AuditedResult::err(
                        ComponentMetaHostError::OutputMaterialization(err),
                        record,
                    );
                }
            };
        match host.take_audit_record(request_id) {
            Some(record) => verter_audit::AuditedResult::ok(Some(output), record),
            None => verter_audit::AuditedResult::err(
                ComponentMetaHostError::AuditRecordMissing { request_id },
                cheap_component_meta_record(
                    canonical_or_alias,
                    verter_audit::AuditCaptureState::FilteredNoop,
                ),
            ),
        }
    }

    /// Get component metadata plus the resolved-state sidecar in this session's
    /// overlay context.
    pub fn get_component_meta_with_resolution(
        &self,
        canonical_or_alias: &str,
    ) -> Result<
        Option<(
            verter_semantic::analysis::component_meta::ComponentMetaAnalysis,
            crate::meta_resolve::ResolvedComponentMetaState,
        )>,
        ComponentMetaHostError,
    > {
        component_meta_trace_custom!(
            "component_meta_session_query_with_resolution",
            canonical_or_alias
        );
        self.inner
            .get_component_meta_with_resolution(canonical_or_alias)
            .map_err(ComponentMetaHostError::from)
    }

    /// Full-meta as encoded payload. Cache-first on the Verter backend.
    pub fn get_component_meta_payload(
        &self,
        canonical_or_alias: &str,
        encode_fn: impl FnOnce(crate::meta_resolve::ComponentMetaOutput) -> Vec<u8>,
    ) -> Result<Option<Vec<u8>>, ComponentMetaHostError> {
        self.inner
            .get_component_meta_payload(canonical_or_alias, encode_fn)
            .map_err(ComponentMetaHostError::from)
    }

    /// Batch surface for [`Self::get_component_meta_payload`]: compute
    /// encoded payloads for `canonicals_or_aliases` under one shared
    /// overlay view. Routes the batch through `HostBatchCoordinator` — on
    /// native it fans the N queries out on the host-owned `HostCpuPool`,
    /// on wasm it runs inline/sequentially — accounting the submission
    /// once per non-empty batch (skipped for an empty batch).
    ///
    /// Returns one slot per input in input order; `None` is reserved
    /// EXCLUSIVELY for a genuinely missing canonical. A real per-id failure
    /// (budget overrun, typed output-materialization failure, per-job
    /// panic) FAILS THE CALL with the first typed error — exactly as the
    /// scalar [`Self::get_component_meta_payload`] does (scalar ≡ batch) —
    /// so a failure is never collapsed onto the missing sentinel.
    pub fn get_component_meta_batch_payloads<F>(
        &self,
        canonical_or_aliases: &[String],
        encode_fn: F,
    ) -> Result<Vec<Option<Vec<u8>>>, ComponentMetaHostError>
    where
        F: Fn(crate::meta_resolve::ComponentMetaOutput) -> Vec<u8> + Sync + Send,
    {
        self.inner
            .get_component_meta_batch_payloads(canonical_or_aliases, encode_fn)
            .map_err(ComponentMetaHostError::from)?
            .into_iter()
            .map(|slot| slot.map_err(ComponentMetaHostError::from))
            .collect()
    }

    /// Tier 1B selective surface API (D32 + D102). Returns the
    /// `ComponentMetaSurface` envelope (eager scalars + `NamedTypeHandle`
    /// for every type-bearing field), or `None` when the canonical does
    /// not resolve to a component.
    pub fn get_component_meta_surface(
        &self,
        canonical_or_alias: &str,
    ) -> Result<Option<crate::component_meta_payload::ComponentMetaSurface>, ComponentMetaHostError>
    {
        self.inner
            .get_component_meta_surface(canonical_or_alias)
            .map_err(ComponentMetaHostError::from)
    }

    /// Tier 1B selective surface API (D32 + D102). Resolves a `TypeHandle`
    /// to a one-layer `TypeExpansion`. Errors are typed (D104 + D114).
    pub fn get_component_meta_type_expansion(
        &self,
        handle: crate::component_meta_payload::TypeHandle,
        depth: Option<usize>,
    ) -> Result<
        crate::component_meta_payload::TypeExpansion,
        crate::component_meta_payload::TypeHandleError,
    > {
        self.inner.get_component_meta_type_expansion(handle, depth)
    }

    /// Get the analysis snapshot in this session's overlay context.
    pub fn get_analysis(
        &self,
        canonical_or_alias: &str,
    ) -> Result<Option<crate::types::FileAnalysisSnapshot>, ComponentMetaHostError> {
        self.inner
            .get_analysis(canonical_or_alias)
            .map_err(ComponentMetaHostError::from)
    }

    /// Get provenance counters for observability.
    pub fn get_provenance(
        &self,
    ) -> Result<crate::types::MetaProvenanceSnapshot, ComponentMetaHostError> {
        self.inner
            .get_provenance()
            .map_err(ComponentMetaHostError::from)
    }

    /// Canonical IDs visible to this session.
    pub fn tracked_file_ids(&self) -> Result<Vec<String>, ComponentMetaHostError> {
        self.inner
            .visible_file_ids()
            .map_err(ComponentMetaHostError::from)
    }

    /// Close the session, releasing its overlays.
    pub fn close(&self) {
        self.inner.close();
    }

    /// Whether this session has been closed.
    pub fn is_closed(&self) -> bool {
        self.inner.is_closed()
    }

    /// Per-session overlay generation counter.
    pub fn overlay_generation(&self) -> u64 {
        self.inner.overlay_generation()
    }
}

/// Build the cheap default-filled [`crate::component_meta_audit::RequestAuditRecord`]
/// the audited component-meta seam returns on its no-record paths
/// (audit disabled, non-fault miss, or evicted record). No per-request
/// counters are collected — the payload is the zero-valued default and
/// `capture_state` records why the full record was unavailable.
fn cheap_component_meta_record(
    canonical_id: &str,
    capture_state: verter_audit::AuditCaptureState,
) -> crate::component_meta_audit::RequestAuditRecord {
    crate::component_meta_audit::RequestAuditRecord {
        request_id: 0,
        canonical_id: canonical_id.to_string(),
        kind: verter_audit::RequestKind::ComponentMeta,
        parent_request_id: verter_scheduler::request_context::current_request_id()
            .map(|id| id.to_string()),
        from_cache: false,
        timings: crate::component_meta_audit::RequestTimingAudit::default(),
        memory: crate::component_meta_audit::RequestMemoryAudit::default(),
        store: crate::component_meta_audit::RequestStoreAudit::default(),
        footprint: None,
        scheduler: None,
        files: Vec::new(),
        waits: None,
        kind_payload: verter_audit::RequestKindPayload::ComponentMeta(
            crate::component_meta_audit::ComponentMetaPayload::default(),
        ),
        capture_state,
        trace_id: String::new(),
    }
}

#[cfg(test)]
fn extract_component_meta_from_resolved_with_evaluated(
    host: &VerterHost,
    ctx: &dyn crate::resolver_core::resolver_context::ResolverContext,
    canonical_id: &str,
    resolved: &crate::meta_resolve::ResolvedComponentMetaState,
    evaluated_types: Option<&ExpandedComponentTypes>,
    include_fallthrough: bool,
) -> ComponentMetaAnalysis {
    // Macro-DTO surface read runs under the request-bound `ctx` (not the
    // bare host) — mirrors the production
    // `extract_component_meta_from_resolved` path.
    let resolved_macros = resolver_component_meta_resolved_macros(
        ctx,
        canonical_id,
        resolved.snapshot.macros.as_ref(),
    );
    let resolved_type_registry =
        resolver_component_meta_type_registry(&resolved.resolved_type_registry);
    let input = verter_semantic::analysis::component_meta::ComponentMetaInput {
        macros: &resolved.snapshot.macros,
        bindings: &resolved.snapshot.bindings,
        imports: &resolved.snapshot.imports,
        template: resolved.snapshot.template.as_deref(),
        options_api: resolved.snapshot.options_api.as_ref(),
        analysis_flags: verter_semantic::analysis::types::AnalysisFlags::from_bits_truncate(
            resolved.snapshot.script_flags,
        ),
        styles: &resolved.snapshot.styles,
        vue_api_calls: &resolved.snapshot.vue_api_calls,
        store_usages: &resolved.snapshot.store_usages,
        resolved_macros: &resolved_macros,
        resolved_type_registry: &resolved_type_registry,
        evaluated_types,
        file_path: canonical_id,
    };

    let mut meta = verter_semantic::analysis::component_meta::extract_component_meta(input);
    if include_fallthrough {
        let mut visiting = rustc_hash::FxHashSet::default();
        if let Some(resolution) = host.compute_fallthrough_surface_from_resolved_state(
            canonical_id,
            resolved,
            None,
            &mut visiting,
            ctx,
        ) {
            meta.accepted_props = resolution.accepted_props;
            meta.accepted_events = resolution.accepted_events;
            meta.accepted_surface_completeness = resolution.accepted_surface_completeness;
            meta.fallthrough_surface = resolution.fallthrough_surface;
        } else {
            meta.accepted_surface_completeness = AcceptedSurfaceCompleteness::LowerBound;
            meta.root_reachability = RootReachability::NoFallthrough {
                reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
            };
            meta.fallthrough_surface = FallthroughSurface::None {
                reason: verter_semantic::analysis::component_meta::NoFallthroughReason::NoTemplate,
            };
        }
    }

    crate::host_manage::populate_public_instance_sidecar(&mut meta);
    crate::host_resolve::populate_sfc_blocks_sidecar(host, canonical_id, &mut meta);
    meta
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
#[path = "component_meta_host_tests.rs"]
mod tests;
