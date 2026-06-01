#![deny(missing_docs)]
//! [`ComponentMetaPayload`] — strongly-typed payload for
//! `RequestKind::ComponentMeta`.
//!
//! Materialiser-specific store counters and solver counters live
//! here rather than on the generic
//! [`crate::store::RequestStoreAudit`] envelope so the envelope
//! stays kind-agnostic.

use serde::{Deserialize, Serialize};
use verter_span::Span;

use crate::record::u64_as_decimal_string;

/// Discriminator naming the audited diagnostic class. Mirror of the
/// session-side macro-expansion diagnostics so the audit substrate
/// can render structured diagnostics without depending on
/// `verter_semantic`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub enum AuditDiagnosticKind {
    /// Cyclic alias / heritage chain detected during the synthesis walk.
    CyclicReference,
    /// A query-level resolver budget was exceeded.
    BudgetExceeded,
    /// A conditional type's `extends` relation was undecidable; both
    /// branches survived (no reduction).
    OpenConditional,
    /// A resolver query returned `QueryError::Other` / `QueryError::Miss`
    /// / `QueryError::AliasCycle` / similar, surfaced as a structured
    /// diagnostic.
    ResolverError,
    /// A union arm reduced to the same surface as another arm and was
    /// short-circuited.
    IdempotentArm,
    /// A union arm reduced to an empty surface.
    EmptyUnionArm,
    /// Catch-all for diagnostics that do not map to a dedicated kind.
    Other,
}

/// One audited diagnostic entry. Audit-substrate mirror of
/// `verter_semantic::analysis::component_meta::ExpansionDiagnostic`
/// scoped to the macro-expansion pass — projected at the session
/// boundary by `verter_session::host_audit_bridge`.
#[derive(Debug, Clone, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct AuditDiagnosticEntry {
    /// Discriminator naming the diagnostic class.
    pub kind: AuditDiagnosticKind,
    /// Free-form human-readable message describing the diagnostic.
    /// The session-side bridge populates this with the projected
    /// diagnostic context (declaration name, node id, error kind).
    pub message: String,
    /// SFC-absolute span the diagnostic applies to. `None` when the
    /// session-side projector could not attribute the diagnostic to a
    /// concrete source range.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    #[ts(type = "{ start: number; end: number } | null")]
    pub span: Option<Span>,
    /// Macro index within the script-analysis macro list, if the
    /// diagnostic was attributable to a specific macro invocation.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub macro_index: Option<usize>,
}

/// Component-meta request payload.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export_to = "audit.generated.ts")]
pub struct ComponentMetaPayload {
    /// Total solver resolve-steps issued across all invocations
    /// during this request.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub total_resolve_steps: u64,
    /// Number of solver invocations during this request.
    pub solve_count: u32,
    /// Total `materialize_component_meta_structure` invocations
    /// observed during the request.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub materialize_structure_calls: u64,
    /// Subset of `materialize_structure_calls` that were satisfied by
    /// the materialiser's `MaterializeStructureDb` peek (warm cache
    /// hit).
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub materialize_structure_cache_hits: u64,
    /// Lock acquisitions on the per-scope `NodeArena` dedup index.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub node_arena_lock_acquisitions: u64,
    /// Lock acquisitions on the family-map dep-signature reverse
    /// index.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub family_map_lock_acquisitions: u64,
    /// Times a `dep_signature` was merged into the materialiser's
    /// `local_fence`.
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub dep_signature_merges: u64,
    /// Subset of `dep_signature_merges` that hit an existing intern
    /// bucket (avoided allocation).
    #[serde(with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub dep_signature_intern_hits: u64,
    /// Macro-expansion diagnostics surfaced during the request.
    /// Producers populate via the session-side
    /// `host_audit_bridge::macro_expansion_to_audit_entries`
    /// converter. Empty when the request did not surface diagnostics
    /// (warm cache hits and clean cold runs).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub diagnostics: Vec<AuditDiagnosticEntry>,
    /// `true` when a fatal `QueryError` (`BudgetExceeded`,
    /// `UnstableState`) propagated through the resolver during the
    /// request. Producers gate `ComponentMetaResultDb` publication on
    /// this flag — partially-populated results never warm the shared
    /// final-result cache.
    #[serde(default, skip_serializing_if = "std::ops::Not::not")]
    pub should_suppress: bool,
    /// Per-request count of `Instantiate { body_mode: Expanded }`
    /// dispatches observed against this request. Mirror of the
    /// process-global `SLOT_BINDING_EXPANDED_INSTANTIATE_CALLS`
    /// counter, partitioned per-request so attribution tests can
    /// assert "no synthesis-attributable Expanded Instantiate fired
    /// during this request" without false positives from peer
    /// dispatches in workspace-parallel runs. Snapshotted at request
    /// finalisation from
    /// `RequestContext::expanded_instantiate_calls`.
    ///
    /// Marked `#[serde(default)]` so existing producers that emit
    /// audit JSON without this field (older record snapshots, hand-
    /// authored test fixtures) deserialize cleanly.
    #[serde(default, with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub expanded_instantiate_calls: u64,
    /// Synthesis-ATTRIBUTABLE subset of [`Self::expanded_instantiate_calls`]:
    /// `Instantiate { body_mode: Expanded }` dispatches observed WHILE the
    /// slot-binding synthesis phase was active on the request. The
    /// request-wide `expanded_instantiate_calls` also counts the canonical
    /// macro-surface PRODUCER's legitimate `Expanded` expansions; this
    /// scoped counter isolates synthesis-phase eagerness. The slot-binding
    /// eagerness guard `enrich_does_not_eagerly_instantiate_carrier` asserts
    /// this is ZERO. Snapshotted at request finalisation from
    /// `RequestContext::synthesis_expanded_instantiate_calls`.
    ///
    /// Marked `#[serde(default)]` so existing producers that emit audit JSON
    /// without this field deserialize cleanly.
    #[serde(default, with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub synthesis_expanded_instantiate_calls: u64,
    /// Per-request count of `MemoEntry` insertions published into
    /// the `SemanticGraphStore` warm map during this request.
    /// Mirror of the host-global memo-size delta, partitioned
    /// per-request so attribution tests can assert
    /// "cache_suppress=true synthesis path made no
    /// synthesis-attributable insertions" without false positives
    /// from peer dispatches in workspace-parallel runs.
    /// Snapshotted at request finalisation from
    /// `RequestContext::memo_insertions`.
    ///
    /// Marked `#[serde(default)]` so existing producers that emit
    /// audit JSON without this field (older record snapshots, hand-
    /// authored test fixtures) deserialize cleanly.
    #[serde(default, with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub memo_insertions: u64,
    /// Per-request count of `cooperative_admission` builds whose
    /// warm-publish was skipped because the build landed with
    /// `cache_suppress=true`. Discriminating signal for the
    /// `cache_suppress_true_skips_memo_insertion` regression: a
    /// non-zero value pins that the memo no-poison gate fired
    /// during the request. Snapshotted at request finalisation from
    /// `RequestContext::memo_publish_suppressed`.
    ///
    /// Marked `#[serde(default)]` so existing producers that emit
    /// audit JSON without this field (older record snapshots, hand-
    /// authored test fixtures) deserialize cleanly.
    #[serde(default, with = "u64_as_decimal_string")]
    #[ts(type = "string")]
    pub memo_publish_suppressed: u64,
}
