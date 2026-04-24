//! Rust-first native audit surface for component-meta requests.
//!
//! Gated by `HostConfig::audit_enabled`. When off, the runtime stays on the
//! zero-overhead default path. When on, timing/memory/store snapshots are
//! captured per request and emitted as a structured `RustAuditRecord`.
//!
//! When `HostConfig::footprint_capture` is additionally true, the request
//! attaches a `RustSemanticFootprintAudit` derived from a per-request
//! accumulator populated by the `record_origin_edge` hook, the VFS audit
//! sink, and the structured-event trace macro.
//!
//! This module owns the canonical audit record types. JS benchmark/harness
//! audit is a separate concern — it does not inline or redefine these types.
//!
//! TS bindings for every record type are generated via `ts-rs`. The output
//! file lives at `packages/types/audit.generated.ts`. The
//! `ts_bindings_export` and `audit_ts_bindings_are_in_sync` tests keep the
//! committed TS in lock-step with the Rust source.

use std::cell::RefCell;
use std::sync::Arc;
use std::time::Instant;

use serde::{Deserialize, Serialize};

pub mod accumulator;
pub mod assertions;
pub mod audit_records_store;
#[cfg(test)]
pub(crate) mod expected_display_snapshots;
pub mod footprint_miner;
pub(crate) mod session_vfs_sink;
pub mod structured_event;

pub use accumulator::{AccumulatorState, DerivationEdgeRaw, RequestFootprintAccumulator};
pub use assertions::{
    render_chain_text, AssertionDiff, ChainTermination, ProvenanceChain, ProvenanceStep,
    WALKER_DEPTH_CAP,
};
pub use audit_records_store::{AuditRecordsStore, AUDIT_RECORDS_STORE_CAPACITY};
pub use footprint_miner::mine_footprint;
pub use structured_event::StructuredComponentMetaEvent;

use crate::types::Hash16;

// ---------------------------------------------------------------------------
// Audit record types — core scalars
// ---------------------------------------------------------------------------

/// Top-level audit record for one component-meta request.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RustAuditRecord {
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub request_id: u64,
    pub canonical_id: String,
    pub timings: RustTimingAudit,
    pub solver: RustSolverAudit,
    pub store: RustStoreAudit,
    pub memory: RustMemoryAudit,
    /// Optional semantic footprint. Populated when
    /// `HostConfig::footprint_capture` is true and the accumulator
    /// collected work for this request.
    pub footprint: Option<RustSemanticFootprintAudit>,
}

/// Phase timings in milliseconds.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RustTimingAudit {
    pub total_ms: f64,
    pub capture_inputs_ms: f64,
    pub store_read_ms: f64,
    pub store_merge_ms: f64,
    pub direct_import_proof_ms: f64,
    pub imported_root_proof_ms: f64,
    pub solver_ms: f64,
    pub materialize_ms: f64,
    pub serialize_ms: f64,
}

/// Solver-level counters from `SolverResult.steps`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RustSolverAudit {
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub total_resolve_steps: u64,
    pub solve_count: u32,
}

/// Store/view counters.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RustStoreAudit {
    pub store_view_hits: u32,
    pub store_view_misses: u32,
    pub structural_merges: u32,
    pub imported_dependency_entries: u32,
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub imported_dependency_bytes: u64,
    pub prepared_type_decls: u32,
    pub prepared_value_decls: u32,
}

/// Memory snapshots.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RustMemoryAudit {
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub process_rss_before_bytes: u64,
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub process_rss_after_bytes: u64,
    // `process_rss_delta_bytes` is i64 (signed) — it is NOT covered by
    // the u64-as-string transport rule (plan §1.4) because JS's
    // `Number.MIN_SAFE_INTEGER`/`MAX_SAFE_INTEGER` gives ±2^53 of
    // headroom on either side, which is ample for RSS deltas.
    pub process_rss_delta_bytes: i64,
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub host_cache_before_bytes: u64,
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub host_cache_after_bytes: u64,
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub workspace_before_bytes: u64,
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub workspace_after_bytes: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RequestPhaseAudit {
    pub imported_root_proof_ms: f64,
}

// ---------------------------------------------------------------------------
// Semantic footprint — plan §2.1
// ---------------------------------------------------------------------------

/// Semantic footprint attached to an audited request. Populated by the
/// footprint miner (Commit 4) from the accumulator's raw events.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RustSemanticFootprintAudit {
    pub indexed_ready_builds: Vec<IndexedReadyBuildRecord>,
    pub vfs_reads: Vec<VfsReadRecord>,
    pub shared_load_reuses: Vec<SharedLoadReuseRecord>,
    pub instantiations: Vec<InstantiationRecord>,
    pub projections: Vec<ProjectionRecord>,
    pub conditional_decisions: Vec<ConditionalRecord>,
    pub substitutions: Vec<SubstitutionRecord>,
    pub alias_resolutions: Vec<AliasResolveRecord>,
    pub materializations: Vec<MaterializationRecord>,
    pub cache_outcomes: CacheOutcomeTally,
    pub graph_completeness: GraphCompletenessReport,
    pub derivation_subgraph: DerivationSubgraph,
}

impl RustSemanticFootprintAudit {
    /// Union of `vfs_reads[*].canonical_id` and `shared_load_reuses[*].canonical_id`,
    /// deduplicated and sorted.
    #[must_use]
    pub fn loaded_files(&self) -> Vec<Arc<str>> {
        let mut out =
            Vec::<Arc<str>>::with_capacity(self.vfs_reads.len() + self.shared_load_reuses.len());
        for r in &self.vfs_reads {
            out.push(Arc::clone(&r.canonical_id));
        }
        for r in &self.shared_load_reuses {
            out.push(Arc::clone(&r.canonical_id));
        }
        out.sort_by(|a, b| a.as_ref().cmp(b.as_ref()));
        out.dedup_by(|a, b| a.as_ref() == b.as_ref());
        out
    }

    /// Produce a new footprint with "incidental" events stripped — the
    /// Commit 6 assertion harness uses this to turn flaky snapshots into
    /// stable ones. Today the function clones a subset that excludes
    /// VFS reads (purely incidental — cache warmth doesn't change the
    /// semantic footprint). Future work may expand the mask list; the
    /// assertion tests pin the current set.
    #[must_use]
    pub fn mask_incidental_spans(&self) -> RustSemanticFootprintAudit {
        let mut out = self.clone();
        out.vfs_reads.clear();
        out
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct IndexedReadyBuildRecord {
    pub canonical_id: Arc<str>,
    pub whole_hash: Hash16,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct VfsReadRecord {
    pub canonical_id: Arc<str>,
    pub layer: VfsLayer,
    pub cache_hit: bool,
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub bytes_read: u64,
    /// Request-id the sink routed this event to — plan §3.A Commit 6.D.
    /// Session-side [`SessionVfsSink`] only pushes events whose
    /// [`verter_workspace::audit_sink::VfsReadEvent::request_id`]
    /// matches the request this sink was registered for, so this
    /// field mirrors that filter decision for consumers who want to
    /// sanity-check audit ownership.
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub request_id: u64,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct SharedLoadReuseRecord {
    pub canonical_id: Arc<str>,
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub winner_request_id: u64,
    pub winner_audited: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct InstantiationRecord {
    pub result: NodeId,
    pub decl_canonical_id: Arc<str>,
    pub decl_symbol_name: Arc<str>,
    pub args_fingerprint: Hash16,
    pub args: Vec<NodeId>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct ProjectionRecord {
    pub result: NodeId,
    pub base: NodeId,
    pub path: Vec<ProjectPathSegment>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct ConditionalRecord {
    pub result: NodeId,
    pub branch: ConditionalBranch,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct SubstitutionRecord {
    pub result: NodeId,
    pub param_name: Arc<str>,
    pub substituted_with: NodeId,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct AliasResolveRecord {
    pub result: NodeId,
    pub alias_name: Arc<str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct MaterializationRecord {
    pub subject: MaterializationSubject,
    pub duration_ms: f64,
}

/// Per-context cache-event tally. No `is_approximate` field — values
/// are EXACT per-request because they come from the request's own
/// atomic counters (plan §1.4).
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct CacheOutcomeTally {
    pub cold_builds: u32,
    pub warm_hits: u32,
    pub joined_waits: u32,
    pub sentinels: u32,
    pub inflight_aborted_retries: u32,
    pub cold_aborts_swept: u32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct GraphCompletenessReport {
    /// Set when the miner truncated derivation edges at
    /// `HostConfig::max_derivation_edges`. Walker invocations that
    /// cross the truncation boundary report truncated ancestry.
    pub has_orphan_edges: bool,
    pub edges_truncated: u32,
}

/// In-audit opaque NodeId. Assigned by the miner from a sorted
/// canonicalisation of touched `SemanticNodeId`s so identical
/// requests produce identical serialised footprints regardless of
/// thread interleaving.
#[derive(
    Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct NodeId(pub u32);

#[derive(
    Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct EdgeId(pub u32);

#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct DerivationSubgraph {
    /// Sorted by `(kind, structural_hash, named_identity)`; `NodeId` is
    /// the index in this sorted list.
    pub nodes: Vec<NodeRecord>,
    /// Sorted by `(result, kind, sources)`; `EdgeId` is the index in
    /// this sorted list.
    pub edges: Vec<DerivationEdgeRecord>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct NodeRecord {
    pub kind: SemanticNodeKind,
    pub named_identity: Option<NamedIdentity>,
    /// Content-deterministic hash distinguishing anonymous nodes.
    /// Commit 4 computes this from the semantic graph's node data.
    pub structural_hash: Hash16,
    pub display_label: Arc<str>,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct NamedIdentity {
    pub canonical_id: Arc<str>,
    pub symbol_name: Arc<str>,
    pub args_fingerprint: Hash16,
}

/// `#[non_exhaustive]` + `Other` catchall future-proofs against new
/// `SemanticNodeData` variants landing after Commit 3 without breaking
/// the audit.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
#[non_exhaustive]
pub enum SemanticNodeKind {
    DeclAnchor,
    Instantiated,
    Alias,
    Conditional,
    Union,
    Intersection,
    Tuple,
    Object,
    Array,
    Primitive,
    TypeParam,
    Opaque,
    IndexedAccess,
    KeyOf,
    TypeOf,
    Mapped,
    TemplateLiteral,
    NormalizeUnion,
    NormalizeIntersection,
    Other { name: Arc<str> },
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct DerivationEdgeRecord {
    pub result: NodeId,
    pub kind: OriginEdgeKind,
    pub sources: Vec<NodeId>,
    pub meta: OriginEdgeMetaDto,
}

/// Audit-side origin edge kind. Mirrors the semantic graph's
/// `verter_session::semantic_query::OriginEdgeKind` (nine kinds) and
/// adds `SharedLoadReuse` — an audit-only edge emitted when a joiner
/// attaches to a winner's in-flight artifact (plan §1.4).
#[derive(
    Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum OriginEdgeKind {
    Instantiate,
    SubstituteTypeParam,
    ConditionalSelect,
    InferBind,
    ProjectMember,
    ProjectIndex,
    ProjectPath,
    Normalize,
    AliasResolve,
    SharedLoadReuse,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum OriginEdgeMetaDto {
    Instantiate {
        type_params: Vec<Arc<str>>,
    },
    SubstituteTypeParam {
        param_name: Arc<str>,
        substituted_with: NodeId,
    },
    ConditionalSelect {
        branch: ConditionalBranch,
    },
    InferBind {
        param_name: Arc<str>,
        bound_to: NodeId,
    },
    ProjectMember {
        member_name: Arc<str>,
    },
    ProjectIndex {
        index_key: Arc<str>,
    },
    ProjectPath {
        path: Vec<ProjectPathSegment>,
    },
    Normalize {
        kind: NormalizeKind,
    },
    AliasResolve {
        alias_name: Arc<str>,
    },
    SharedLoadReuse {
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        winner_request_id: u64,
        winner_audited: bool,
    },
}

#[derive(
    Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum ConditionalBranch {
    True,
    False,
    Deferred,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum ProjectPathSegment {
    Member { name: Arc<str> },
    Index { key: Arc<str> },
    KeyOf,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum NormalizeKind {
    Union,
    Intersection,
    Simplify,
}

#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum MaterializationSubject {
    MemberRoute { owner: Arc<str>, member: Arc<str> },
    PublicPropType { owner: Arc<str>, prop: Arc<str> },
    DefinePropsMember { owner: Arc<str>, member: Arc<str> },
    FallthroughInheritance { owner: Arc<str> },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum DispatchKeyKind {
    ResolveDecl,
    Instantiate,
    ProjectMember,
    ProjectIndex,
    ProjectPath,
    Normalize,
    ResolvedNamedType,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum CacheOutcomeKind {
    Hit,
    Miss,
    JoinedWait,
    Sentinel,
    ColdBuild,
    InflightAbortedRetry,
    ColdAbortSwept,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum VfsLayer {
    /// Overlay (active editor buffer).
    Overlay,
    /// Snapshot cache hit.
    Snapshot,
    /// Disk read.
    Disk,
    /// Directory index returned a negative (file known not to exist).
    /// Session-side audit mirrors
    /// [`verter_workspace::audit_sink::VfsAuditLayer::DirIndexNegative`].
    DirIndexNegative,
    /// Read missed every layer — the file was not found.
    Missing,
}

impl From<verter_workspace::audit_sink::VfsAuditLayer> for VfsLayer {
    fn from(layer: verter_workspace::audit_sink::VfsAuditLayer) -> Self {
        use verter_workspace::audit_sink::VfsAuditLayer as W;
        match layer {
            W::Overlay => VfsLayer::Overlay,
            W::Snapshot => VfsLayer::Snapshot,
            W::Disk => VfsLayer::Disk,
            W::DirIndexNegative => VfsLayer::DirIndexNegative,
            W::Missing => VfsLayer::Missing,
        }
    }
}

// ---------------------------------------------------------------------------
// Audit builder — accumulates data during a request
// ---------------------------------------------------------------------------

/// Builder for accumulating audit data during a component-meta request.
/// Created only when `audit_enabled` is true.
pub struct AuditBuilder {
    request_id: u64,
    canonical_id: String,
    request_start: Instant,
    phase_start: Instant,
    timings: RustTimingAudit,
    solver: RustSolverAudit,
    store: RustStoreAudit,
    memory: RustMemoryAudit,
    footprint: Option<RustSemanticFootprintAudit>,
}

impl AuditBuilder {
    pub fn new(request_id: u64, canonical_id: String) -> Self {
        let now = Instant::now();
        let rss = current_process_rss();
        Self {
            request_id,
            canonical_id,
            request_start: now,
            phase_start: now,
            timings: RustTimingAudit::default(),
            solver: RustSolverAudit::default(),
            store: RustStoreAudit::default(),
            memory: RustMemoryAudit {
                process_rss_before_bytes: rss,
                ..Default::default()
            },
            footprint: None,
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

    pub fn record_solver_steps(&mut self, steps: u64) {
        self.solver.total_resolve_steps += steps;
        self.solver.solve_count += 1;
    }

    pub fn record_store(&mut self, store: RustStoreAudit) {
        self.store = store;
    }

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

    pub fn record_timings(&mut self, timings: RustTimingAudit) {
        self.timings = timings;
    }

    pub fn record_solver(&mut self, solver: RustSolverAudit) {
        self.solver = solver;
    }

    /// Attach a fully-mined semantic footprint to this builder.
    pub fn record_footprint(&mut self, footprint: RustSemanticFootprintAudit) {
        self.footprint = Some(footprint);
    }

    pub fn finish(mut self) -> RustAuditRecord {
        self.timings.total_ms = self.request_start.elapsed().as_secs_f64() * 1000.0;
        self.memory.process_rss_after_bytes = current_process_rss();
        self.memory.process_rss_delta_bytes = self.memory.process_rss_after_bytes as i64
            - self.memory.process_rss_before_bytes as i64;

        RustAuditRecord {
            request_id: self.request_id,
            canonical_id: self.canonical_id,
            timings: self.timings,
            solver: self.solver,
            store: self.store,
            memory: self.memory,
            footprint: self.footprint,
        }
    }
}

/// Named phases for timing capture.
#[derive(Debug, Clone, Copy)]
pub enum AuditPhase {
    CaptureInputs,
    StoreRead,
    StoreMerge,
    DirectImportProof,
    ImportedRootProof,
    Solver,
    Materialize,
    Serialize,
}

thread_local! {
    static ACTIVE_REQUEST_AUDIT: RefCell<Vec<(u64, RequestPhaseAudit)>> =
        const { RefCell::new(Vec::new()) };
}

pub struct RequestAuditGuard {
    request_id: u64,
}

impl RequestAuditGuard {
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

pub fn begin_request_audit(request_id: u64) -> RequestAuditGuard {
    ACTIVE_REQUEST_AUDIT.with(|stack| {
        stack
            .borrow_mut()
            .push((request_id, RequestPhaseAudit::default()));
    });
    RequestAuditGuard { request_id }
}

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

pub fn current_request_audit_snapshot() -> RequestPhaseAudit {
    ACTIVE_REQUEST_AUDIT.with(|stack| {
        stack
            .borrow()
            .last()
            .map(|(_, audit)| audit.clone())
            .unwrap_or_default()
    })
}

// ---------------------------------------------------------------------------
// Process memory snapshot
// ---------------------------------------------------------------------------

/// Get current process RSS in bytes. Returns 0 if unavailable.
fn current_process_rss() -> u64 {
    #[cfg(target_os = "linux")]
    {
        if let Ok(statm) = std::fs::read_to_string("/proc/self/statm") {
            if let Some(rss_pages) = statm.split_whitespace().nth(1) {
                if let Ok(pages) = rss_pages.parse::<u64>() {
                    return pages * 4096;
                }
            }
        }
    }
    #[cfg(target_os = "macos")]
    {
        let mut usage = std::mem::MaybeUninit::<libc_rusage>::uninit();
        // SAFETY: getrusage is a POSIX function that fills the provided struct.
        let ret = unsafe {
            getrusage(0 /* RUSAGE_SELF */, usage.as_mut_ptr())
        };
        if ret == 0 {
            let usage = unsafe { usage.assume_init() };
            return usage.ru_maxrss as u64;
        }
    }
    0
}

#[cfg(target_os = "macos")]
#[repr(C)]
#[allow(non_camel_case_types)]
struct libc_rusage {
    ru_utime: [i64; 2],
    ru_stime: [i64; 2],
    ru_maxrss: i64,
    _pad: [i64; 13],
}

#[cfg(target_os = "macos")]
unsafe extern "C" {
    fn getrusage(who: i32, usage: *mut libc_rusage) -> i32;
}

// ---------------------------------------------------------------------------
// Trace emission
// ---------------------------------------------------------------------------

/// Emit an audit record via the component-meta trace system and, when
/// `VERTER_COMPONENT_META_AUDIT_JSON_OUT` is set, also serialise the
/// record to the named path.
pub fn emit_audit_trace(record: &RustAuditRecord) {
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
        record.solver.total_resolve_steps,
        record.solver.solve_count,
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

/// Serialise an audit record to JSON (plan §1.4 debug flow).
pub fn emit_json(record: &RustAuditRecord) -> String {
    serde_json::to_string(record).unwrap_or_default()
}

/// Record a fresh [`IndexedReady`](crate::project_type_store::IndexedReady)
/// insertion in the active request's accumulator. Pushes both a
/// typed `IndexedReadyBuildRecord` (direct lane used by the miner
/// on the happy path) and the equivalent `StructuredComponentMetaEvent`
/// (fallback lane when the direct records vec is empty — plan
/// §3 Commit 5 fallback semantics). No-op when no request context is
/// installed. Plan §3 Commit 5 / §3.A Commit 6.E.
pub fn record_indexed_ready_built(canonical_id: Arc<str>, whole_hash: Hash16) {
    if let Some(acc) = crate::request_context::current_accumulator() {
        acc.push_indexed_ready_build(IndexedReadyBuildRecord {
            canonical_id: Arc::clone(&canonical_id),
            whole_hash,
        });
        acc.push_structured_event(StructuredComponentMetaEvent::IndexedReadyBuilt {
            canonical_id,
            whole_hash,
        });
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn audit_builder_captures_total_timing() {
        let builder = AuditBuilder::new(1, "test.vue".into());
        std::thread::sleep(std::time::Duration::from_millis(5));
        let record = builder.finish();
        assert!(record.timings.total_ms >= 4.0);
        assert_eq!(record.request_id, 1);
        assert_eq!(record.canonical_id, "test.vue");
        assert!(record.footprint.is_none());
    }

    #[test]
    fn audit_builder_records_solver_steps() {
        let mut builder = AuditBuilder::new(2, "component.vue".into());
        builder.record_solver_steps(42);
        builder.record_solver_steps(100);
        let record = builder.finish();
        assert_eq!(record.solver.total_resolve_steps, 142);
        assert_eq!(record.solver.solve_count, 2);
    }

    #[test]
    fn audit_builder_captures_phase_timings() {
        let mut builder = AuditBuilder::new(3, "phased.vue".into());
        std::thread::sleep(std::time::Duration::from_millis(2));
        builder.end_phase(AuditPhase::CaptureInputs);
        std::thread::sleep(std::time::Duration::from_millis(2));
        builder.end_phase(AuditPhase::Solver);
        let record = builder.finish();
        assert!(record.timings.capture_inputs_ms >= 1.0);
        assert!(record.timings.solver_ms >= 1.0);
        assert_eq!(record.timings.store_read_ms, 0.0);
    }

    #[test]
    fn audit_default_host_config_is_off() {
        let config = crate::HostConfig::default();
        assert!(!config.audit_enabled);
    }

    #[test]
    fn hash16_available_in_component_meta_audit_via_crate_types() {
        let h: Hash16 = [7u8; 16];
        let record = IndexedReadyBuildRecord {
            canonical_id: Arc::from("/a.ts"),
            whole_hash: h,
        };
        assert_eq!(record.whole_hash[0], 7);
    }

    #[test]
    fn derivation_subgraph_serde_round_trips_nodes_and_edges_preserving_node_ids() {
        let graph = DerivationSubgraph {
            nodes: vec![
                NodeRecord {
                    kind: SemanticNodeKind::DeclAnchor,
                    named_identity: Some(NamedIdentity {
                        canonical_id: Arc::from("/x.ts"),
                        symbol_name: Arc::from("Foo"),
                        args_fingerprint: [0u8; 16],
                    }),
                    structural_hash: [1u8; 16],
                    display_label: Arc::from("Foo"),
                },
                NodeRecord {
                    kind: SemanticNodeKind::Primitive,
                    named_identity: None,
                    structural_hash: [2u8; 16],
                    display_label: Arc::from("string"),
                },
            ],
            edges: vec![DerivationEdgeRecord {
                result: NodeId(1),
                kind: OriginEdgeKind::ProjectMember,
                sources: vec![NodeId(0)],
                meta: OriginEdgeMetaDto::ProjectMember {
                    member_name: Arc::from("foo"),
                },
            }],
        };
        let json = serde_json::to_string(&graph).unwrap();
        let back: DerivationSubgraph = serde_json::from_str(&json).unwrap();
        assert_eq!(back.nodes.len(), 2);
        assert_eq!(back.edges.len(), 1);
        assert_eq!(back.edges[0].result, NodeId(1));
        assert_eq!(back.edges[0].sources[0], NodeId(0));
    }

    #[test]
    fn node_id_stable_within_footprint_across_serialization_roundtrip() {
        let fp = RustSemanticFootprintAudit {
            derivation_subgraph: DerivationSubgraph {
                nodes: vec![NodeRecord {
                    kind: SemanticNodeKind::Alias,
                    named_identity: None,
                    structural_hash: [3u8; 16],
                    display_label: Arc::from("Alias"),
                }],
                edges: vec![],
            },
            ..Default::default()
        };
        let json = serde_json::to_string(&fp).unwrap();
        let back: RustSemanticFootprintAudit = serde_json::from_str(&json).unwrap();
        assert_eq!(back.derivation_subgraph.nodes.len(), 1);
        assert!(matches!(
            back.derivation_subgraph.nodes[0].kind,
            SemanticNodeKind::Alias
        ));
    }

    #[test]
    fn semantic_node_kind_non_exhaustive_with_other_variant_accepts_unknown_names() {
        let k = SemanticNodeKind::Other {
            name: Arc::from("UnknownFutureVariant"),
        };
        let json = serde_json::to_string(&k).unwrap();
        let back: SemanticNodeKind = serde_json::from_str(&json).unwrap();
        match back {
            SemanticNodeKind::Other { name } => {
                assert_eq!(name.as_ref(), "UnknownFutureVariant");
            }
            other => panic!("expected Other, got {other:?}"),
        }
    }

    #[test]
    fn loaded_files_unions_vfs_reads_and_shared_load_reuses() {
        let fp = RustSemanticFootprintAudit {
            vfs_reads: vec![
                VfsReadRecord {
                    canonical_id: Arc::from("/b.ts"),
                    layer: VfsLayer::Overlay,
                    cache_hit: true,
                    bytes_read: 10,
                    request_id: 1,
                },
                VfsReadRecord {
                    canonical_id: Arc::from("/a.ts"),
                    layer: VfsLayer::Disk,
                    cache_hit: false,
                    bytes_read: 20,
                    request_id: 1,
                },
            ],
            shared_load_reuses: vec![
                SharedLoadReuseRecord {
                    canonical_id: Arc::from("/c.ts"),
                    winner_request_id: 1,
                    winner_audited: true,
                },
                // dup to prove dedup
                SharedLoadReuseRecord {
                    canonical_id: Arc::from("/a.ts"),
                    winner_request_id: 2,
                    winner_audited: false,
                },
            ],
            ..Default::default()
        };
        let files = fp.loaded_files();
        assert_eq!(
            files.iter().map(|s| s.as_ref()).collect::<Vec<_>>(),
            vec!["/a.ts", "/b.ts", "/c.ts"]
        );
    }
}
