#![deny(missing_docs)]
//! Session-side wiring for the component-meta audit surface.
//!
//! Audit DTOs (record envelope, timing/memory/store data, footprint
//! types, structured events, observer trait) live in
//! [`verter_audit`]. This module hosts the session-only orchestration
//! glue: the per-request [`AuditBuilder`], the
//! [`RequestPhaseAudit`] TLS stack, the structured-trace emit
//! helpers, and the bridges between session-owned domain types
//! (`semantic_query::ProjectionMode`,
//! `verter_workspace::audit_sink::VfsAuditLayer`) and the substrate's
//! audit-side mirrors.
//!
//! In-crate re-exports below preserve the historic
//! `verter_session::component_meta_audit::<Type>` import paths so
//! same-crate callers do not need to retarget every import.

use std::cell::RefCell;
use std::sync::Arc;

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

pub mod accumulator;
pub mod assertions;
pub mod audit_records_store;
#[cfg(test)]
pub(crate) mod expected_display_snapshots;
pub mod footprint_miner;
pub(crate) mod session_vfs_sink;
pub mod structured_event;

#[cfg(test)]
mod mod_tests;

pub use accumulator::{AccumulatorState, RequestFootprintAccumulator};
pub use assertions::{
    render_chain_text, AssertionDiff, ChainTermination, ProvenanceChain, ProvenanceStep,
    WALKER_DEPTH_CAP,
};
pub use audit_records_store::{AuditRecordsStore, AUDIT_RECORDS_STORE_CAPACITY};
pub use footprint_miner::mine_footprint;

// ----------------------------------------------------------------------------
// Re-exports of audit DTOs from the substrate. Preserves the historic
// `verter_session::component_meta_audit::<Type>` import paths for
// same-crate callers; consumers outside this crate import from
// `verter_audit::*` directly.
// ----------------------------------------------------------------------------

pub use verter_audit::footprint::{
    AliasResolveRecord, CacheOutcomeTally, ConditionalRecord, GraphCompletenessReport,
    IndexedReadyBuildRecord, InstantiationRecord, MaterializationRecord, ProjectionRecord,
    RequestFootprintAudit, SharedLoadReuseRecord, SubstitutionRecord, VfsReadRecord,
};
pub use verter_audit::memory::{current_process_rss, RequestMemoryAudit};
pub use verter_audit::observer::{
    current_observer, install_observer, AuditEvent, AuditObserver, ObserverGuard,
};
pub use verter_audit::origin_graph::{
    ConditionalBranch, DerivationEdgeRaw, DerivationEdgeRecord, DerivationSubgraph,
    DispatchKeyKind, EdgeId, MaterializationScopeAudit, MaterializationSubject,
    MaterializeSkipReason, NamedIdentity, NodeId, NodeRecord, NormalizeKind, OriginEdgeKind,
    OriginEdgeMetaDto, ProjectPathSegment, ProjectionModeAudit, SemanticNodeKind, VfsLayer,
};
pub use verter_audit::payloads::cache_outcomes::CacheOutcomeKind;
pub use verter_audit::payloads::ComponentMetaPayload;
pub use verter_audit::record::{IncidentalFields, RequestAuditRecord, RequestPhaseAudit};
pub use verter_audit::store::RequestStoreAudit;
pub use verter_audit::structured_event::StructuredAuditEvent;
pub use verter_audit::timing::RequestTimingAudit;

// In-crate alias so existing TLS-stack call sites continue to read.
pub use verter_audit::record::{Hash16 as AuditHash16, RequestKind, RequestKindPayload};

/// Convert a session-owned [`crate::semantic_query::ProjectionMode`]
/// into the audit-side mirror enum.
///
/// Replaces the `impl From<...> for ProjectionModeAudit` that used
/// to live in this module. Both source and target are now foreign
/// types from the perspective of `verter_session`, so the orphan
/// rule prevents an `impl From`. Producers call this helper
/// explicitly.
#[must_use]
pub fn projection_mode_audit_from(
    mode: crate::semantic_query::ProjectionMode,
) -> ProjectionModeAudit {
    use crate::semantic_query::ProjectionMode;
    match mode {
        ProjectionMode::Identity => ProjectionModeAudit::Identity,
        ProjectionMode::Navigate => ProjectionModeAudit::Navigate,
        ProjectionMode::Shallow => ProjectionModeAudit::Shallow,
        ProjectionMode::Expanded => ProjectionModeAudit::Expanded,
        ProjectionMode::Skeleton => ProjectionModeAudit::Skeleton,
    }
}

/// Convert a workspace-side `VfsAuditLayer` into the audit-side
/// mirror.
///
/// Replaces `impl From<verter_workspace::audit_sink::VfsAuditLayer>
/// for VfsLayer` for the same orphan-rule reason as
/// [`projection_mode_audit_from`].
#[must_use]
pub fn vfs_layer_from_workspace(layer: verter_workspace::audit_sink::VfsAuditLayer) -> VfsLayer {
    use verter_workspace::audit_sink::VfsAuditLayer as W;
    match layer {
        W::Overlay => VfsLayer::Overlay,
        W::Snapshot => VfsLayer::Snapshot,
        W::Disk => VfsLayer::Disk,
        W::DirIndexNegative => VfsLayer::DirIndexNegative,
        W::Missing => VfsLayer::Missing,
    }
}

// ----------------------------------------------------------------------------
// Audit builder — accumulates data during a request
// ----------------------------------------------------------------------------

/// Builder for accumulating audit data during a component-meta
/// request. Created only when `audit_enabled` is true.
pub struct AuditBuilder {
    request_id: u64,
    canonical_id: String,
    request_start: Instant,
    phase_start: Instant,
    timings: RequestTimingAudit,
    store: RequestStoreAudit,
    memory: RequestMemoryAudit,
    footprint: Option<RequestFootprintAudit>,
    component_meta_payload: ComponentMetaPayload,
}

impl AuditBuilder {
    /// Construct a new builder stamped with `request_id` and the
    /// resolved `canonical_id`. Captures the current process RSS for
    /// the memory-delta baseline.
    pub fn new(request_id: u64, canonical_id: String) -> Self {
        let now = Instant::now();
        let rss = current_process_rss();
        Self {
            request_id,
            canonical_id,
            request_start: now,
            phase_start: now,
            timings: RequestTimingAudit::default(),
            store: RequestStoreAudit::default(),
            memory: RequestMemoryAudit {
                process_rss_before_bytes: rss,
                ..Default::default()
            },
            footprint: None,
            component_meta_payload: ComponentMetaPayload::default(),
        }
    }

    /// Mark the end of the current phase and start the next one.
    pub fn end_phase(&mut self, phase: AuditPhase) {
        let elapsed = self.phase_start.elapsed().as_secs_f64() * 1000.0;
        match phase {
            AuditPhase::CaptureInputs => self.timings.capture_inputs_ms = elapsed,
            AuditPhase::StoreRead => self.timings.store_read_ms = elapsed,
            AuditPhase::StoreMerge => self.timings.store_merge_ms = elapsed,
            AuditPhase::DirectImportProof => self.timings.direct_import_proof_ms = elapsed,
            AuditPhase::ImportedRootProof => self.timings.imported_root_proof_ms = elapsed,
            AuditPhase::Solver => self.timings.solver_ms = elapsed,
            AuditPhase::Materialize => self.timings.materialize_ms = elapsed,
            AuditPhase::Serialize => self.timings.serialize_ms = elapsed,
        }
        self.phase_start = Instant::now();
    }

    /// Record `steps` solver resolve-steps and bump the solve-count.
    /// Counters live on the [`ComponentMetaPayload`] (component-meta
    /// is the only request kind that runs the solver).
    pub fn record_solver_steps(&mut self, steps: u64) {
        self.component_meta_payload.total_resolve_steps += steps;
        self.component_meta_payload.solve_count += 1;
    }

    /// Replace the generic store-counter block. Component-meta-specific
    /// counters route through [`Self::record_component_meta_store`].
    pub fn record_store(&mut self, store: RequestStoreAudit) {
        self.store = store;
    }

    /// Replace the component-meta store + materialiser counter
    /// block. These fields live on [`ComponentMetaPayload`] rather
    /// than the generic [`RequestStoreAudit`] envelope because they
    /// are kind-specific (only component-meta requests run the
    /// materialiser).
    pub fn record_component_meta_store(
        &mut self,
        materialize_structure_calls: u64,
        materialize_structure_cache_hits: u64,
        node_arena_lock_acquisitions: u64,
        family_map_lock_acquisitions: u64,
        dep_signature_merges: u64,
        dep_signature_intern_hits: u64,
    ) {
        self.component_meta_payload.materialize_structure_calls = materialize_structure_calls;
        self.component_meta_payload.materialize_structure_cache_hits =
            materialize_structure_cache_hits;
        self.component_meta_payload.node_arena_lock_acquisitions = node_arena_lock_acquisitions;
        self.component_meta_payload.family_map_lock_acquisitions = family_map_lock_acquisitions;
        self.component_meta_payload.dep_signature_merges = dep_signature_merges;
        self.component_meta_payload.dep_signature_intern_hits = dep_signature_intern_hits;
    }

    /// Record host-cache + workspace memory snapshots (before/after).
    pub fn record_memory_snapshots(
        &mut self,
        host_cache_before_bytes: u64,
        host_cache_after_bytes: u64,
        workspace_before_bytes: u64,
        workspace_after_bytes: u64,
    ) {
        self.memory.host_cache_before_bytes = host_cache_before_bytes;
        self.memory.host_cache_after_bytes = host_cache_after_bytes;
        self.memory.workspace_before_bytes = workspace_before_bytes;
        self.memory.workspace_after_bytes = workspace_after_bytes;
    }

    /// Replace the timings block.
    pub fn record_timings(&mut self, timings: RequestTimingAudit) {
        self.timings = timings;
    }

    /// Replace the in-flight component-meta payload wholesale.
    ///
    /// Used at the end of cold-resolver execution by the
    /// host-manage path: the component-meta cache stores a
    /// pre-aggregated [`ComponentMetaPayload`] from the cold
    /// resolution, and the audit-emitting wrapper imports those
    /// counters into the builder before adding any post-cache work.
    pub fn record_component_meta_payload(&mut self, payload: ComponentMetaPayload) {
        self.component_meta_payload = payload;
    }

    /// Borrow the in-flight component-meta payload mutably so callers
    /// outside this module can update individual counters before
    /// finalising the record.
    pub fn component_meta_payload_mut(&mut self) -> &mut ComponentMetaPayload {
        &mut self.component_meta_payload
    }

    /// Borrow the in-flight component-meta payload immutably.
    #[must_use]
    pub fn component_meta_payload(&self) -> &ComponentMetaPayload {
        &self.component_meta_payload
    }

    /// Attach a fully-mined semantic footprint to this builder.
    pub fn record_footprint(&mut self, footprint: RequestFootprintAudit) {
        self.footprint = Some(footprint);
    }

    /// Finalize the builder into a [`RequestAuditRecord`] — captures
    /// the request-end RSS, computes the signed delta, and fills the
    /// `total_ms` wall-clock.
    pub fn finish(mut self) -> RequestAuditRecord {
        self.timings.total_ms = self.request_start.elapsed().as_secs_f64() * 1000.0;
        self.memory.process_rss_after_bytes = current_process_rss();
        self.memory.process_rss_delta_bytes = self.memory.process_rss_after_bytes as i64
            - self.memory.process_rss_before_bytes as i64;

        RequestAuditRecord {
            request_id: self.request_id,
            canonical_id: self.canonical_id,
            kind: RequestKind::ComponentMeta,
            parent_request_id: None,
            from_cache: false,
            timings: self.timings,
            memory: self.memory,
            store: self.store,
            footprint: self.footprint,
            kind_payload: RequestKindPayload::ComponentMeta(self.component_meta_payload),
        }
    }
}

/// Named phases for timing capture.
#[derive(Debug, Clone, Copy)]
pub enum AuditPhase {
    /// Capture inputs (request args, config snapshot).
    CaptureInputs,
    /// Read the project type store.
    StoreRead,
    /// Merge store data with the overlay view.
    StoreMerge,
    /// Prove direct imports.
    DirectImportProof,
    /// Prove transitively-imported type roots.
    ImportedRootProof,
    /// Type solver invocation.
    Solver,
    /// Member-route / public-type materialization.
    Materialize,
    /// Serialize the final component-meta payload.
    Serialize,
}

thread_local! {
    static ACTIVE_REQUEST_AUDIT: RefCell<Vec<(u64, RequestPhaseAudit)>> =
        const { RefCell::new(Vec::new()) };
}

/// RAII guard returned by [`begin_request_audit`]. Drops the
/// corresponding entry from the `ACTIVE_REQUEST_AUDIT` TLS stack on
/// scope exit.
pub struct RequestAuditGuard {
    request_id: u64,
}

impl RequestAuditGuard {
    /// Snapshot the phase-audit state for this request without
    /// removing it from the stack.
    pub fn snapshot(&self) -> RequestPhaseAudit {
        ACTIVE_REQUEST_AUDIT.with(|stack| {
            stack
                .borrow()
                .iter()
                .rev()
                .find(|(request_id, _)| *request_id == self.request_id)
                .map(|(_, audit)| audit.clone())
                .unwrap_or_default()
        })
    }
}

impl Drop for RequestAuditGuard {
    fn drop(&mut self) {
        ACTIVE_REQUEST_AUDIT.with(|stack| {
            let mut stack = stack.borrow_mut();
            if let Some(position) = stack
                .iter()
                .rposition(|(request_id, _)| *request_id == self.request_id)
            {
                stack.remove(position);
            }
        });
    }
}

/// Push a new [`RequestPhaseAudit`] entry for `request_id` onto the
/// TLS stack and return the RAII guard that removes it on drop.
pub fn begin_request_audit(request_id: u64) -> RequestAuditGuard {
    ACTIVE_REQUEST_AUDIT.with(|stack| {
        stack
            .borrow_mut()
            .push((request_id, RequestPhaseAudit::default()));
    });
    RequestAuditGuard { request_id }
}

/// Accumulate `elapsed_ms` into the current request's imported-root
/// proof phase. Zero or negative values are dropped (defensive guard
/// against timer skew).
pub fn record_imported_root_proof_ms(elapsed_ms: f64) {
    if elapsed_ms <= 0.0 {
        return;
    }
    ACTIVE_REQUEST_AUDIT.with(|stack| {
        if let Some((_, audit)) = stack.borrow_mut().last_mut() {
            audit.imported_root_proof_ms += elapsed_ms;
        }
    });
}

/// Snapshot the top-of-stack request's phase-audit without removing
/// it. Used by consumers that need a sidecar view without owning a
/// guard.
pub fn current_request_audit_snapshot() -> RequestPhaseAudit {
    ACTIVE_REQUEST_AUDIT.with(|stack| {
        stack
            .borrow()
            .last()
            .map(|(_, audit)| audit.clone())
            .unwrap_or_default()
    })
}

// ----------------------------------------------------------------------------
// Trace emission
// ----------------------------------------------------------------------------

/// Emit an audit record via the component-meta trace system and, when
/// `VERTER_COMPONENT_META_AUDIT_JSON_OUT` is set, also serialise the
/// record to the named path.
pub fn emit_audit_trace(record: &RequestAuditRecord) {
    let cm = record.component_meta_payload().cloned().unwrap_or_default();
    let detail = format!(
        "request_id={} canonical={} total_ms={:.2} solver_ms={:.2} solver_steps={} solve_count={} \
         capture_inputs_ms={:.2} store_read_ms={:.2} store_merge_ms={:.2} \
         direct_import_proof_ms={:.2} imported_root_proof_ms={:.2} \
         materialize_ms={:.2} serialize_ms={:.2} \
         rss_before={}B rss_after={}B rss_delta={}B \
         host_cache_before={}B host_cache_after={}B \
         workspace_before={}B workspace_after={}B \
         store_view_hits={} store_view_misses={} structural_merges={} \
         imported_dep_entries={} imported_dep_bytes={} prepared_type_decls={} prepared_value_decls={} \
         footprint_present={}",
        record.request_id,
        record.canonical_id,
        record.timings.total_ms,
        record.timings.solver_ms,
        cm.total_resolve_steps,
        cm.solve_count,
        record.timings.capture_inputs_ms,
        record.timings.store_read_ms,
        record.timings.store_merge_ms,
        record.timings.direct_import_proof_ms,
        record.timings.imported_root_proof_ms,
        record.timings.materialize_ms,
        record.timings.serialize_ms,
        record.memory.process_rss_before_bytes,
        record.memory.process_rss_after_bytes,
        record.memory.process_rss_delta_bytes,
        record.memory.host_cache_before_bytes,
        record.memory.host_cache_after_bytes,
        record.memory.workspace_before_bytes,
        record.memory.workspace_after_bytes,
        record.store.store_view_hits,
        record.store.store_view_misses,
        record.store.structural_merges,
        record.store.imported_dependency_entries,
        record.store.imported_dependency_bytes,
        record.store.prepared_type_decls,
        record.store.prepared_value_decls,
        record.footprint.is_some(),
    );
    eprintln!("[verter-rust-audit] {detail}");

    if let Ok(path) = std::env::var("VERTER_COMPONENT_META_AUDIT_JSON_OUT") {
        if !path.is_empty() {
            if let Ok(serialized) = serde_json::to_string_pretty(record) {
                let _ = std::fs::write(&path, serialized);
            }
        }
    }
}

/// Serialise an audit record to JSON.
pub fn emit_json(record: &RequestAuditRecord) -> String {
    serde_json::to_string(record).unwrap_or_default()
}

/// Merge the `dep_signature` entries from a `CacheRead` (or any
/// `&[(Arc<str>, DepVersion)]` slice) into the materialiser's
/// per-frame `local_fence` while recording audit counters in lock
/// step.
pub fn merge_dep_signature_into_local_fence(
    local_fence: &mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
    incoming: &[(Arc<str>, crate::semantic_query::DepVersion)],
) {
    crate::host_manage::record_dep_signature_merge();
    let pre_existing_count = local_fence.len();
    for entry in incoming {
        let is_hit = local_fence
            .iter()
            .take(pre_existing_count)
            .any(|existing| Arc::ptr_eq(&existing.0, &entry.0) && existing.1 == entry.1);
        if is_hit {
            crate::host_manage::record_dep_signature_intern_hit();
        }
        local_fence.push(entry.clone());
    }
}

/// Record a fresh [`IndexedReady`](crate::project_type_store::IndexedReady)
/// insertion in the active request's accumulator. Pushes both a
/// typed [`IndexedReadyBuildRecord`] (direct lane used by the miner
/// on the happy path) and the equivalent [`StructuredAuditEvent`]
/// (fallback lane when the direct records vec is empty). No-op when
/// no request context is installed.
pub fn record_indexed_ready_built(canonical_id: Arc<str>, whole_hash: crate::types::Hash16) {
    if let Some(acc) = crate::request_context::current_accumulator() {
        acc.push_indexed_ready_build(IndexedReadyBuildRecord {
            canonical_id: Arc::clone(&canonical_id),
            whole_hash,
        });
        acc.push_structured_event(StructuredAuditEvent::IndexedReadyBuilt {
            canonical_id,
            whole_hash,
        });
    }
}

// ----------------------------------------------------------------------------
// Stable display key for SemanticNodeId
// ----------------------------------------------------------------------------

/// Produce a deterministic, human-readable key for a
/// [`SemanticNodeId`](crate::semantic_query::SemanticNodeId)
/// suitable for audit trace output and
/// `MaterializationSubject::Structure.node_key` field.
///
/// The key is deterministic under one project generation: identical
/// `(graph, id)` pairs produce identical strings. Returns
/// `<unknown:{id}>` when the id has not been interned in `graph`
/// (defensive: an audit lookup must not panic on a stale id from a
/// prior generation).
#[must_use]
pub fn audit_key_for_node(
    graph: &crate::semantic_query_memo::SemanticGraphStore,
    id: crate::semantic_query::SemanticNodeId,
) -> Arc<str> {
    use crate::semantic_query::{IndexKey, LiteralValue, SemanticNodeData};
    let Some(data) = graph.node_data(id) else {
        return Arc::from(format!("<unknown:{}>", id.0));
    };
    let label = match data.as_ref() {
        SemanticNodeData::Alias(inner) => format!("Alias({})", inner.0),
        SemanticNodeData::Object(_) => format!("Object#{}", id.0),
        SemanticNodeData::Union(arms) => format!("Union[{}]", arms.len()),
        SemanticNodeData::Intersection(arms) => format!("Intersection[{}]", arms.len()),
        SemanticNodeData::Primitive(p) => format!("Primitive({p:?})"),
        SemanticNodeData::Literal(LiteralValue::String(s)) => format!("Literal(\"{s}\")"),
        SemanticNodeData::Literal(other) => format!("Literal({other:?})"),
        SemanticNodeData::Opaque(_) => format!("Opaque#{}", id.0),
        SemanticNodeData::Array { element, readonly } => {
            format!("Array{{element={},readonly={}}}", element.0, readonly)
        }
        SemanticNodeData::Tuple { elements, readonly } => {
            format!("Tuple[{},readonly={}]", elements.len(), readonly)
        }
        SemanticNodeData::TemplateLiteral {
            quasis,
            expressions,
        } => format!("TemplateLiteral[{}q,{}e]", quasis.len(), expressions.len()),
        SemanticNodeData::KeyOf { base } => format!("KeyOf({})", base.0),
        SemanticNodeData::IndexedAccess { object, index } => match index {
            IndexKey::String(s) => format!("IndexedAccess({}[\"{}\"])", object.0, s),
            IndexKey::Number(n) => format!("IndexedAccess({}[{}])", object.0, n),
            IndexKey::TypeNode(n) => format!("IndexedAccess({}[<type:{}>])", object.0, n.0),
        },
        SemanticNodeData::Mapped { source, .. } => format!("Mapped(source={})", source.0),
        SemanticNodeData::TypeOf { value_root, path } => format!(
            "TypeOf({}::{},path[{}])",
            value_root.scope.canonical_id,
            value_root.name,
            path.len()
        ),
        SemanticNodeData::TypeParam {
            decl,
            display_name,
            param_index,
            ..
        } => format!(
            "TypeParam({}::{}#{})",
            decl.canonical_id, display_name, param_index
        ),
        SemanticNodeData::Infer { name } => format!("Infer({name})"),
        SemanticNodeData::Conditional { distributive, .. } => {
            format!("Conditional(distributive={distributive})")
        }
        SemanticNodeData::VueMacroElements(_) => format!("VueMacroElements#{}", id.0),
        SemanticNodeData::Function { params, .. } => format!("Function[{}p]", params.len()),
        SemanticNodeData::DeclRef { identity } => {
            format!("DeclRef({}::{})", identity.canonical_id, identity.decl_name)
        }
        SemanticNodeData::InstantiationRef { base, args } => format!(
            "InstantiationRef({}::{}[{}])",
            base.canonical_id,
            base.decl_name,
            args.len()
        ),
    };
    Arc::from(label)
}
