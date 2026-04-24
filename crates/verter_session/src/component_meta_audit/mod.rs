#![deny(missing_docs)]
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
    /// Monotonic request id set at
    /// `get_component_meta_with_resolution` entry. Decimal-string
    /// transport (plan §1.4) — non-zero, unique per audited request.
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub request_id: u64,
    /// Canonical file id the request resolved.
    pub canonical_id: String,
    /// Per-phase wall-clock timings (ms).
    pub timings: RustTimingAudit,
    /// Solver-level counters aggregated over the request.
    pub solver: RustSolverAudit,
    /// Store/view counters.
    pub store: RustStoreAudit,
    /// Process memory snapshots (before/after/delta).
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
    /// End-to-end wall-clock for the request.
    pub total_ms: f64,
    /// Time spent capturing inputs (request args, config).
    pub capture_inputs_ms: f64,
    /// Time spent reading the project type store.
    pub store_read_ms: f64,
    /// Time spent merging new store data with the overlay view.
    pub store_merge_ms: f64,
    /// Time spent proving direct imports from the owner file.
    pub direct_import_proof_ms: f64,
    /// Time spent proving transitively-imported type roots.
    pub imported_root_proof_ms: f64,
    /// Time spent inside the type solver.
    pub solver_ms: f64,
    /// Time spent materializing member routes + public types.
    pub materialize_ms: f64,
    /// Time spent serializing the final component-meta payload.
    pub serialize_ms: f64,
}

/// Solver-level counters from `SolverResult.steps`.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RustSolverAudit {
    /// Total solver resolve-steps issued across all invocations.
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub total_resolve_steps: u64,
    /// Number of solver invocations.
    pub solve_count: u32,
}

/// Store/view counters.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RustStoreAudit {
    /// Store-view cache hits.
    pub store_view_hits: u32,
    /// Store-view cache misses.
    pub store_view_misses: u32,
    /// Structural-merge count.
    pub structural_merges: u32,
    /// Imported-dependency entries touched.
    pub imported_dependency_entries: u32,
    /// Imported-dependency byte total.
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub imported_dependency_bytes: u64,
    /// Prepared type declarations.
    pub prepared_type_decls: u32,
    /// Prepared value declarations.
    pub prepared_value_decls: u32,
}

/// Memory snapshots.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RustMemoryAudit {
    /// Process RSS before request start (bytes).
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub process_rss_before_bytes: u64,
    /// Process RSS after request completion (bytes).
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub process_rss_after_bytes: u64,
    /// Signed delta = after − before (bytes).
    #[serde(with = "crate::i64_as_decimal_string")]
    #[ts(type = "string")]
    pub process_rss_delta_bytes: i64,
    /// Host cache memory footprint before the request (bytes).
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub host_cache_before_bytes: u64,
    /// Host cache memory footprint after the request (bytes).
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub host_cache_after_bytes: u64,
    /// Workspace memory footprint before the request (bytes).
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub workspace_before_bytes: u64,
    /// Workspace memory footprint after the request (bytes).
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub workspace_after_bytes: u64,
}

/// Phase-specific audit data threaded through TLS via
/// [`RequestAuditGuard`]. Currently only the imported-root-proof
/// phase is instrumented in detail.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RequestPhaseAudit {
    /// Total milliseconds spent inside the imported-root-proof phase
    /// for the current request. Accumulated by
    /// [`record_imported_root_proof_ms`] against the top-of-stack
    /// TLS entry.
    pub imported_root_proof_ms: f64,
}

// ---------------------------------------------------------------------------
// Semantic footprint — plan §2.1
// ---------------------------------------------------------------------------

/// Semantic footprint attached to an audited request. Populated by the
/// footprint miner (Commit 4) from the accumulator's raw events.
/// Field docs are carried by the individual record-vector type
/// declarations below.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RustSemanticFootprintAudit {
    /// Fresh `IndexedReady` builds observed during the request.
    pub indexed_ready_builds: Vec<IndexedReadyBuildRecord>,
    /// VFS reads fanned out from the workspace's audit sink to this
    /// request's accumulator.
    pub vfs_reads: Vec<VfsReadRecord>,
    /// Records where this request joined a winner's in-flight cache
    /// slot instead of starting from cold.
    pub shared_load_reuses: Vec<SharedLoadReuseRecord>,
    /// Type-instantiation steps observed.
    pub instantiations: Vec<InstantiationRecord>,
    /// Projection steps observed (member / index / keyof / path).
    pub projections: Vec<ProjectionRecord>,
    /// Conditional-type branch decisions.
    pub conditional_decisions: Vec<ConditionalRecord>,
    /// Type-parameter substitution steps.
    pub substitutions: Vec<SubstitutionRecord>,
    /// Alias-resolve hops (`type A = B` traversal).
    pub alias_resolutions: Vec<AliasResolveRecord>,
    /// Materialization envelopes (member routes, public prop types,
    /// fallthrough inheritance, etc.).
    pub materializations: Vec<MaterializationRecord>,
    /// Per-context cache-event tally (exact under concurrency).
    pub cache_outcomes: CacheOutcomeTally,
    /// Report covering derivation-subgraph truncation / orphan-edge
    /// markers.
    pub graph_completeness: GraphCompletenessReport,
    /// Canonicalized derivation subgraph (`NodeRecord`s + edges).
    pub derivation_subgraph: DerivationSubgraph,
}

impl RustSemanticFootprintAudit {
    /// Files the scheduler actually read on behalf of this request:
    /// the union of canonical ids from `vfs_reads` and
    /// `shared_load_reuses`, deduplicated and sorted. Exact per
    /// plan §1.4 — this is the read-contract answer, not the
    /// dependency-graph answer.
    ///
    /// Use [`Self::declared_dependency_files`] for the broader set
    /// that also includes fresh `IndexedReady` builds (dependency
    /// cache entries populated during — or observed alongside — the
    /// request, whether or not they were read via the instrumented
    /// fan-out path).
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

    /// Broader "what dependencies did this request touch at the
    /// cache-entry level" answer: the union of canonical ids from
    /// `vfs_reads`, `shared_load_reuses`, AND `indexed_ready_builds`,
    /// deduplicated and sorted.
    ///
    /// This is useful for discovery, dependency-graph rendering, and
    /// tests that assert a superset (e.g. "the request must at least
    /// have seen files X, Y, Z in its dependency closure"). It is
    /// **explicitly NOT an exact-read contract** — some entries come
    /// from `IndexedReady` snapshots populated pre-request (scheduler
    /// prefetch, shared warmup) and were never read via the
    /// audit-instrumented path for THIS request. Use
    /// [`Self::loaded_files`] when the question is "which files did
    /// the scheduler actually read during THIS request".
    #[must_use]
    pub fn declared_dependency_files(&self) -> Vec<Arc<str>> {
        let mut out = Vec::<Arc<str>>::with_capacity(
            self.vfs_reads.len() + self.shared_load_reuses.len() + self.indexed_ready_builds.len(),
        );
        for r in &self.vfs_reads {
            out.push(Arc::clone(&r.canonical_id));
        }
        for r in &self.shared_load_reuses {
            out.push(Arc::clone(&r.canonical_id));
        }
        for r in &self.indexed_ready_builds {
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

/// Fresh `IndexedReady` build observed during the request. Emitted
/// by the `IndexedReadyBuilt` structured event and surfaced by the
/// miner into `RustSemanticFootprintAudit::indexed_ready_builds`.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct IndexedReadyBuildRecord {
    /// Canonical id of the file whose `IndexedReady` entry was freshly
    /// populated during the request.
    pub canonical_id: Arc<str>,
    /// Content hash of the build's source snapshot — the `IndexedReady`
    /// store's identity key.
    pub whole_hash: Hash16,
}

/// One VFS read fanned out from the workspace sink to this request's
/// accumulator. See the workspace `VfsAuditSink` trait.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct VfsReadRecord {
    /// Canonical id of the file that was read.
    pub canonical_id: Arc<str>,
    /// Which VFS layer served the read (overlay / snapshot / disk /
    /// directory-index-negative / missing).
    pub layer: VfsLayer,
    /// `true` when the read resolved from an in-memory cache (overlay
    /// or snapshot) without touching the disk layer.
    pub cache_hit: bool,
    /// Number of bytes returned (0 for `DirIndexNegative` / `Missing`).
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

/// Joiner record — this request attached to a winner's in-flight
/// cache slot instead of starting fresh. Plan §2.7.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct SharedLoadReuseRecord {
    /// Canonical id of the shared artifact.
    pub canonical_id: Arc<str>,
    /// Request id of the winning (first-to-arrive) request that owns
    /// the in-flight slot.
    #[serde(with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub winner_request_id: u64,
    /// `true` when the winner's request was itself audited (so its
    /// `RustAuditRecord` can be cross-referenced). Consumers render
    /// a different chain-terminal based on this flag.
    pub winner_audited: bool,
}

/// One type-instantiation step in the derivation subgraph.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct InstantiationRecord {
    /// In-audit `NodeId` of the instantiated type.
    pub result: NodeId,
    /// Canonical id of the file declaring the generic.
    pub decl_canonical_id: Arc<str>,
    /// Symbol name of the generic declaration.
    pub decl_symbol_name: Arc<str>,
    /// Fingerprint over the type arguments — part of identity for
    /// dedup and cache keying.
    pub args_fingerprint: Hash16,
    /// NodeIds of the argument types, in declaration order.
    pub args: Vec<NodeId>,
}

/// One projection step (indexed-access / member-access / keyof / …).
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct ProjectionRecord {
    /// In-audit `NodeId` of the projected result.
    pub result: NodeId,
    /// In-audit `NodeId` of the base being projected from.
    pub base: NodeId,
    /// Path segments applied from base → result, in order.
    pub path: Vec<ProjectPathSegment>,
}

/// One conditional-select decision (`T extends U ? X : Y`).
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct ConditionalRecord {
    /// In-audit `NodeId` of the selected branch's result.
    pub result: NodeId,
    /// Which branch the solver selected (`True` / `False` / `Deferred`).
    pub branch: ConditionalBranch,
}

/// One type-parameter substitution step.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct SubstitutionRecord {
    /// In-audit `NodeId` of the post-substitution type.
    pub result: NodeId,
    /// Name of the type parameter being substituted.
    pub param_name: Arc<str>,
    /// In-audit `NodeId` of the concrete argument substituted in.
    pub substituted_with: NodeId,
}

/// One alias-resolve step (`type A = B` traversal).
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct AliasResolveRecord {
    /// In-audit `NodeId` of the type on the right-hand side of the alias.
    pub result: NodeId,
    /// Name of the alias that was followed.
    pub alias_name: Arc<str>,
}

/// One materialization envelope (member route / public prop type /
/// fallthrough inheritance / etc.).
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct MaterializationRecord {
    /// What was being materialized — see [`MaterializationSubject`].
    pub subject: MaterializationSubject,
    /// Wall-clock duration of the envelope in milliseconds.
    pub duration_ms: f64,
}

/// Per-context cache-event tally. No `is_approximate` field — values
/// are EXACT per-request because they come from the request's own
/// atomic counters (plan §1.4).
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct CacheOutcomeTally {
    /// Cold builds this request triggered (no cache hit, no in-flight
    /// peer to join).
    pub cold_builds: u32,
    /// Warm cache hits.
    pub warm_hits: u32,
    /// Times this request joined a peer's in-flight artifact.
    pub joined_waits: u32,
    /// Sentinel observations (placeholder entries that collapse to
    /// a real artifact later).
    pub sentinels: u32,
    /// In-flight aborts followed by a retry loop.
    pub inflight_aborted_retries: u32,
    /// Cold entries reaped during generation reconciliation.
    pub cold_aborts_swept: u32,
}

/// Report covering derivation-subgraph completeness (truncation
/// markers, orphan-edge flags).
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct GraphCompletenessReport {
    /// Set when the miner truncated derivation edges at
    /// `HostConfig::max_derivation_edges`. Walker invocations that
    /// cross the truncation boundary report truncated ancestry.
    pub has_orphan_edges: bool,
    /// Count of edges dropped during truncation.
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

/// In-audit opaque edge id. Assigned by the miner from the sorted
/// canonicalisation of edges so identical requests produce identical
/// serialised footprints.
#[derive(
    Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct EdgeId(pub u32);

/// Derivation subgraph captured by the audit. Nodes and edges are
/// assigned stable opaque ids by the miner. Field docs below name the
/// sort keys.
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

/// One node entry in the derivation subgraph. Identity fields
/// (`kind`, `named_identity`, `structural_hash`) participate in the
/// deterministic NodeId assignment (plan §1.4).
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct NodeRecord {
    /// Structural kind (union / intersection / alias / instantiated /
    /// etc.). See [`SemanticNodeKind`].
    pub kind: SemanticNodeKind,
    /// Named-type identity projection, when this node corresponds to
    /// an exported type symbol. `None` for anonymous nodes.
    pub named_identity: Option<NamedIdentity>,
    /// Content-deterministic hash distinguishing anonymous nodes.
    /// Commit 4 computes this from the semantic graph's node data.
    pub structural_hash: Hash16,
    /// Short human-readable label for the node — used by walker /
    /// chain renderers.
    pub display_label: Arc<str>,
}

/// Named-type identity projection — `(canonical, symbol, args)` triple
/// used to key instantiation equality.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct NamedIdentity {
    /// Canonical id of the file declaring the type symbol.
    pub canonical_id: Arc<str>,
    /// Declared symbol name.
    pub symbol_name: Arc<str>,
    /// Fingerprint over the type arguments applied to the symbol.
    pub args_fingerprint: Hash16,
}

/// `#[non_exhaustive]` + `Other` catchall future-proofs against new
/// `SemanticNodeData` variants landing after Commit 3 without breaking
/// the audit.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
#[non_exhaustive]
pub enum SemanticNodeKind {
    /// Anchor node for a declaration — used as the root of a
    /// derivation starting from an exported type.
    DeclAnchor,
    /// Instantiation of a generic with concrete arguments.
    Instantiated,
    /// Alias target (`type A = B`).
    Alias,
    /// Conditional type (`T extends U ? X : Y`).
    Conditional,
    /// Union type (`A | B`).
    Union,
    /// Intersection type (`A & B`).
    Intersection,
    /// Tuple type.
    Tuple,
    /// Object literal type.
    Object,
    /// Array / readonly-array type.
    Array,
    /// Primitive (`string`, `number`, `boolean`, etc.).
    Primitive,
    /// Unbound type parameter.
    TypeParam,
    /// Opaque placeholder (e.g. a miss / unknown).
    Opaque,
    /// Indexed-access type (`T[K]`).
    IndexedAccess,
    /// `keyof T`.
    KeyOf,
    /// `typeof expr`.
    TypeOf,
    /// Mapped type (`{ [K in ...] : ... }`).
    Mapped,
    /// Template-literal type.
    TemplateLiteral,
    /// Normalized union (post-flatten).
    NormalizeUnion,
    /// Normalized intersection (post-flatten).
    NormalizeIntersection,
    /// Catch-all for variants added to the semantic graph after
    /// Commit 3. The plan uses `#[non_exhaustive]` so future
    /// variants can land without breaking the audit consumer
    /// contract.
    Other {
        /// Name of the unrecognized variant — preserved verbatim for
        /// human inspection.
        name: Arc<str>,
    },
}

/// One derivation edge. `result` is the node produced; `sources` are
/// the nodes consumed; `meta` carries kind-specific payload.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct DerivationEdgeRecord {
    /// NodeId of the node produced by this edge.
    pub result: NodeId,
    /// Kind of derivation step (see [`OriginEdgeKind`]).
    pub kind: OriginEdgeKind,
    /// NodeIds of the input nodes consumed to produce `result`.
    pub sources: Vec<NodeId>,
    /// Kind-specific payload (substitution names, projection
    /// segments, etc.). See [`OriginEdgeMetaDto`].
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
    /// Instantiation of a generic with concrete arguments.
    Instantiate,
    /// Substitution of a type parameter with an argument.
    SubstituteTypeParam,
    /// Selected branch of a conditional type.
    ConditionalSelect,
    /// Binding of an `infer` clause.
    InferBind,
    /// Member projection (`T["name"]` or `.name`).
    ProjectMember,
    /// Indexed projection (`T[K]`).
    ProjectIndex,
    /// Multi-segment path projection.
    ProjectPath,
    /// Normalization step (union / intersection flatten, simplify).
    Normalize,
    /// Alias-resolve hop.
    AliasResolve,
    /// Audit-only edge — this request joined a winner's in-flight
    /// artifact via scheduler dedup. Terminates chain walks into
    /// `shared_load_terminals`.
    SharedLoadReuse,
}

/// Kind-specific payload attached to a `DerivationEdgeRecord`.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum OriginEdgeMetaDto {
    /// Instantiation — carries the names of the generic's type
    /// parameters, in declaration order, to aid display rendering.
    Instantiate {
        /// Names of the declared type parameters.
        type_params: Vec<Arc<str>>,
    },
    /// Type-parameter substitution.
    SubstituteTypeParam {
        /// Name of the parameter being substituted.
        param_name: Arc<str>,
        /// NodeId of the substituted-in type.
        substituted_with: NodeId,
    },
    /// Conditional-type branch selection.
    ConditionalSelect {
        /// Which branch the solver chose.
        branch: ConditionalBranch,
    },
    /// `infer` binding.
    InferBind {
        /// Name of the inferred parameter.
        param_name: Arc<str>,
        /// NodeId the parameter was bound to.
        bound_to: NodeId,
    },
    /// Single-segment member projection.
    ProjectMember {
        /// Member name that was projected out.
        member_name: Arc<str>,
    },
    /// Single-segment indexed projection.
    ProjectIndex {
        /// Index key that was projected out.
        index_key: Arc<str>,
    },
    /// Multi-segment path projection.
    ProjectPath {
        /// Path segments, in traversal order.
        path: Vec<ProjectPathSegment>,
    },
    /// Normalization pass (union/intersection flatten, simplify).
    Normalize {
        /// Specific normalization kind performed.
        kind: NormalizeKind,
    },
    /// Alias-resolve hop.
    AliasResolve {
        /// Name of the alias that was followed.
        alias_name: Arc<str>,
    },
    /// Audit-only edge — this request joined a winner's slot.
    /// Terminates chain walks.
    SharedLoadReuse {
        /// Winning request's id.
        #[serde(with = "crate::u64_as_decimal_string")]
        #[ts(type = "string")]
        winner_request_id: u64,
        /// `true` when the winner's own request was audited so its
        /// record can be consulted.
        winner_audited: bool,
    },
}

/// Conditional-select branch discriminator.
#[derive(
    Debug, Clone, Copy, Hash, Eq, PartialEq, Ord, PartialOrd, Serialize, Deserialize, ts_rs::TS,
)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum ConditionalBranch {
    /// The `extends U` clause was proved true — `X` was selected.
    True,
    /// The `extends U` clause was proved false — `Y` was selected.
    False,
    /// The conditional stayed unresolved (open over a type parameter)
    /// — both arms survive into the result.
    Deferred,
}

/// One step in a projection path (member / index / keyof).
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum ProjectPathSegment {
    /// `.<name>` member access.
    Member {
        /// Member name.
        name: Arc<str>,
    },
    /// `[<key>]` indexed access.
    Index {
        /// Literal key used as the index.
        key: Arc<str>,
    },
    /// `keyof T` — yields the union of keys.
    KeyOf,
}

/// Kind of normalization performed (union-flatten, intersection-
/// flatten, or a simplification pass).
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum NormalizeKind {
    /// Union flattening / dedup pass.
    Union,
    /// Intersection flattening / dedup pass.
    Intersection,
    /// Miscellaneous simplify pass.
    Simplify,
}

/// Subject of a materialization envelope — which owner+member
/// (or other identity) the envelope covers.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum MaterializationSubject {
    /// Member-route materialization (owner's public member lookup).
    MemberRoute {
        /// Owner file's canonical id.
        owner: Arc<str>,
        /// Member name being materialized.
        member: Arc<str>,
    },
    /// Public prop type materialization.
    PublicPropType {
        /// Owner file's canonical id.
        owner: Arc<str>,
        /// Prop name being materialized.
        prop: Arc<str>,
    },
    /// `defineProps<…>()` member materialization.
    DefinePropsMember {
        /// Owner file's canonical id.
        owner: Arc<str>,
        /// Member name being materialized.
        member: Arc<str>,
    },
    /// Fallthrough-inheritance resolver envelope.
    FallthroughInheritance {
        /// Owner file's canonical id.
        owner: Arc<str>,
    },
}

/// Dispatch key kind — semantic-query cache key discriminator.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum DispatchKeyKind {
    /// Resolve a declaration (`typeof …`, `type A = …`).
    ResolveDecl,
    /// Instantiate a generic.
    Instantiate,
    /// Member projection.
    ProjectMember,
    /// Indexed projection.
    ProjectIndex,
    /// Multi-segment path projection.
    ProjectPath,
    /// Normalization pass.
    Normalize,
    /// Resolved named-type key (see Vue macro resolution).
    ResolvedNamedType,
}

/// Cache-outcome discriminator for per-event tallies.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum CacheOutcomeKind {
    /// Warm cache hit.
    Hit,
    /// Cache miss (no entry present).
    Miss,
    /// Joined a peer's in-flight slot and waited.
    JoinedWait,
    /// Observed a sentinel (placeholder) entry.
    Sentinel,
    /// Performed a cold build from source.
    ColdBuild,
    /// Retry loop after an in-flight slot was aborted.
    InflightAbortedRetry,
    /// Cold entry reaped during generation reconciliation.
    ColdAbortSwept,
}

/// Which VFS layer served the read — mirrored from the workspace's
/// own `VfsAuditLayer`.
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

    /// Record `steps` solver resolve-steps and bump the solve-count.
    pub fn record_solver_steps(&mut self, steps: u64) {
        self.solver.total_resolve_steps += steps;
        self.solver.solve_count += 1;
    }

    /// Replace the store-counter block.
    pub fn record_store(&mut self, store: RustStoreAudit) {
        self.store = store;
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
    pub fn record_timings(&mut self, timings: RustTimingAudit) {
        self.timings = timings;
    }

    /// Replace the solver-counter block.
    pub fn record_solver(&mut self, solver: RustSolverAudit) {
        self.solver = solver;
    }

    /// Attach a fully-mined semantic footprint to this builder.
    pub fn record_footprint(&mut self, footprint: RustSemanticFootprintAudit) {
        self.footprint = Some(footprint);
    }

    /// Finalize the builder into a [`RustAuditRecord`] — captures the
    /// request-end RSS, computes the signed delta, and fills the
    /// `total_ms` wall-clock.
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
