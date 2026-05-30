#![deny(missing_docs)]
//! [`TypeInfoGraphPayload`] — strongly-typed audit payload for
//! `RequestKind::TypeInfoGraph`.
//!
//! Mirrors the wire-form payload that producers populate from the
//! `SemanticTypeGraph` response. Only the audit-shape data lives
//! here — the producer crate (`verter_session`) is responsible for
//! collecting counters from the graph snapshot and the host runtime.
//!
//! Per the substrate's leaf-only rule the payload depends solely on
//! `serde`, `ts_rs`, and the substrate's own tag enums; it does NOT
//! depend on `verter_protocol`, `verter_session`, or any consumer
//! crate.

use serde::{Deserialize, Serialize};

use crate::payloads::tags::ProjectionModeTag;

/// Closed mirror of the typeinfo graph operation discriminator
/// (`verter_protocol::typeinfo::graph::Operation`).
///
/// Keeps the audit substrate decoupled from `verter_protocol` —
/// producers map the wire operation enum to this tag at the audit
/// emission boundary.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum GraphOperationTag {
    /// `resolve_symbol_graph_with_audit`.
    #[default]
    ResolveSymbol,
    /// `evaluate_type_expression_graph_with_audit`.
    EvaluateExpression,
    /// `project_path_graph_with_audit`.
    ProjectPath,
    /// `relate_with_audit`.
    Relate,
    /// `get_framework_surfaces_with_audit`.
    FrameworkSurfaces,
    /// `expand_graph_around_with_audit`.
    ExpandAround,
    /// `evaluate_flow_narrowing_at_with_audit`.
    FlowNarrowingAt,
    /// `evaluate_contextual_type_at_with_audit`.
    ContextualTypeAt,
}

/// Closed mirror of the reduction-demand axis on
/// `GraphProjectionReductionContext`.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum ReductionDemandTag {
    /// Caller asked for the published surface (the default macro /
    /// consumer-facing shape).
    #[default]
    Published,
    /// Caller asked for the structural-transit surface (the inner
    /// view a normalizer uses while threading hops).
    StructuralTransit,
}

/// Closed mirror of the typeinfo graph closure policy. The wire
/// substrate carries the full policy (with budgets); the audit
/// surface keeps only the discriminator because consumers aggregate
/// by policy class rather than budget value.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum GraphClosurePolicyTag {
    /// Root + its symbol only.
    #[default]
    RootOnly,
    /// Root + hops along the path.
    Path,
    /// Root + one BFS hop.
    OneLevel,
    /// Caller-supplied node and depth budgets.
    Expanded,
    /// Closure derived from a named projection's required edges.
    ProjectionRequired,
}

/// Closed mirror of the per-node exactness lattice. One per node;
/// the payload below carries the cumulative counts per status, so
/// the audit observer can attribute degraded results without
/// re-reading the snapshot.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum ExactnessTag {
    /// Fully resolved to a concrete node.
    #[default]
    ExactResolved,
    /// Fully resolved to a symbolic carrier (e.g. an unbound type
    /// parameter that survives in the projection).
    ExactSymbolic,
    /// Generic blocker that could not be resolved (e.g. an open
    /// extends-clause).
    UnresolvedGeneric,
    /// Partial result — some descendants are not yet exact.
    Partial,
    /// Cold result was a miss.
    Miss,
    /// Construct is unsupported (e.g. an intrinsic without a
    /// registry entry).
    Unsupported,
    /// Budget exceeded during walk.
    BudgetExceeded,
    /// Publication fence exhausted retries.
    Unstable,
    /// Cycle detected and downgraded into a `Cycle` node.
    Cycle,
}

/// Closed mirror of the typeinfo degradation classifier used in
/// `StructuredAuditEvent::TypeInfoGraphDegraded`. Captures WHY a
/// publication was admitted as degraded.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub enum TypeInfoDegradationReasonTag {
    /// Walker exhausted the node budget.
    #[default]
    BudgetExceededNodes,
    /// Walker exhausted the depth budget.
    BudgetExceededDepth,
    /// Publication fence ran past `MAX_INFLIGHT_RETRIES`.
    UnstablePublicationFence,
    /// One or more nodes ended at `Cycle` exactness.
    CycleDetected,
    /// One or more nodes ended at `Unsupported` exactness.
    UnsupportedConstruct,
    /// One or more nodes ended at `Miss` exactness.
    ColdMiss,
    /// One or more nodes ended at `UnresolvedGeneric` exactness.
    UnresolvedGeneric,
    /// Validation error before semantic execution (e.g. malformed
    /// structured expression).
    RequestValidation,
}

/// Strongly-typed audit payload for one typeinfo graph request.
///
/// Producers populate this from the response payload after the
/// graph publication path returns. The substrate stays leaf — the
/// concrete graph snapshot lives in `verter_protocol`; this struct
/// carries only the aggregated counters / discriminators the audit
/// runtime needs.
#[derive(Debug, Clone, Default, Serialize, Deserialize, ts_rs::TS)]
#[ts(export, export_to = "audit.generated.ts")]
pub struct TypeInfoGraphPayload {
    /// Which graph operation produced the response.
    pub operation: GraphOperationTag,
    /// Projection mode the request ran with.
    pub mode: ProjectionModeTag,
    /// Reduction demand axis (published vs structural-transit).
    pub demand: ReductionDemandTag,
    /// Number of declared roots in the request's
    /// `GraphQueryIdentity`.
    pub roots_count: u32,
    /// Closure policy class.
    pub closure: GraphClosurePolicyTag,
    /// Schema version the producer ran under. Echoes
    /// `SemanticTypeGraph.schema_version`.
    pub schema_version: u32,
    /// Total nodes in the response snapshot.
    pub snapshot_node_count: u32,
    /// Total origin edges in the response snapshot.
    pub snapshot_edge_count: u32,
    /// Total symbol nodes in the response snapshot.
    pub snapshot_symbol_count: u32,
    /// Per-status node counts. Sum equals
    /// [`Self::snapshot_node_count`].
    pub exactness_exact_resolved: u32,
    /// See [`Self::exactness_exact_resolved`].
    pub exactness_exact_symbolic: u32,
    /// See [`Self::exactness_exact_resolved`].
    pub exactness_unresolved_generic: u32,
    /// See [`Self::exactness_exact_resolved`].
    pub exactness_partial: u32,
    /// See [`Self::exactness_exact_resolved`].
    pub exactness_miss: u32,
    /// See [`Self::exactness_exact_resolved`].
    pub exactness_unsupported: u32,
    /// See [`Self::exactness_exact_resolved`].
    pub exactness_budget_exceeded: u32,
    /// See [`Self::exactness_exact_resolved`].
    pub exactness_unstable: u32,
    /// See [`Self::exactness_exact_resolved`].
    pub exactness_cycle: u32,
    /// `true` when this record came from the warm cache.
    pub cache_hit: bool,
    /// Number of publication-fence retries (≤
    /// `MAX_INFLIGHT_RETRIES`). Cold fresh publish: 0.
    pub publication_retries: u8,
    /// Number of merged-declaration carriers in the snapshot.
    pub merged_decl_count: u32,
    /// Number of augmentation-related carriers in the snapshot.
    pub augmentation_count: u32,
    /// Number of overload-signature entries in the snapshot.
    pub overload_signature_count: u32,
    /// Number of relation-engine queries the request performed.
    pub relation_check_count: u32,
    /// Number of origin edges emitted by the request (sum of
    /// edge_count across the request's snapshot).
    pub origin_edges_emitted: u32,
    /// Per-projection emission flags. Producers set each flag
    /// when the projection ran for this request.
    pub display_projection_emitted: bool,
    /// See [`Self::display_projection_emitted`].
    pub zod_projection_emitted: bool,
    /// See [`Self::display_projection_emitted`].
    pub json_schema_projection_emitted: bool,
    /// See [`Self::display_projection_emitted`].
    pub storybook_projection_emitted: bool,
    /// See [`Self::display_projection_emitted`].
    pub docs_projection_emitted: bool,
    /// See [`Self::display_projection_emitted`].
    pub type_descriptor_projection_emitted: bool,
    /// `true` when the publication was admitted as degraded. The
    /// concrete reason is carried in
    /// [`Self::degradation_reasons`] (which is empty for a clean
    /// success).
    pub degraded: bool,
    /// Degradation reasons observed during the request. Sorted /
    /// deduplicated by producers. Empty on clean success.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub degradation_reasons: Vec<TypeInfoDegradationReasonTag>,
}

impl TypeInfoGraphPayload {
    /// Construct a payload for a request that failed validation
    /// before semantic execution. Sets [`Self::degraded`] and adds
    /// `TypeInfoDegradationReasonTag::RequestValidation` to the
    /// reason set.
    #[must_use]
    pub fn from_validation_error(operation: GraphOperationTag) -> Self {
        Self {
            operation,
            degraded: true,
            degradation_reasons: vec![TypeInfoDegradationReasonTag::RequestValidation],
            ..Self::empty()
        }
    }

    /// Empty payload — all counters zero, all flags false. Producers
    /// use this as the starting point of an explicit field-by-field
    /// fill.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Sum of every per-exactness counter. Producers can call this
    /// in tests to assert the counters partition
    /// [`Self::snapshot_node_count`].
    #[must_use]
    pub fn total_exactness_counted(&self) -> u32 {
        self.exactness_exact_resolved
            .saturating_add(self.exactness_exact_symbolic)
            .saturating_add(self.exactness_unresolved_generic)
            .saturating_add(self.exactness_partial)
            .saturating_add(self.exactness_miss)
            .saturating_add(self.exactness_unsupported)
            .saturating_add(self.exactness_budget_exceeded)
            .saturating_add(self.exactness_unstable)
            .saturating_add(self.exactness_cycle)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn from_validation_error_marks_degraded() {
        let payload = TypeInfoGraphPayload::from_validation_error(GraphOperationTag::ResolveSymbol);
        assert!(payload.degraded);
        assert_eq!(
            payload.degradation_reasons,
            vec![TypeInfoDegradationReasonTag::RequestValidation],
        );
        assert_eq!(payload.snapshot_node_count, 0);
    }

    #[test]
    fn total_exactness_counted_is_saturating_sum() {
        let mut payload = TypeInfoGraphPayload {
            exactness_exact_resolved: 3,
            exactness_partial: 2,
            exactness_miss: 1,
            ..TypeInfoGraphPayload::default()
        };
        assert_eq!(payload.total_exactness_counted(), 6);
        payload.exactness_cycle = u32::MAX;
        assert_eq!(payload.total_exactness_counted(), u32::MAX);
    }

    #[test]
    fn serde_round_trips_through_json() {
        let payload = TypeInfoGraphPayload {
            operation: GraphOperationTag::EvaluateExpression,
            mode: ProjectionModeTag::Expanded,
            demand: ReductionDemandTag::Published,
            roots_count: 1,
            closure: GraphClosurePolicyTag::OneLevel,
            schema_version: 1,
            snapshot_node_count: 5,
            snapshot_edge_count: 6,
            snapshot_symbol_count: 2,
            exactness_exact_resolved: 4,
            exactness_partial: 1,
            cache_hit: true,
            publication_retries: 0,
            ..TypeInfoGraphPayload::default()
        };
        let json = serde_json::to_string(&payload).expect("serialize");
        let back: TypeInfoGraphPayload = serde_json::from_str(&json).expect("deserialize");
        assert_eq!(back.operation, GraphOperationTag::EvaluateExpression);
        assert_eq!(back.mode, ProjectionModeTag::Expanded);
        assert_eq!(back.snapshot_node_count, 5);
        assert!(back.cache_hit);
    }
}
