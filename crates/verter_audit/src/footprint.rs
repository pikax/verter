#![deny(missing_docs)]
//! Semantic-footprint DTOs and the per-record vector types attached
//! to a [`RequestFootprintAudit`]. Pure data — production miners in
//! `verter_session::component_meta_audit::footprint_miner` populate
//! these types from the per-request accumulator.

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::origin_graph::{
    ConditionalBranch, DerivationSubgraph, MaterializationSubject, NodeId, ProjectPathSegment,
    VfsLayer,
};
use crate::record::{u64_as_decimal_string, Hash16, IncidentalFields};
use crate::structured_event::StructuredAuditEvent;

/// Semantic footprint attached to an audited request.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct RequestFootprintAudit {
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
    /// Materialization envelopes.
    pub materializations: Vec<MaterializationRecord>,
    /// Per-context cache-event tally (exact under concurrency).
    pub cache_outcomes: CacheOutcomeTally,
    /// Report covering derivation-subgraph truncation / orphan-edge
    /// markers.
    pub graph_completeness: GraphCompletenessReport,
    /// Canonicalized derivation subgraph.
    pub derivation_subgraph: DerivationSubgraph,
    /// Verbatim ordered log of every structured event the request
    /// emitted. Drained from the per-request accumulator's
    /// `structured_events` lane and surfaced verbatim so audit
    /// consumers can inspect materializer envelopes, dispatch
    /// enter/exit markers, policy-skip events, and request
    /// start/end markers without recomputing them from the
    /// derivation subgraph.
    ///
    /// Serde-default for back-compat with audit payloads written
    /// before this field landed.
    #[serde(default)]
    pub structured_events: Vec<StructuredAuditEvent>,
    /// Per-request hot-path counters for the resolver / import-route
    /// substrate. Exact per-request because each counter is bumped
    /// against the active observer's per-request atomics. Zero-valued
    /// counters are still emitted (serde does not skip them) so audit
    /// consumers can rely on a stable shape for diffing.
    ///
    /// Serde-default for back-compat with audit payloads written
    /// before this field landed.
    #[serde(default)]
    pub resolver_hot_path: ResolverHotPathCounters,
}

impl RequestFootprintAudit {
    /// Files the scheduler actually read on behalf of this request:
    /// the union of canonical ids from `vfs_reads` and
    /// `shared_load_reuses`, deduplicated and sorted. Exact per
    /// the read-once contract.
    ///
    /// Use [`Self::declared_dependency_files`] for the broader set
    /// that also includes fresh `IndexedReady` builds.
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

    /// Produce a new footprint with "incidental" events stripped —
    /// the assertion harness uses this to turn flaky snapshots into
    /// stable ones. Driver: the [`IncidentalFields`] trait
    /// implementation on this type.
    #[must_use]
    pub fn mask_incidental_spans(&self) -> RequestFootprintAudit {
        let mut out = self.clone();
        <RequestFootprintAudit as IncidentalFields>::mask_incidental(&mut out);
        out
    }
}

impl IncidentalFields for RequestFootprintAudit {
    fn incidental_fields() -> &'static [&'static str] {
        &["vfs_reads"]
    }

    fn mask_incidental(&mut self) {
        for field in Self::incidental_fields() {
            match *field {
                "vfs_reads" => self.vfs_reads.clear(),
                unknown => panic!(
                    "RequestFootprintAudit::mask_incidental: \
                     incidental_fields() entry `{unknown}` has no match arm — \
                     extend the match statement in lock-step with the trait method",
                ),
            }
        }
    }
}

/// Fresh `IndexedReady` build observed during the request.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct IndexedReadyBuildRecord {
    /// Canonical id of the file whose `IndexedReady` entry was freshly
    /// populated during the request.
    pub canonical_id: Arc<str>,
    /// Content hash of the build's source snapshot.
    pub whole_hash: Hash16,
}

/// One VFS read fanned out from the workspace sink to this request's
/// accumulator.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct VfsReadRecord {
    /// Canonical id of the file that was read.
    pub canonical_id: Arc<str>,
    /// Which VFS layer served the read.
    pub layer: VfsLayer,
    /// `true` when the read resolved from an in-memory cache.
    pub cache_hit: bool,
    /// Number of bytes returned (0 for `DirIndexNegative` / `Missing`).
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub bytes_read: u64,
    /// Request-id the sink routed this event to.
    #[serde(with = "u64_as_decimal_string")]
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
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub winner_request_id: u64,
    /// `true` when the winner's request was itself audited.
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
    /// Fingerprint over the type arguments.
    pub args_fingerprint: Hash16,
    /// NodeIds of the argument types, in declaration order.
    pub args: Vec<NodeId>,
}

/// One projection step.
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

/// One conditional-select decision.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct ConditionalRecord {
    /// In-audit `NodeId` of the selected branch's result.
    pub result: NodeId,
    /// Which branch the solver selected.
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

/// One alias-resolve step.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct AliasResolveRecord {
    /// In-audit `NodeId` of the type on the right-hand side of the
    /// alias.
    pub result: NodeId,
    /// Name of the alias that was followed.
    pub alias_name: Arc<str>,
}

/// One materialization envelope.
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
    /// Cold builds this request triggered.
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

/// Report covering derivation-subgraph completeness.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct GraphCompletenessReport {
    /// Set when the miner truncated derivation edges at
    /// `HostConfig::max_derivation_edges`.
    pub has_orphan_edges: bool,
    /// Count of edges dropped during truncation.
    pub edges_truncated: u32,
}

/// Per-request resolver / import-route hot-path counters. Populated
/// by producer-side emits via `verter_audit::current_observer()` and
/// surfaced on [`RequestFootprintAudit::resolver_hot_path`]. Exact
/// per-request — each field maps to one [`crate::AuditEvent`]
/// variant (or pair) that the session-side `RequestContext` bumps
/// atomically.
///
/// All counters are zero by default; consumers diff them against
/// other components' audits to attribute cost spikes.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct ResolverHotPathCounters {
    /// Total invocations of
    /// `run_external_type_frontier_closure_with_view` during the
    /// audited request.
    pub frontier_closure_invocations_total: u32,
    /// Subset of [`Self::frontier_closure_invocations_total`] whose
    /// frontier returned `target = None` (broken import chain).
    pub frontier_closure_invocations_target_none: u32,
    /// Subset of [`Self::frontier_closure_invocations_target_none`]
    /// where the `(owner_canonical, type_name)` pair already emitted
    /// a `None` earlier in the same request — the producer detected
    /// the duplicate via a per-request set. The dominant signal for
    /// the "cross-request negative-resolution caching defect"
    /// hypothesis.
    pub frontier_closure_redundant_target_none_pairs: u32,
    /// Warm hits on a host-owned negative entry in the
    /// resolved-external-type cache. Always `0` until negative
    /// caching lands.
    pub resolved_external_type_cache_negative_hits: u32,
    /// Misses on a host-owned negative entry — the cache had no
    /// "known None" entry to short-circuit, so the closure walked
    /// the frontier fully.
    pub resolved_external_type_cache_negative_misses: u32,
    /// Cold import-route resolutions that returned a positive
    /// target.
    pub resolve_import_cold_positive: u32,
    /// Cold import-route resolutions that returned `None` (no
    /// known target).
    pub resolve_import_cold_negative: u32,
    /// Warm import-route resolutions served with a positive target.
    pub resolve_import_warm_positive: u32,
    /// Warm import-route resolutions served with a known-miss target.
    pub resolve_import_warm_negative: u32,
    /// Import-route lookups that the helper classified as
    /// `import_route_is_known_miss`.
    pub known_miss_route_served: u32,
    /// Known-miss entries the validator revalidated as still missing
    /// in the current `content_generation`.
    pub known_miss_route_revalidated: u32,
    /// Known-miss entries the validator recomputed because the
    /// `content_generation` advanced past the recorded value.
    pub known_miss_route_recomputed: u32,
    /// Cold imported-registry-symbol resolutions (cache miss in
    /// `ImportedRegistryDb`).
    pub imported_registry_cold: u32,
    /// Warm imported-registry-symbol resolutions (`peek` hit).
    pub imported_registry_warm: u32,
    /// Imported-registry-symbol resolutions that returned `None`
    /// from the cold compute path.
    pub imported_registry_negative: u32,
    /// Cold imported-type-root resolutions (closure body ran).
    pub imported_root_cold: u32,
    /// Warm imported-type-root resolutions (closure did not run).
    pub imported_root_warm: u32,
    /// Barrel-export hops traversed during route-frontier resolution.
    pub route_db_barrel_steps: u32,
    /// `export *` wildcard fan-out expansions observed.
    pub route_db_wildcard_fanout: u32,
    /// Cold prepared-decl bundle materializations (singleflight
    /// leader closure ran).
    pub prepared_decl_bundle_cold: u32,
    /// Warm prepared-decl bundle cache hits.
    pub prepared_decl_bundle_warm: u32,
}
