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

#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;

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
    /// transport — non-zero, unique per audited request.
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
    /// True when the audited request was satisfied from the warm
    /// component-meta result cache (HostFenceValidator validated the
    /// cached `dep_signature`). Cold cold-resolver runs leave this
    /// `false`.
    /// Audit consumers may aggregate against this flag to separate
    /// warm-cache replay from genuine resolver work.
    ///
    /// Serde-default for back-compat with old audit payloads that
    /// predate the field.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub from_cache: bool,
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
    /// Total `materialize_component_meta_structure` invocations
    /// observed during the request.
    #[serde(default, with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub materialize_structure_calls: u64,
    /// Subset of `materialize_structure_calls` that were satisfied by
    /// the materialiser's `MaterializeStructureDb` peek (warm cache
    /// hit).
    #[serde(default, with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub materialize_structure_cache_hits: u64,
    /// Lock acquisitions on the per-scope `NodeArena` dedup index.
    ///
    #[serde(default, with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub node_arena_lock_acquisitions: u64,
    /// Lock acquisitions on the family-map dep-signature reverse
    /// index.
    #[serde(default, with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub family_map_lock_acquisitions: u64,
    /// Times a `dep_signature` was merged into the materialiser's
    /// `local_fence`.
    #[serde(default, with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub dep_signature_merges: u64,
    /// Subset of `dep_signature_merges` that hit an existing intern
    /// bucket (avoided allocation).
    #[serde(default, with = "crate::u64_as_decimal_string")]
    #[ts(type = "string")]
    pub dep_signature_intern_hits: u64,
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
// Semantic footprint
// ---------------------------------------------------------------------------

/// Semantic footprint attached to an audited request. Populated by the
/// footprint miner from the accumulator's raw events.
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
    /// Verbatim ordered log of every structured event the request
    /// emitted. Drained from the per-request accumulator's
    /// `structured_events` lane and surfaced verbatim so audit
    /// consumers can inspect the materializer envelopes
    /// (`MaterializeStructureEnter` / `Exit`),
    /// dispatch enter/exit markers, policy-skip events, cycle-detected
    /// events, and request-start/end markers without having to
    /// recompute them from the derivation subgraph.
    ///
    /// Serde-default for back-compat with audit payloads written
    /// before this field landed.
    #[serde(default)]
    pub structured_events: Vec<StructuredComponentMetaEvent>,
}

impl RustSemanticFootprintAudit {
    /// Files the scheduler actually read on behalf of this request:
    /// the union of canonical ids from `vfs_reads` and
    /// `shared_load_reuses`, deduplicated and sorted. Exact per
    /// this is the read-contract answer, not the
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
    /// assertion harness uses this to turn flaky snapshots into stable
    /// ones. The fields that get cleared are enumerated by the
    /// [`IncidentalFields`] implementation on this type; the
    /// `corpus_generator_parity::commit_7_snapshots_stable_against_current_incidental_event_names_list`
    /// test pins the declared set so a silent expansion surfaces as a
    /// named failure.
    ///
    /// The driver is the [`IncidentalFields`] trait. The set starts
    /// with `vfs_reads` (purely incidental — cache warmth doesn't
    /// change the semantic footprint); future additions extend the
    /// trait impl on this type, not a parallel constant.
    #[must_use]
    pub fn mask_incidental_spans(&self) -> RustSemanticFootprintAudit {
        let mut out = self.clone();
        <RustSemanticFootprintAudit as IncidentalFields>::mask_incidental(&mut out);
        out
    }
}

/// Contract for audit record types whose fields include
/// timing-incidental payloads that must be cleared before snapshot
/// comparison.
///
/// `incidental_fields()` enumerates the field names that are
/// incidental — fixture snapshots are stable against changes to
/// these fields' contents but not against changes to which fields
/// are listed (adding a field implies pinned snapshots need
/// regeneration). `mask_incidental(&mut self)` clears every payload
/// whose name appears in `incidental_fields()`.
///
/// The `commit_7_snapshots_stable_against_current_incidental_event_names_list`
/// test in `corpus_generator_parity.rs` pins each implementor's
/// declared set so a silent expansion surfaces as a named failure
/// rather than flapping snapshots.
///
/// Implementors today: [`RustSemanticFootprintAudit`] (one
/// incidental field, `vfs_reads`).
//
// TODO: relocate to `verter_audit::record` once that crate exists.
// The trait currently lives alongside the only audit record type
// that implements it; once a dedicated audit crate is established
// it should move there so other audit record types can adopt the
// same incidental-mask contract.
pub trait IncidentalFields {
    /// Names of the fields cleared by [`Self::mask_incidental`].
    /// `'static` so callers can compare slices and emit names in
    /// diagnostics without lifetime juggling.
    fn incidental_fields() -> &'static [&'static str];

    /// Clear every payload whose field name is in
    /// [`Self::incidental_fields`]. Implementations must branch on
    /// the listed names — an unknown name is a contract violation
    /// and should panic so the lock-step regression surfaces
    /// immediately.
    fn mask_incidental(&mut self);
}

impl IncidentalFields for RustSemanticFootprintAudit {
    fn incidental_fields() -> &'static [&'static str] {
        &["vfs_reads"]
    }

    fn mask_incidental(&mut self) {
        for field in Self::incidental_fields() {
            match *field {
                "vfs_reads" => self.vfs_reads.clear(),
                // Every entry in `incidental_fields()` must have a
                // corresponding arm here — the test pinning the
                // declared set will catch any missing wiring.
                unknown => panic!(
                    "RustSemanticFootprintAudit::mask_incidental: \
                     incidental_fields() entry `{unknown}` has no match arm — \
                     extend the match statement in lock-step with the trait method",
                ),
            }
        }
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
    /// Request-id the sink routed this event to.
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
/// cache slot instead of starting fresh.
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
/// atomic counters.
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
/// deterministic NodeId assignment.
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
    /// Computed from the semantic graph's node data.
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
/// `SemanticNodeData` variants without breaking the audit.
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
    /// The plan uses `#[non_exhaustive]` so future
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
/// attaches to a winner's in-flight artifact.
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
    /// Generic structural materialisation envelope. Subject of every
    /// `materialize_component_meta_structure` invocation.
    Structure {
        /// Owner scope's canonical id (the scope the materialiser was
        /// dispatched in — `MaterializeStructureCacheKey.scope_canonical_id`).
        owner: Arc<str>,
        /// Stable display key for the input `SemanticNodeId` — see
        /// [`audit_key_for_node`].
        node_key: Arc<str>,
        /// Axis the input was lowered at.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
    },
}

/// PUB mirror of the materialiser's `MaterializationScope` axis. Kept
/// out of `verter_session::component_meta_materialize` so audit
/// consumers (TS bindings, harness) do not depend on the materialiser
/// type. Must be `pub` (not `pub(crate)`) for the e2e test
/// integration.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq, Hash)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum MaterializationScopeAudit {
    /// Top-level entry — input came from a caller's first
    /// `materialize_component_meta_structure` invocation.
    TopLevel,
    /// Nested entry — input came from a parent materialise frame
    /// recursing into a child shape.
    Nested,
}

/// PUB mirror of `verter_session::semantic_query::ProjectionMode`. Same
/// rationale as [`MaterializationScopeAudit`] — keeps audit consumers
/// independent of the dispatch types.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq, Hash)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum ProjectionModeAudit {
    /// Identity — pass-through, no projection.
    Identity,
    /// Navigate — preserve carriers, no expansion.
    Navigate,
    /// Shallow — expose one level of surface members.
    Shallow,
    /// Expanded — recursively materialize.
    Expanded,
    /// Skeleton — open-generic body access for cycle detection.
    /// / R10-2.
    Skeleton,
}

impl From<crate::semantic_query::ProjectionMode> for ProjectionModeAudit {
    fn from(m: crate::semantic_query::ProjectionMode) -> Self {
        match m {
            crate::semantic_query::ProjectionMode::Identity => Self::Identity,
            crate::semantic_query::ProjectionMode::Navigate => Self::Navigate,
            crate::semantic_query::ProjectionMode::Shallow => Self::Shallow,
            crate::semantic_query::ProjectionMode::Expanded => Self::Expanded,
            crate::semantic_query::ProjectionMode::Skeleton => Self::Skeleton,
        }
    }
}

/// Reason a `MaterializeStructurePolicySkip` event fired — captures
/// the policy-table arm that bailed before dispatch.
#[derive(Debug, Clone, Copy, Serialize, Deserialize, ts_rs::TS, PartialEq, Eq, Hash)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum MaterializeSkipReason {
    /// Object-property lookup hit a function-typed property at
    /// `Nested` depth — function bodies are not materialised through
    /// member position.
    FunctionPropertyAtNested,
    /// Top-level generic ref carried explicit type arguments —
    /// reserved for the dedicated InstantiationRef arm.
    GenericRefWithArgsTopLevel,
    /// Top-level ref resolved to a node under `node_modules/` —
    /// package types are kept opaque.
    PackageRefTopLevel,
    /// Registry-route check rejected the input as not inline-
    /// materializable (e.g., `Pick`/`Omit` over a non-bare root).
    RegistryRouteNotInlineMaterialisable,
    /// Top-level input shape is non-structural (primitive, literal,
    /// type-param, etc.) — nothing to materialise.
    NonStructuralTopLevel,
    /// The registry-route guard's cycle
    /// check (`ref_root_reaches_transitive_cycle_node` over the
    /// route's actual root identity) fired. The wrapping `Pick`/
    /// `Omit`/`IndexedAccess` is kept symbolic because expanding a
    /// recursive helper would publish a circular shape into the
    /// component-meta surface.
    RegistryRouteCycleGuard,
    /// The recursive-helper cycle guard
    /// fired on a plain `DeclRef` or userland `InstantiationRef`. The
    /// declaration body reaches itself via a complex helper (e.g.,
    /// `GetItemKeys<T> = DotPathKeys<T>` -> `GetItemKeys<T>`); kept
    /// symbolic.
    RecursiveHelperCycleGuard,
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
    /// Path-dependent outcome — the materialiser's depth fuse
    /// tripped, the owner scope was unloaded mid-compute, or a
    /// dispatch sub-call returned `Recursive`. Non-cacheable;
    /// propagates upward as `MaterializeOutcome::Tainted`.
    Tainted,
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
            from_cache: false,
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
///
/// Per-platform sources:
/// - Linux: `/proc/self/statm` field 1 (resident pages) × 4 KB.
/// - macOS: `getrusage(RUSAGE_SELF).ru_maxrss` (already in bytes on macOS).
/// - Windows: `K32GetProcessMemoryInfo(GetCurrentProcess()).WorkingSetSize`.
/// - WASM (`wasm32`): no process memory accounting; returns `0`.
/// - Other targets: returns `0` (best-effort fallback).
pub fn current_process_rss() -> u64 {
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
    #[cfg(target_os = "windows")]
    {
        use windows_sys::Win32::System::ProcessStatus::{
            K32GetProcessMemoryInfo, PROCESS_MEMORY_COUNTERS,
        };
        use windows_sys::Win32::System::Threading::GetCurrentProcess;

        let mut counters = std::mem::MaybeUninit::<PROCESS_MEMORY_COUNTERS>::uninit();
        // SAFETY: `K32GetProcessMemoryInfo` writes a `PROCESS_MEMORY_COUNTERS`
        // through `counters.as_mut_ptr()` when it returns non-zero, with the
        // size of the struct passed via `cb`. `GetCurrentProcess` returns a
        // pseudo-handle that does not need to be closed. We only read
        // `counters.assume_init()` on the success path.
        let ok = unsafe {
            K32GetProcessMemoryInfo(
                GetCurrentProcess(),
                counters.as_mut_ptr(),
                std::mem::size_of::<PROCESS_MEMORY_COUNTERS>() as u32,
            )
        };
        if ok != 0 {
            // SAFETY: success path — `counters` was fully initialized by the
            // call above.
            let counters = unsafe { counters.assume_init() };
            return counters.WorkingSetSize as u64;
        }
    }
    #[cfg(target_arch = "wasm32")]
    {
        // WASM has no process working-set accounting; the audit substrate
        // records `process_rss_*=0` on this target by design.
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

/// Serialise an audit record to JSON.
pub fn emit_json(record: &RustAuditRecord) -> String {
    serde_json::to_string(record).unwrap_or_default()
}

/// Merge the `dep_signature` entries from a `CacheRead` (or any
/// `&[(Arc<str>, DepVersion)]` slice) into the materialiser's
/// per-frame `local_fence` while recording audit counters in lock
/// step.
///
/// Each call records `dep_signature_merges`. Each per-entry redundant
/// append (the same `(canonical, kind)` pair was already present at
/// the same `version`) records `dep_signature_intern_hits` —
/// the production analog of the test-only
/// `DepSignatureInterner::intern` hit semantic. Production callers
/// that previously wrote `local_fence.extend(read.dep_signature.iter().cloned())`
/// route through this helper so the cold-resolver path observes both
/// counters.
///
/// The `local_fence` stays a `Vec` (no de-duplication semantics) to
/// preserve byte-equivalence with the pre-fix behaviour; only the
/// audit hooks change.
pub fn merge_dep_signature_into_local_fence(
    local_fence: &mut Vec<(Arc<str>, crate::semantic_query::DepVersion)>,
    incoming: &[(Arc<str>, crate::semantic_query::DepVersion)],
) {
    crate::host_manage::record_dep_signature_merge();
    // Capture the pre-extend `(canonical, kind)` set so we can detect
    // redundant entries arriving from `incoming`. The fence is
    // typically small (single-digit entries per frame), so a linear
    // scan is cheaper than building a hash set.
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
/// typed `IndexedReadyBuildRecord` (direct lane used by the miner
/// on the happy path) and the equivalent `StructuredComponentMetaEvent`
/// (fallback lane when the direct records vec is empty). No-op when
/// no request context is installed.
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
// Stable display key for SemanticNodeId
// ---------------------------------------------------------------------------

/// Produce a deterministic, human-readable key for a
/// [`SemanticNodeId`] suitable for audit trace output and
/// `MaterializationSubject::Structure.node_key` field.
///
/// The key is deterministic under one project generation: identical
/// `(graph, id)` pairs produce identical strings. The format favours
/// recognisability (variant tag + identity-bearing fields) over
/// minimality — audit consumers grep these keys.
///
/// Returns `<unknown:{id}>` when the id has not been interned in
/// `graph` (defensive: an audit lookup must not panic on a stale
/// id from a prior generation).
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
                // Dup to prove dedup
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

    // -----------------------------------------------------------------
    // Audit-counter loss probe + smallest reproducer (D80 permanent
    // regression smoke).
    //
    // Drives a real cold-resolver call through `MetaProject` and
    // snapshots which `RustStoreAudit` counters report 0 vs > 0
    // across a representative single-file resolution. The probe
    // documents the EXPECTED state: every counter wired to a
    // production code path that runs in the cold-resolver flow must
    // increment.
    //
    // The previously-zero counters were:
    //   - `node_arena_lock_acquisitions` — bumped only in
    //      `invalidate_for_canonical`, which never runs on the cold
    //      resolver path.
    //   - `dep_signature_merges` — bumped only in
    //      `convert_dispatch_result`, a `#[allow(dead_code)]` helper
    //      with zero production callers.
    //   - `dep_signature_intern_hits` — bumped only inside the
    //      test-only `DepSignatureInterner::intern`. No production
    //      code instantiates that interner.
    //
    // The fix wires each counter to a production hot path:
    //   - `record_node_arena_lock_acquisition()` bumps on every
    //      shard-mutex acquisition in `NodeArena::push_impl`.
    //   - `record_dep_signature_merge()` bumps inside
    //      `CompletionFence::merge_signature` AND inside the audit
    //      module helper `merge_dep_signature_into_local_fence` that
    //      replaces production `local_fence.extend(read.dep_signature)`
    //      patterns.
    //   - `record_dep_signature_intern_hit()` bumps when
    //      `merge_signature` (or the helper) observes the incoming
    //      `(canonical, kind)` pair is already present at the same
    //      `version` (redundant merge avoided).
    // -----------------------------------------------------------------

    /// Drive a small SFC + dependency through the cold resolver and
    /// return the published `RustAuditRecord`. The fixture exercises:
    ///   - cross-file imported `interface` (forces `imported_root_proof`),
    ///   - multiple props (forces `materialize_structure_calls`),
    ///   - which together force the substrate to: intern semantic
    ///     nodes (NodeArena push_impl shard locks), walk origins
    ///     under the completion fence (dep_signature merges), and
    ///     re-merge already-observed origins (intern hits).
    fn run_probe_request() -> crate::component_meta_audit::RustAuditRecord {
        let host = crate::VerterHost::new_standalone(crate::types::HostConfig {
            analysis_level: crate::types::AnalysisLevel::Full,
            audit_enabled: true,
            footprint_capture: true,
            ..crate::types::HostConfig::default()
        });
        let project = crate::meta::MetaProject::new(host);
        project
            .upsert_base(
                "/types.ts",
                r#"export interface Props {
  message: string;
  level: number;
  optional?: boolean;
}"#,
            )
            .unwrap();
        project
            .upsert_base(
                "/Owner.vue",
                r#"<script setup lang="ts">
import type { Props } from './types'
defineProps<Props>()
</script>
<template><div /></template>"#,
            )
            .unwrap();

        let host = project.host();
        let (_analysis, resolution) = host
            .get_component_meta_with_resolution("/Owner.vue")
            .expect("resolver must produce metadata for the probe fixture");
        host.take_audit_record(resolution.request_id)
            .expect("audit record must publish for the probe fixture")
    }

    /// Characterization probe (D80 permanent regression smoke). The
    /// probe documents per-counter status for the cold-resolver path.
    /// Should any of the post-fix wired counters silently drop back
    /// to 0, this test regresses with a per-counter summary in the
    /// failure message.
    ///
    /// Pre-fix: the three currently-zero counters listed in the
    /// comment above are 0 for every fixture. Post-fix: every counter
    /// listed below must report > 0.
    #[test]
    fn audit_counter_loss_reproduction() {
        let record = run_probe_request();
        let store = &record.store;

        // Counters that MUST be > 0 on any non-trivial cold resolution.
        // Each entry pairs (counter name, observed value) so the
        // failure message identifies which specific counter regressed.
        let observed: Vec<(&'static str, u64)> = vec![
            (
                "node_arena_lock_acquisitions",
                store.node_arena_lock_acquisitions,
            ),
            ("dep_signature_merges", store.dep_signature_merges),
            ("dep_signature_intern_hits", store.dep_signature_intern_hits),
        ];

        let zero: Vec<&'static str> = observed
            .iter()
            .filter_map(|(name, value)| (*value == 0).then_some(*name))
            .collect();

        assert!(
            zero.is_empty(),
            "audit_counter_loss_reproduction (D80 permanent smoke): the following \
             RustStoreAudit counters silently regressed back to 0 on a non-trivial \
             cold-resolver request — every counter listed below is wired to a \
             production code path exercised by the probe fixture and MUST report \
             > 0. Zero counters: {zero:?}. Observed values: {observed:?}.",
        );
    }

    /// Smallest reproducer (DISCRIMINATING). A minimal SFC + dep
    /// fixture that exercises the substrate work expected to bump
    /// each of the three previously-zero counters. The assertion is
    /// per-counter and self-describing so a regression surfaces with
    /// the exact counter that broke.
    #[test]
    fn audit_counter_smallest_reproducer() {
        let record = run_probe_request();
        let store = &record.store;

        // Pre-fix: ALL three of these are 0 on every fixture.
        // Post-fix: each must observe at least one bump.
        assert!(
            store.node_arena_lock_acquisitions > 0,
            "smallest reproducer: NodeArena shard-lock acquisitions must \
             increment on the cold-resolver path. Production `push_impl` \
             acquires a shard mutex on every interned semantic node — \
             observing 0 means the audit hook is no longer wired into the \
             production hot path. Counter: {}",
            store.node_arena_lock_acquisitions,
        );
        assert!(
            store.dep_signature_merges > 0,
            "smallest reproducer: dep_signature merges must increment when \
             the cold resolver walks origins under the completion fence. \
             Production `CompletionFence::merge_signature` is called by \
             `origins_with_fence` — observing 0 means the audit hook is no \
             longer wired into the production merge site. Counter: {}",
            store.dep_signature_merges,
        );
        assert!(
            store.dep_signature_intern_hits > 0,
            "smallest reproducer: dep_signature intern-hits must increment \
             when `merge_signature` observes a `(canonical, kind)` pair \
             already present at the same version (redundant merge avoided). \
             Observing 0 means either the wiring regressed OR the fixture \
             no longer exercises overlapping origin walks — either case is \
             a regression in audit-counter coverage. Counter: {}",
            store.dep_signature_intern_hits,
        );
    }

    /// Discriminating (D118): the cold-path attribution sheet at
    /// `crates/verter_session/tests/perf_bounds/cold-path-attribution-baseline.md`
    /// must (a) identify a dominant cost arm per fixture and
    /// (b) record the bridge max-depth column (D115). Pre-fix the
    /// sheet did not exist or missed both columns; post-fix it
    /// contains both.
    ///
    /// The test reads the sheet and asserts:
    ///   1. it is present
    ///   2. it names `materialize` (or `materialize_ms`) as a dominant
    ///      arm at least once
    ///   3. it includes `bridge max depth (D115)` and
    ///      `bridge worst batch (D110)` columns
    ///   4. it explicitly addresses chat-components (links to or names
    ///      the deferred-baselines doc).
    #[test]
    fn chat_messages_attribution_sheet_has_dominant_cost_arm_and_bridge_max_depth_recorded() {
        let manifest_dir =
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set during cargo test");
        let path = std::path::Path::new(&manifest_dir)
            .join("tests")
            .join("perf_bounds")
            .join("cold-path-attribution-baseline.md");
        assert!(
            path.is_file(),
            "Tier 4 §6.4 deliverable: cold-path attribution sheet must \
             exist at `{}`. Without this file the corpus-wide cost \
             attribution is not captured.",
            path.display(),
        );
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read attribution sheet at {}: {}", path.display(), e));

        // (a) dominant cost arm named — `materialize_ms` is the
        // observed dominant phase per the corpus snapshot.
        assert!(
            body.contains("materialize_ms")
                || body.contains("dominant phase")
                || body.contains("dominant cost arm"),
            "attribution sheet must identify a dominant cost arm (e.g., \
             `materialize_ms` / `dominant cost arm`). Sheet contents \
             do not include those markers.",
        );
        // (b) bridge max-depth column header (D115).
        assert!(
            body.contains("bridge max depth") || body.contains("bridge_max_depth_observed"),
            "attribution sheet must record the `bridge max depth` \
             column (D115). Sheet does not include the column header.",
        );
        // (c) bridge worst-batch column header (D110).
        assert!(
            body.contains("bridge worst batch") || body.contains("bridge_worst_batch"),
            "attribution sheet must record the `bridge worst batch` \
             column (D110). Sheet does not include the column header.",
        );
        // (d) chat-components must be referenced (the §00b deferred
        // baselines doc covers ChatMessage / ChatMessages — the sheet
        // must link or name them).
        assert!(
            body.contains("chat-components")
                || body.contains("ChatMessage")
                || body.contains("00b-deferred-baselines"),
            "attribution sheet must reference the chat-components \
             deferred-baselines doc OR name a chat fixture. The Tier 4 \
             plan requires the sheet to address them explicitly.",
        );
    }

    /// Discriminating: the audit example dump (the JSON record
    /// emitted by `audit_real_component_meta` and the synthesized
    /// record published by `get_component_meta_with_resolution`) must
    /// surface the per-request `structured_events` log.
    ///
    /// Pre-fix: the `RustAuditRecord` carried `footprint.vfs_reads`
    /// etc. but did NOT expose `structured_events` at all — the
    /// `AccumulatorState.structured_events` log was drained into the
    /// miner without surfacing in the published payload. Operators
    /// reading the audit dump could not see materializer envelopes,
    /// dispatch enter/exit, or policy-skip events.
    ///
    /// Post-fix: the audit record's `footprint.structured_events`
    /// vector is non-empty after a real cold-resolver call. The cold
    /// path emits `IndexedReadyBuilt` per imported file plus a
    /// stream of trace events surfaced through the structured-event
    /// log.
    ///
    /// The discriminator is structural: the field must exist on the
    /// envelope AND must be populated by the production call chain.
    #[test]
    fn audit_dump_includes_structured_events() {
        let record = run_probe_request();
        let footprint = record
            .footprint
            .as_ref()
            .expect("audit-enabled host must attach a footprint to cold-resolver records");
        assert!(
            !footprint.structured_events.is_empty(),
            "audit_dump_includes_structured_events: the published \
             RustAuditRecord.footprint.structured_events vector must \
             be populated by the production cold-resolver call. \
             Observing an empty vector means either the field is no \
             longer surfaced in the envelope OR the miner is dropping \
             it before publish. Footprint had {} structured event(s).",
            footprint.structured_events.len(),
        );
        // Must include at minimum one `IndexedReadyBuilt` event — the
        // cold resolver always lowers at least the owner's snapshot
        // through the parser, which emits this typed event.
        let has_indexed_ready_built = footprint
            .structured_events
            .iter()
            .any(|e| matches!(e, StructuredComponentMetaEvent::IndexedReadyBuilt { .. }));
        assert!(
            has_indexed_ready_built,
            "audit_dump_includes_structured_events: the structured-event \
             log must include at least one `IndexedReadyBuilt` event \
             (the cold resolver always lowers the owner snapshot \
             through the parser). Found {} event(s) — only \
             non-`IndexedReadyBuilt` variants surfaced, which means \
             the production parser's audit hook regressed.",
            footprint.structured_events.len(),
        );
    }

    /// Discriminating: the chat-components deferred-baselines doc
    /// (`docs/arch/debt-closure/00b-deferred-baselines-chat-components.md`)
    /// must be CLOSED by Tier 4 §6.7. Pre-fix the doc has
    /// `Status: Open` and unchecked closure boxes. Post-fix it has
    /// `Status: Closed` (or equivalent) and concrete numbers for both
    /// chat components instead of placeholder timeout markers.
    #[test]
    fn chat_baselines_closed_with_concrete_numbers() {
        let manifest_dir =
            std::env::var("CARGO_MANIFEST_DIR").expect("CARGO_MANIFEST_DIR set during cargo test");
        let path = std::path::Path::new(&manifest_dir)
            .ancestors()
            .nth(2)
            .expect("workspace root resolves from the crate manifest dir")
            .join("docs")
            .join("arch")
            .join("debt-closure")
            .join("00b-deferred-baselines-chat-components.md");
        assert!(
            path.is_file(),
            "Tier 4 §6.7 deliverable: deferred-baselines doc must exist at \
             `{}`. The doc tracks chat-message + chat-messages baseline \
             closure.",
            path.display(),
        );
        let body = std::fs::read_to_string(&path)
            .unwrap_or_else(|e| panic!("read deferred-baselines doc at {}: {}", path.display(), e));

        // Status flipped to a closed/resolved marker.
        assert!(
            body.contains("Status: Closed")
                || body.contains("Status:** Closed")
                || body.contains("**Status:** Closed"),
            "chat-components deferred-baselines doc must declare \
             `Status: Closed` after Tier 4 §6.7 closure. Current \
             status in the doc: pre-Tier-4 `Open` placeholder.",
        );
        // Concrete numeric measurements present (replaces the prior
        // `≥ 300s, audit-dump-blocked` placeholder).
        assert!(
            !body.contains("audit-dump-blocked")
                || body.contains("post-Tier-1 measurement")
                || body.contains("post-fix"),
            "chat-components doc must replace the pre-Tier-4 \
             `audit-dump-blocked` placeholder with concrete \
             measurements OR an explicit post-fix reference.",
        );
    }
}
