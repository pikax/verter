#![deny(missing_docs)]
//! [`StructuredAuditEvent`] — typed structured events emitted by
//! audited request paths. The session-side macro
//! `component_meta_trace_structured!` constructs these variants and
//! pushes them onto the per-request accumulator.
//!
//! The enum is authoritative; producers MUST add a `// Custom
//! justified: <reason>` comment at every `Custom` construction site
//! (the
//! `every_custom_variant_construction_site_has_justification_comment`
//! architecture guard enforces this).
//!
//! # R23 scope-fence (cache-subsystem emissions)
//!
//! New emissions on the cache subsystem call paths
//! (`FileArtifactStore` admit/evict, `FactRegistry` writes,
//! `RouteDb` per-name resolution, `ValidatedFactCache` validation
//! summaries, augmentation stitching, augmentation-index updates,
//! and admission-guard refusals) MUST use the typed
//! [`StructuredAuditEvent`] variants enumerated below. The
//! [`StructuredAuditEvent::Custom`] escape hatch is forbidden on
//! these surfaces — the
//! `audit_event_shape::stage_6c_augmentation_paths_do_not_emit_custom`
//! and `audit_event_shape::cache_subsystem_paths_do_not_emit_custom`
//! arch guards enforce this scope fence in the
//! `verter_session::resolver_core::route_db`,
//! `verter_session::file_artifact_store`,
//! `verter_session::resolver_core::mod`, and
//! `verter_semantic::facts::registry` source surfaces.
//!
//! The typed cache-subsystem variants are:
//!
//! - [`StructuredAuditEvent::CacheDrainedAtUpsert`]
//! - [`StructuredAuditEvent::FactSignatureOverflow`]
//! - [`StructuredAuditEvent::FactSignatureAdmissionRefused`]
//! - [`StructuredAuditEvent::ModuleAugmentationStitched`]
//! - [`StructuredAuditEvent::ModuleAugmentationIndexShape`]
//! - [`StructuredAuditEvent::FileArtifactCache`]
//! - [`StructuredAuditEvent::FactRegistryWrite`]
//! - [`StructuredAuditEvent::FactValidationSummary`]
//! - [`StructuredAuditEvent::ExportRouteResolved`]

use std::sync::Arc;

use serde::{Deserialize, Serialize};

use crate::origin_graph::{
    DispatchKeyKind, MaterializationScopeAudit, MaterializationSubject, MaterializeSkipReason,
    ProjectionModeAudit, VfsLayer,
};
use crate::payloads::cache_outcomes::CacheOutcomeKind;
use crate::payloads::tags::{
    AdmissionRefusalReason, AugmentationTargetKindTag, CompileCacheModeTag, DowngradeReasonTag,
    FactKeyKindTag, FactLaneTag, FileArtifactCacheAction,
};
use crate::payloads::typeinfo_graph::{
    GraphClosurePolicyTag, GraphOperationTag, TypeInfoDegradationReasonTag,
};
use crate::record::{u64_as_decimal_string, Hash16};

/// Why a cold-compute result was admitted as non-cacheable instead of
/// entering the warm cache.
///
/// The cache-runtime admission types carry this on their non-cacheable
/// arms: a `Cacheable` outcome enters the warm cache, while every other
/// outcome routes the value back to the winning flight alone (joiners
/// fork and recompute) and stamps the reason here so structured refusal
/// telemetry can attribute the miss without a format string.
///
/// This lives in the audit leaf crate so structured refusal events can
/// depend on it without a back-edge to `verter_session`. The session
/// crate re-exports it from `cache_runtime::admission`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum NonAdmissionReason {
    /// The value is an intrinsic / builtin whose result is the same in
    /// every world and is deliberately never cached as a fact-validated
    /// entry.
    IntrinsicNonCacheable,
    /// The path-precise fact signature exceeded the size cap. The value
    /// is correct; the signature is too large to admit safely.
    SignatureOverflow,
    /// The producer observed zero facts on a source-dependent cache, so
    /// there is no signature to validate a warm hit against.
    EmptySignature,
    /// The entry's keyed self-root was edited (or became untracked)
    /// during the cold compute window, so the freshly-built value is
    /// already stale for the keyed file.
    SelfRootConflict,
    /// The computed route depends on the project generation and the
    /// generation moved during the cold window.
    RouteGenerationDependency,
    /// A test forced the admission path to refuse, exercising the
    /// non-cacheable broadcast contract deterministically.
    ForcedTestRefusal,
    /// The world generation under which the value was computed has been
    /// superseded by a newer generation.
    GenerationSuperseded,
    /// Post-compute revalidation rejected the entry just before publish
    /// (a mutation invalidated its dep-signature mid-compute).
    PostComputeRevalidationFailed,
    /// A retention / compute budget was exhausted before the value could
    /// be admitted.
    BudgetExceeded,
    /// The compute was cancelled or its enclosing request was
    /// interrupted before it could publish.
    Cancelled,
    /// The value could not be rooted to a self-root canonical, so a
    /// cross-view joiner could never view-validate it.
    UnresolvedProvenance,
    /// The cold compute itself failed (panic substitute, missing dep,
    /// parse error).
    ComputeFailed,
    /// The result is a GENUINE partial: a request-scoped partiality
    /// signal (budget exhaustion, fatal `QueryError`, same-path
    /// recursion, walker fatal) was folded onto the request's
    /// materialization-cache-suppress sticky during the cold compute.
    /// A partial must NOT warm-replay as complete, so every result-cache
    /// admission gate (`MaterializeStructureDb`, `ShapeCacheDb`,
    /// `ImportedRegistryDb`, `ResolvabilityDb`) routes the value through
    /// `ReturnOnly` under this reason rather than admitting it.
    PartialResult,
}

impl std::fmt::Display for NonAdmissionReason {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let name = match self {
            Self::IntrinsicNonCacheable => "IntrinsicNonCacheable",
            Self::SignatureOverflow => "SignatureOverflow",
            Self::EmptySignature => "EmptySignature",
            Self::SelfRootConflict => "SelfRootConflict",
            Self::RouteGenerationDependency => "RouteGenerationDependency",
            Self::ForcedTestRefusal => "ForcedTestRefusal",
            Self::GenerationSuperseded => "GenerationSuperseded",
            Self::PostComputeRevalidationFailed => "PostComputeRevalidationFailed",
            Self::BudgetExceeded => "BudgetExceeded",
            Self::Cancelled => "Cancelled",
            Self::UnresolvedProvenance => "UnresolvedProvenance",
            Self::ComputeFailed => "ComputeFailed",
            Self::PartialResult => "PartialResult",
        };
        f.write_str(name)
    }
}

/// Typed structured event emitted by an audited request path.
///
/// All variants are `Serialize + Deserialize` so they can be written
/// to the TLS accumulator's event log and, later, to the footprint
/// miner's output without a trip through a format string.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum StructuredAuditEvent {
    /// Emitted at the entry of `get_component_meta_with_resolution`.
    RequestStart {
        /// Canonical id being resolved.
        canonical_id: Arc<str>,
        /// Stamped request id (decimal-string transport).
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        request_id: u64,
    },
    /// Emitted when `get_component_meta_with_resolution` returns.
    RequestEnd {
        /// Request id this event closes.
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        request_id: u64,
        /// `true` when the resolution produced `Some(...)`.
        success: bool,
    },
    /// A fresh `IndexedReady` entry was installed.
    IndexedReadyBuilt {
        /// Canonical id of the newly-built entry.
        canonical_id: Arc<str>,
        /// Content hash of the source snapshot.
        whole_hash: Hash16,
    },
    /// One VFS read was observed by the session-side sink.
    VfsRead {
        /// Canonical id that was read.
        canonical_id: Arc<str>,
        /// VFS layer that served the read.
        layer: VfsLayer,
        /// `true` when served by an in-memory cache.
        cache_hit: bool,
        /// Number of bytes returned.
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        bytes_read: u64,
    },
    /// This request attached to a winner's in-flight slot.
    SharedLoadReuse {
        /// Canonical id of the shared artifact.
        canonical_id: Arc<str>,
        /// Winning request's id.
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        winner_request_id: u64,
        /// `true` when the winner was itself audited.
        winner_audited: bool,
    },
    /// Entering a semantic-query dispatch envelope.
    DispatchEnter {
        /// Kind of dispatch key being resolved.
        key_kind: DispatchKeyKind,
        /// Nesting depth within the envelope stack.
        depth: u16,
    },
    /// Leaving a semantic-query dispatch envelope.
    DispatchExit {
        /// Kind of dispatch key that was resolved.
        key_kind: DispatchKeyKind,
        /// Cache outcome recorded for the dispatch.
        outcome: CacheOutcomeKind,
        /// Wall-clock duration in nanoseconds.
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    /// Start envelope for member-route materialization.
    MaterializeMemberRouteStart {
        /// What is being materialized.
        subject: MaterializationSubject,
    },
    /// End envelope with captured duration.
    MaterializeMemberRouteEnd {
        /// Subject this event closes.
        subject: MaterializationSubject,
        /// Wall-clock duration (ns).
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    /// Start envelope for public-prop-type rematerialization.
    RematerializePublicPropTypeStart {
        /// Subject (owner + prop).
        subject: MaterializationSubject,
    },
    /// End envelope with captured duration.
    RematerializePublicPropTypeEnd {
        /// Subject this event closes.
        subject: MaterializationSubject,
        /// Wall-clock duration (ns).
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    /// `defineProps<…>()` member materialization event.
    MaterializeDefinePropsMember {
        /// Subject (owner + member).
        subject: MaterializationSubject,
    },
    /// Fallthrough-inheritance was computed for an owner file.
    FallthroughInheritanceComputed {
        /// Subject (owner).
        subject: MaterializationSubject,
    },
    /// Imported type-root resolution hop.
    ResolveImportedTypeRoot {
        /// Canonical id of the declaring file.
        canonical_id: Arc<str>,
        /// Symbol name that was being resolved.
        symbol_name: Arc<str>,
    },
    /// Eval-state checkpoint with captured duration.
    CurrentEvalState {
        /// Canonical id whose eval state was captured.
        canonical_id: Arc<str>,
        /// Wall-clock duration (ns).
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    /// Entering `materialize_component_meta_structure`.
    MaterializeStructureEnter {
        /// Stable display key for the input semantic node.
        base: Arc<str>,
        /// Axis the input was lowered at.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
        /// Materialiser stack depth at the entry (post-increment).
        depth: u16,
    },
    /// Leaving `materialize_component_meta_structure`.
    MaterializeStructureExit {
        /// Stable display key for the input semantic node.
        base: Arc<str>,
        /// Axis the input was lowered at.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
        /// Cache outcome recorded for the materialiser entry.
        outcome: CacheOutcomeKind,
        /// Wall-clock duration (ns).
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        duration_ns: u64,
    },
    /// Policy gate fired before dispatch.
    MaterializeStructurePolicySkip {
        /// Stable display key for the input semantic node.
        base: Arc<str>,
        /// Axis the input was at when the gate fired.
        scope_axis: MaterializationScopeAudit,
        /// Specific policy arm that bailed.
        reason: MaterializeSkipReason,
    },
    /// Same-key re-entry detected on the materialiser's thread-local
    /// in-flight stack.
    MaterializeStructureCycleDetected {
        /// Stable display key for the input semantic node.
        base: Arc<str>,
        /// Axis the input was at when the cycle was detected.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
        /// Materialiser stack depth at detection.
        depth: u16,
    },
    /// Defensive depth fuse tripped.
    MaterializeStructureDepthFuseTripped {
        /// Stable display key for the input semantic node.
        base: Arc<str>,
        /// Axis the input was at when the fuse tripped.
        scope_axis: MaterializationScopeAudit,
        /// Caller-side projection mode the materialiser ran with.
        mode: ProjectionModeAudit,
        /// Materialiser stack depth at trip.
        depth: u16,
    },
    /// One cache cascade drain at the per-canonical upsert layer.
    ///
    /// Emitted by the full `host.upsert(...)` path at every cache
    /// drain site enumerated in
    /// `crates/verter_session/tests/fixtures/cache_baseline/evict_canonical_inventory.json`.
    /// The quintuple-unchanged fast path (R1) MUST NOT emit this event;
    /// the structural-change path emits one event per draining
    /// instruction. Cache-reuse tests use the absence of these
    /// events to prove byte-identical re-upsert is a true no-op;
    /// invalidation-characterisation tests use the presence/order
    /// to characterise path-precise invalidation.
    CacheDrainedAtUpsert {
        /// Static identifier for the cache layer being drained
        /// (e.g. `"resolved_type_cache"`, `"eval_env_cache"`,
        /// `"compile_slots"`, `"derived_raw_cache"`,
        /// `"semantic_invalidate"`, `"workspace_parsed_edges"`,
        /// `"resolver_runtime"`, `"store_view_epoch"`,
        /// `"project_type_store"`, `"dependency_cache"`).
        layer: Arc<str>,
        /// Canonical id of the file whose upsert triggered the drain.
        canonical_id: Arc<str>,
    },
    /// R20 typed event: a `ValidatedFactCache` candidate's
    /// `fact_dep_signature` exceeded the
    /// `FACT_SIGNATURE_CAP` size cap and was admitted as
    /// `NonCacheable`. Producers fall back to cold recompute;
    /// correctness is preserved but the warm-cache slot is
    /// skipped for this candidate.
    FactSignatureOverflow {
        /// Number of `FactVersionRef` entries the producer
        /// attempted to admit.
        candidate_size: u32,
        /// Configured cap value at admission time. Today this
        /// equals `verter_session::resolver_core::FACT_SIGNATURE_CAP`
        /// (1024); the field is recorded explicitly so the audit
        /// trail survives future cap tuning.
        cap: u32,
    },
    /// R20 typed event: a `ValidatedFactCache` candidate failed the
    /// admission guard because its `fact_dep_signature` was empty and
    /// the cache is NOT a documented source-independent kind. The
    /// candidate was admitted as `NonCacheable`; correctness is
    /// preserved by falling back to cold recompute every time. The
    /// final-state canary asserts this event fires for the synthetic
    /// empty-signature test only — production producers must observe
    /// at least one fact.
    FactSignatureAdmissionRefused {
        /// Static identifier for the cache layer that refused the
        /// admission (mirrors the `layer` discriminator on
        /// `CacheDrainedAtUpsert`). Values like `"materialize_structure"`,
        /// `"route_db_routes"`, `"validated_fact_cache_generic"`.
        cache_kind: Arc<str>,
        /// Reason for the refusal.
        reason: AdmissionRefusalReason,
    },
    /// Module-augmentation stitching produced an effective export
    /// set for an augmentation target (R29 + G1).
    ///
    /// Cold-path only — emitted once per `EffectiveExportSet`
    /// cold/stale compute, after the augmenter set has been folded
    /// into the consumer's effective surface. The `target_kind_tag`
    /// together with the parallel optional fields below identify the
    /// target. `augmenter_count` and `fingerprint` describe the
    /// `AugmenterSet` that contributed.
    ModuleAugmentationStitched {
        /// Discriminator for the augmentation target kind.
        target_kind_tag: AugmentationTargetKindTag,
        /// External-specifier text when
        /// `target_kind_tag == ExternalSpecifier`.
        external_specifier: Option<Arc<str>>,
        /// Resolved canonical path when
        /// `target_kind_tag == ResolvedRelativeCanonical`.
        resolved_relative_canonical: Option<Arc<str>>,
        /// Wildcard glob pattern when
        /// `target_kind_tag == WildcardAmbient`.
        wildcard_pattern: Option<Arc<str>>,
        /// Number of augmenters that contributed to the surface.
        augmenter_count: u32,
        /// `AugmenterSet.fingerprint` at stitch time. The
        /// `ModuleAugmentationIndexShape` fact recorded on the
        /// consumer's `fact_dep_signature` carries this same value
        /// as its `expected_hash`, so a future augmenter-set change
        /// invalidates the consumer.
        fingerprint: Hash16,
    },
    /// `FileArtifactStore.augmentation_index` entry was installed
    /// or refreshed (R29 + G1).
    ///
    /// Cold-path only — emitted by the index-population path when
    /// a new `AugmentationTargetKey` is inserted (`prev_fingerprint
    /// == None`) or an existing entry's `AugmenterSet.fingerprint`
    /// transitions because a new augmenter has entered or left
    /// `FileArtifactStore` (`prev_fingerprint == Some(...)`).
    /// Downstream consumers that observed the prior fingerprint
    /// fail their `fact_dep_signature` validation on the next read
    /// and recompute.
    ModuleAugmentationIndexShape {
        /// Discriminator for the augmentation target kind.
        target_kind_tag: AugmentationTargetKindTag,
        /// External-specifier text when
        /// `target_kind_tag == ExternalSpecifier`.
        external_specifier: Option<Arc<str>>,
        /// Resolved canonical path when
        /// `target_kind_tag == ResolvedRelativeCanonical`.
        resolved_relative_canonical: Option<Arc<str>>,
        /// Wildcard glob pattern when
        /// `target_kind_tag == WildcardAmbient`.
        wildcard_pattern: Option<Arc<str>>,
        /// Previous fingerprint when this is a refresh; `None` on
        /// first install.
        prev_fingerprint: Option<Hash16>,
        /// New fingerprint after install/refresh.
        new_fingerprint: Hash16,
        /// Number of augmenters in the post-install set.
        augmenter_count: u32,
    },
    /// `FileArtifactStore` admitted or evicted an artifact entry
    /// (R5). Cold-path / mutation-path only — warm reads (`get`,
    /// `get_any`) never emit this event.
    ///
    /// The discriminator carries the canonical id, the action
    /// (`Admit` / `Evict`), the content-hash + parse-env-hash
    /// dimensions of the affected `FileArtifactKey`, and the
    /// post-action `entries.len()` so downstream telemetry can
    /// observe the store's footprint trajectory without an
    /// out-of-band counter.
    FileArtifactCache {
        /// Canonical id whose entry was admitted or evicted.
        canonical_id: Arc<str>,
        /// Discriminator for the action.
        action: FileArtifactCacheAction,
        /// Content hash dimension of the `FileArtifactKey`.
        content_hash: Hash16,
        /// Parse-env hash dimension of the `FileArtifactKey`.
        parse_env_hash: Hash16,
        /// Total entries in the store after this action — for
        /// non-mutating Admit/Evict no-ops the count is the
        /// post-action store size, which equals the pre-action
        /// size.
        entry_count_after: u32,
    },
    /// A parse-domain `Fact` was admitted to the `FactRegistry`
    /// (R10, R11). Parse-time emission — cold path only; fires
    /// once per fact insertion at shallow-process time. The
    /// `semantic_hash` / `display_hash` discriminator pair lets
    /// downstream telemetry observe semantic-vs-display lane
    /// churn without re-reading the registry.
    FactRegistryWrite {
        /// Canonical id whose registry the fact was admitted to.
        canonical_id: Arc<str>,
        /// Discriminator for the structural shape of the fact's
        /// `FactKey`.
        fact_key_kind: FactKeyKindTag,
        /// Discriminator for the lane the fact was observed under.
        lane: FactLaneTag,
        /// `Fact::semantic_hash` recorded at admission time.
        semantic_hash: Hash16,
        /// `Fact::display_hash` recorded at admission time.
        display_hash: Hash16,
    },
    /// Aggregate counters for one `ValidatedFactCache` validation
    /// pass (R24). Warm-hit aggregation only — counters bump
    /// once per fact-validation pass close-out, never per-hit on
    /// the hot path. Per-request consumers fold these counters
    /// into footprint summaries without paying per-validation
    /// emission cost.
    FactValidationSummary {
        /// Stamped request id this summary attributes to.
        #[serde(with = "u64_as_decimal_string")]
        #[ts(type = "string")]
        request_id: u64,
        /// Static identifier for the cache layer this summary
        /// closes (mirrors the discriminator on
        /// `CacheDrainedAtUpsert.layer`).
        cache_kind: Arc<str>,
        /// Number of `fact_dep_signature` validation attempts
        /// performed during the pass.
        validations_attempted: u32,
        /// Number of warm hits — validating candidate found on
        /// first match.
        warm_hits: u32,
        /// Number of stale misses — entry exists but no candidate
        /// validated under the active view.
        stale_misses: u32,
        /// Number of archive-style fallback checks consulted
        /// during the pass (zero in steady state; non-zero on
        /// substrate paths that retain a sidecar archive layer).
        archive_checks: u32,
    },
    /// `RouteDb` resolved a per-name route to a canonical+source
    /// target (R15). Cold-path only — fires when a consumer
    /// actually walks the `EffectiveExportSet` and the resolver
    /// admits a fresh route candidate. The `augmented` field
    /// records whether the resolution went through module
    /// augmentation stitching, so consumers can correlate
    /// `ExportRouteResolved` with `ModuleAugmentationStitched`
    /// without joining on fingerprints.
    ExportRouteResolved {
        /// Canonical id of the provider whose surface was queried.
        provider_canonical: Arc<str>,
        /// Name the consumer asked for (`exported_name`).
        exported_name: Arc<str>,
        /// Canonical id where the route resolved.
        resolved_canonical: Arc<str>,
        /// Defining symbol name in the resolved canonical.
        resolved_source_name: Arc<str>,
        /// `true` when the resolution traversed an augmenter
        /// surface; `false` for a bare native route.
        augmented: bool,
    },
    /// A compile request's actual cache mode differs from the requested
    /// mode. Emitted at most once per compile request, at classification
    /// time, when `actual != requested`. Under the mode fold this is
    /// exactly a `Content -> Stateless` downgrade (a `Content` request
    /// hit a cross-file / session-scoped / IDE-shape reason and floored
    /// to `Stateless`); `Session` and `Stateless` never change mode.
    CompileModeDowngrade {
        /// The cache mode the caller requested.
        requested: CompileCacheModeTag,
        /// The cache mode the runtime actually ran under.
        actual: CompileCacheModeTag,
        /// Every triggering reason, in priority order. Preserved in full
        /// for telemetry even though the public single-reason projection
        /// keeps only the first.
        reasons: Vec<DowngradeReasonTag>,
    },
    /// A typeinfo graph publication completed and admitted a clean
    /// snapshot into the typeinfo result cache.
    ///
    /// Emitted exactly once per cold-publish; warm hits go through
    /// `TypeInfoGraphCacheHit` instead, which keeps the structured-event
    /// bus free of per-hit traffic.
    TypeInfoGraphPublished {
        /// Static identifier for the publication layer (e.g.
        /// `"typeinfo_graph_session"`). Interned at producer-side so
        /// the audit substrate stays `Send + Sync + Deserialize`.
        layer: Arc<str>,
        /// Which graph operation produced the payload.
        operation: GraphOperationTag,
        /// Total nodes in the published snapshot.
        snapshot_node_count: u32,
        /// Number of declared roots in the snapshot.
        roots_count: u32,
        /// Closure policy class the publication ran under.
        closure: GraphClosurePolicyTag,
    },
    /// A typeinfo graph publication completed but the response was
    /// admitted as degraded — at least one node ended at a non-exact
    /// status, or the request failed validation before semantic
    /// execution. Carries the closed reason classification.
    TypeInfoGraphDegraded {
        /// Static identifier for the publication layer.
        layer: Arc<str>,
        /// Which graph operation produced the payload.
        operation: GraphOperationTag,
        /// Closed reason classification for the degradation.
        reason: TypeInfoDegradationReasonTag,
        /// Total nodes in the published (degraded) snapshot.
        snapshot_node_count: u32,
    },
    /// A typeinfo graph request was satisfied from the warm result
    /// cache. The structured payload stays minimal — counters live on
    /// the audit payload, not on every per-hit event.
    TypeInfoGraphCacheHit {
        /// Static identifier for the cache layer that served the hit.
        layer: Arc<str>,
        /// Which graph operation the hit attributed to.
        operation: GraphOperationTag,
    },
    /// Escape hatch for ad-hoc events. Every construction site MUST
    /// carry a `// Custom justified: <reason>` comment.
    Custom {
        /// Short identifier for the event kind.
        name: Arc<str>,
        /// Free-form detail payload.
        detail: Arc<str>,
    },
}

impl std::fmt::Display for StructuredAuditEvent {
    /// Hand-authored `Display` — produces a compact single-line
    /// representation matching the snapshot test pinning in
    /// `verter_session::component_meta_audit::expected_display_snapshots`.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::RequestStart {
                canonical_id,
                request_id,
            } => write!(f, "RequestStart({canonical_id}, #{request_id})"),
            Self::RequestEnd {
                request_id,
                success,
            } => write!(f, "RequestEnd(#{request_id}, success={success})"),
            Self::IndexedReadyBuilt {
                canonical_id,
                whole_hash,
            } => write!(
                f,
                "IndexedReadyBuilt({canonical_id}, hash={})",
                short_hash(whole_hash)
            ),
            Self::VfsRead {
                canonical_id,
                layer,
                cache_hit,
                bytes_read,
            } => write!(
                f,
                "VfsRead({canonical_id}, {layer:?}, hit={cache_hit}, bytes={bytes_read})"
            ),
            Self::SharedLoadReuse {
                canonical_id,
                winner_request_id,
                winner_audited,
            } => write!(
                f,
                "SharedLoadReuse({canonical_id}, winner=#{winner_request_id}, audited={winner_audited})"
            ),
            Self::DispatchEnter { key_kind, depth } => {
                write!(f, "DispatchEnter({key_kind:?}, depth={depth})")
            }
            Self::DispatchExit {
                key_kind,
                outcome,
                duration_ns,
            } => write!(f, "DispatchExit({key_kind:?}, {outcome:?}, {duration_ns}ns)"),
            Self::MaterializeMemberRouteStart { subject } => {
                write!(f, "MaterializeMemberRouteStart({subject:?})")
            }
            Self::MaterializeMemberRouteEnd {
                subject,
                duration_ns,
            } => write!(f, "MaterializeMemberRouteEnd({subject:?}, {duration_ns}ns)"),
            Self::RematerializePublicPropTypeStart { subject } => {
                write!(f, "RematerializePublicPropTypeStart({subject:?})")
            }
            Self::RematerializePublicPropTypeEnd {
                subject,
                duration_ns,
            } => write!(
                f,
                "RematerializePublicPropTypeEnd({subject:?}, {duration_ns}ns)"
            ),
            Self::MaterializeDefinePropsMember { subject } => {
                write!(f, "MaterializeDefinePropsMember({subject:?})")
            }
            Self::FallthroughInheritanceComputed { subject } => {
                write!(f, "FallthroughInheritanceComputed({subject:?})")
            }
            Self::ResolveImportedTypeRoot {
                canonical_id,
                symbol_name,
            } => write!(f, "ResolveImportedTypeRoot({canonical_id}::{symbol_name})"),
            Self::CurrentEvalState {
                canonical_id,
                duration_ns,
            } => write!(f, "CurrentEvalState({canonical_id}, {duration_ns}ns)"),
            Self::MaterializeStructureEnter {
                base,
                scope_axis,
                mode,
                depth,
            } => write!(
                f,
                "MaterializeStructureEnter({base}, {scope_axis:?}, {mode:?}, depth={depth})"
            ),
            Self::MaterializeStructureExit {
                base,
                scope_axis,
                mode,
                outcome,
                duration_ns,
            } => write!(
                f,
                "MaterializeStructureExit({base}, {scope_axis:?}, {mode:?}, {outcome:?}, {duration_ns}ns)"
            ),
            Self::MaterializeStructurePolicySkip {
                base,
                scope_axis,
                reason,
            } => write!(
                f,
                "MaterializeStructurePolicySkip({base}, {scope_axis:?}, {reason:?})"
            ),
            Self::MaterializeStructureCycleDetected {
                base,
                scope_axis,
                mode,
                depth,
            } => write!(
                f,
                "MaterializeStructureCycleDetected({base}, {scope_axis:?}, {mode:?}, depth={depth})"
            ),
            Self::MaterializeStructureDepthFuseTripped {
                base,
                scope_axis,
                mode,
                depth,
            } => write!(
                f,
                "MaterializeStructureDepthFuseTripped({base}, {scope_axis:?}, {mode:?}, depth={depth})"
            ),
            Self::CacheDrainedAtUpsert {
                layer,
                canonical_id,
            } => write!(f, "CacheDrainedAtUpsert({layer}, {canonical_id})"),
            Self::FactSignatureOverflow {
                candidate_size,
                cap,
            } => write!(
                f,
                "FactSignatureOverflow(size={candidate_size}, cap={cap})"
            ),
            Self::FactSignatureAdmissionRefused { cache_kind, reason } => write!(
                f,
                "FactSignatureAdmissionRefused({cache_kind}, {reason:?})"
            ),
            Self::ModuleAugmentationStitched {
                target_kind_tag,
                external_specifier,
                resolved_relative_canonical,
                wildcard_pattern,
                augmenter_count,
                fingerprint,
            } => {
                let target = format_augmentation_target(
                    *target_kind_tag,
                    external_specifier.as_deref(),
                    resolved_relative_canonical.as_deref(),
                    wildcard_pattern.as_deref(),
                );
                write!(
                    f,
                    "ModuleAugmentationStitched({target}, n={augmenter_count}, fp={})",
                    short_hash(fingerprint)
                )
            }
            Self::ModuleAugmentationIndexShape {
                target_kind_tag,
                external_specifier,
                resolved_relative_canonical,
                wildcard_pattern,
                prev_fingerprint,
                new_fingerprint,
                augmenter_count,
            } => {
                let target = format_augmentation_target(
                    *target_kind_tag,
                    external_specifier.as_deref(),
                    resolved_relative_canonical.as_deref(),
                    wildcard_pattern.as_deref(),
                );
                match prev_fingerprint {
                    Some(prev) => write!(
                        f,
                        "ModuleAugmentationIndexShape({target}, prev={}, new={}, n={augmenter_count})",
                        short_hash(prev),
                        short_hash(new_fingerprint),
                    ),
                    None => write!(
                        f,
                        "ModuleAugmentationIndexShape({target}, install={}, n={augmenter_count})",
                        short_hash(new_fingerprint),
                    ),
                }
            }
            Self::FileArtifactCache {
                canonical_id,
                action,
                content_hash,
                parse_env_hash,
                entry_count_after,
            } => write!(
                f,
                "FileArtifactCache({canonical_id}, {action:?}, ch={}, pe={}, n={entry_count_after})",
                short_hash(content_hash),
                short_hash(parse_env_hash),
            ),
            Self::FactRegistryWrite {
                canonical_id,
                fact_key_kind,
                lane,
                semantic_hash,
                display_hash,
            } => write!(
                f,
                "FactRegistryWrite({canonical_id}, {fact_key_kind:?}, {lane:?}, sem={}, disp={})",
                short_hash(semantic_hash),
                short_hash(display_hash),
            ),
            Self::FactValidationSummary {
                request_id,
                cache_kind,
                validations_attempted,
                warm_hits,
                stale_misses,
                archive_checks,
            } => write!(
                f,
                "FactValidationSummary(#{request_id}, {cache_kind}, n={validations_attempted}, warm={warm_hits}, stale={stale_misses}, archive={archive_checks})"
            ),
            Self::ExportRouteResolved {
                provider_canonical,
                exported_name,
                resolved_canonical,
                resolved_source_name,
                augmented,
            } => write!(
                f,
                "ExportRouteResolved({provider_canonical}::{exported_name} -> {resolved_canonical}::{resolved_source_name}, augmented={augmented})"
            ),
            Self::CompileModeDowngrade {
                requested,
                actual,
                reasons,
            } => write!(
                f,
                "CompileModeDowngrade({requested:?} -> {actual:?}, reasons={reasons:?})"
            ),
            Self::TypeInfoGraphPublished {
                layer,
                operation,
                snapshot_node_count,
                roots_count,
                closure,
            } => write!(
                f,
                "TypeInfoGraphPublished({layer}, {operation:?}, nodes={snapshot_node_count}, roots={roots_count}, closure={closure:?})"
            ),
            Self::TypeInfoGraphDegraded {
                layer,
                operation,
                reason,
                snapshot_node_count,
            } => write!(
                f,
                "TypeInfoGraphDegraded({layer}, {operation:?}, reason={reason:?}, nodes={snapshot_node_count})"
            ),
            Self::TypeInfoGraphCacheHit { layer, operation } => {
                write!(f, "TypeInfoGraphCacheHit({layer}, {operation:?})")
            }
            Self::Custom { name, detail } => write!(f, "Custom({name}, {detail})"),
        }
    }
}

fn short_hash(hash: &Hash16) -> String {
    let mut s = String::with_capacity(8);
    for byte in hash.iter().take(4) {
        s.push_str(&format!("{byte:02x}"));
    }
    s
}

/// Format an augmentation target for `Display`. Picks the parallel
/// optional field that matches `target_kind_tag` and renders it
/// concisely.
fn format_augmentation_target(
    target_kind_tag: AugmentationTargetKindTag,
    external_specifier: Option<&str>,
    resolved_relative_canonical: Option<&str>,
    wildcard_pattern: Option<&str>,
) -> String {
    match target_kind_tag {
        AugmentationTargetKindTag::ExternalSpecifier => match external_specifier {
            Some(spec) => format!("ext={spec}"),
            None => "ext=?".to_owned(),
        },
        AugmentationTargetKindTag::ResolvedRelativeCanonical => match resolved_relative_canonical {
            Some(canon) => format!("rel={canon}"),
            None => "rel=?".to_owned(),
        },
        AugmentationTargetKindTag::WildcardAmbient => match wildcard_pattern {
            Some(pat) => format!("wild={pat}"),
            None => "wild=?".to_owned(),
        },
        AugmentationTargetKindTag::GlobalAugmentation => "global".to_owned(),
    }
}

#[cfg(test)]
mod non_admission_reason_tests {
    //! Discriminating coverage for [`NonAdmissionReason`] — the
    //! cache-runtime non-cacheable refusal classification that lives in
    //! the audit leaf crate so structured refusal events can depend on
    //! it without a back-edge to `verter_session`.
    //!
    //! These tests fail to compile / fail their assertions against a
    //! tree where the enum or one of its variants is absent, and pass
    //! once the full variant set is present.
    use super::NonAdmissionReason;

    /// The complete refusal-classification surface, every variant named
    /// once. A regression dropping a variant fails to compile here.
    const ALL: &[NonAdmissionReason] = &[
        NonAdmissionReason::IntrinsicNonCacheable,
        NonAdmissionReason::SignatureOverflow,
        NonAdmissionReason::EmptySignature,
        NonAdmissionReason::SelfRootConflict,
        NonAdmissionReason::RouteGenerationDependency,
        NonAdmissionReason::ForcedTestRefusal,
        NonAdmissionReason::GenerationSuperseded,
        NonAdmissionReason::PostComputeRevalidationFailed,
        NonAdmissionReason::BudgetExceeded,
        NonAdmissionReason::Cancelled,
        NonAdmissionReason::UnresolvedProvenance,
        NonAdmissionReason::ComputeFailed,
    ];

    #[test]
    fn every_reason_is_distinct_and_copy() {
        // `Copy` — moving by value leaves the original usable.
        for (i, &a) in ALL.iter().enumerate() {
            let copied = a;
            assert_eq!(a, copied, "a NonAdmissionReason must be Copy + Eq");
            // Every OTHER variant compares unequal: no two discriminants
            // collapse onto each other (a `#[default]`-style merge would
            // fail this).
            for (j, &b) in ALL.iter().enumerate() {
                if i != j {
                    assert_ne!(a, b, "distinct refusal reasons must not compare equal");
                }
            }
        }
        // The surface has exactly the twelve documented reasons.
        assert_eq!(ALL.len(), 12, "NonAdmissionReason must expose 12 variants");
    }

    #[test]
    fn display_is_distinct_per_variant() {
        // Each variant Displays to a distinct, non-empty name so refusal
        // telemetry can attribute the miss without a format string. A
        // `Display` that collapsed two variants onto the same text would
        // fail this discriminator.
        let mut seen = std::collections::BTreeSet::new();
        for &reason in ALL {
            let rendered = reason.to_string();
            assert!(
                !rendered.is_empty(),
                "{reason:?} must render a non-empty name"
            );
            assert!(
                seen.insert(rendered.clone()),
                "Display collision on {rendered:?}"
            );
        }
    }

    #[test]
    fn serde_round_trips_through_json() {
        for &reason in ALL {
            let json = serde_json::to_string(&reason).expect("serialize NonAdmissionReason");
            let back: NonAdmissionReason =
                serde_json::from_str(&json).expect("deserialize NonAdmissionReason");
            assert_eq!(reason, back, "serde round-trip must preserve the variant");
        }
    }
}
