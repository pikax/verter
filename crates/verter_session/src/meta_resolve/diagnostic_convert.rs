//! Conversion helpers between the dispatch-layer `ShallowDiagnostic`
//! enum and the component-meta `ExpansionDiagnostic` envelope. These
//! helpers let the component-meta entry-points project walker
//! diagnostics surfaced in `CacheRead.walker_diagnostics` into the
//! consumer-visible `MacroExpansionDiagnostics` payload so consumers
//! see structured reasons (cycles, open conditionals, pathological
//! inputs) without re-walking the graph.

use crate::project_semantic_dispatch::walk::ShallowDiagnostic;
use verter_semantic::analysis::component_meta::{MacroExpansionDiagnostics, MacroExpansionKind};
use verter_semantic::analysis::type_expand::{
    ExpansionDiagnostic, ExpansionExactness, ExpansionExecutionStatus, ExpansionStopReason,
};

/// Project a single `ShallowDiagnostic` to an `ExpansionDiagnostic`.
///
/// Mapping table (1:1 — each `ShallowDiagnostic` projects to a
/// dedicated `ExpansionStopReason` variant):
///
/// | `ShallowDiagnostic` variant | `ExpansionStopReason`      |
/// |-----------------------------|----------------------------|
/// | `DuplicateArmShortCircuited`| `IdempotentArm`            |
/// | `CycleShortCircuited`       | `CyclicReference`          |
/// | `CyclicInstantiation`       | `CyclicInstantiation`      |
/// | `InstantiationError`        | `InstantiationError`       |
/// | `OpenConditional`           | `IndeterminateConditional` |
/// | `PathologicalInput`         | `BudgetExceeded`           |
/// | `UnionArmEmpty`             | `EmptyUnionArm`            |
/// | `UnresolvedSurfaceArm`      | `UnresolvedReference`      |
///
/// Variant payload data (node ids, declaration identities, error
/// details) is preserved through `ExpansionDiagnostic.context` —
/// `ExpansionStopReason` itself is a payload-free `Copy` enum because
/// it is mirrored to a proto enum and a string-encoded TS enum.
#[must_use]
pub(crate) fn shallow_to_expansion(diag: &ShallowDiagnostic) -> ExpansionDiagnostic {
    match diag {
        ShallowDiagnostic::ProjectionWorkLimit { root } => ExpansionDiagnostic {
            reason: ExpansionStopReason::ProjectionWorkLimit,
            context: format!("projection-work-limit@{:?}", root),
            property_name: None,
        },
        ShallowDiagnostic::ConnectedQueryDepthLimit { root } => ExpansionDiagnostic {
            reason: ExpansionStopReason::ConnectedQueryDepthLimit,
            context: format!("connected-query-depth-limit@{:?}", root),
            property_name: None,
        },
        ShallowDiagnostic::DuplicateArmShortCircuited { node } => ExpansionDiagnostic {
            reason: ExpansionStopReason::IdempotentArm,
            context: format!("duplicate-arm-short-circuited@{:?}", node),
            property_name: None,
        },
        ShallowDiagnostic::CycleShortCircuited { node } => ExpansionDiagnostic {
            reason: ExpansionStopReason::CyclicReference,
            context: format!("cycle-short-circuited@{:?}", node),
            property_name: None,
        },
        ShallowDiagnostic::CyclicInstantiation { decl } => ExpansionDiagnostic {
            reason: ExpansionStopReason::CyclicInstantiation,
            context: format!(
                "cyclic-instantiation::{}::{}",
                decl.canonical_id, decl.decl_name
            ),
            property_name: Some(decl.decl_name.to_string()),
        },
        ShallowDiagnostic::InstantiationError { decl, error } => ExpansionDiagnostic {
            reason: ExpansionStopReason::InstantiationError,
            context: format!(
                "instantiation-error::{}::{}::{:?}",
                decl.canonical_id, decl.decl_name, error
            ),
            property_name: Some(decl.decl_name.to_string()),
        },
        ShallowDiagnostic::OpenConditional { node } => ExpansionDiagnostic {
            reason: ExpansionStopReason::IndeterminateConditional,
            context: format!("open-conditional@{:?}", node),
            property_name: None,
        },
        ShallowDiagnostic::PathologicalInput { root } => ExpansionDiagnostic {
            reason: ExpansionStopReason::BudgetExceeded,
            context: format!("pathological-input@{:?}", root),
            property_name: None,
        },
        ShallowDiagnostic::UnionArmEmpty {
            union_node,
            arm_index,
        } => ExpansionDiagnostic {
            reason: ExpansionStopReason::EmptyUnionArm,
            context: format!("union-arm-empty@{:?}#{}", union_node, arm_index),
            property_name: None,
        },
        ShallowDiagnostic::UnresolvedSurfaceArm {
            name,
            owner_canonical,
            owner,
        } => ExpansionDiagnostic {
            reason: ExpansionStopReason::UnresolvedReference,
            context: format!(
                "unresolved-surface-arm::{}::{:?}::{}",
                owner_canonical, owner, name
            ),
            property_name: Some(name.to_string()),
        },
    }
}

/// Project a slice of `ShallowDiagnostic` instances into a
/// `MacroExpansionDiagnostics` envelope tagged with the macro index
/// they apply to. The `macro_kind` discriminates props / emits /
/// slots / exposed so the consumer-side renderer can route the
/// diagnostics to the right surface; `exactness` and `execution_status` summarise
/// whether the walker completed normally (Completed + ExactConcrete)
/// or surfaced an incomplete result (Interrupted + Incomplete) via
/// the `cache_suppress` aggregation.
///
/// The helper lives under the `meta_resolve` boundary so the same
/// `verter_session` crate owns both the producer (walker) and the
/// consumer-projection step.
#[must_use]
pub(crate) fn shallow_diagnostics_to_macro_expansion(
    diags: &[ShallowDiagnostic],
    macro_index: usize,
    macro_kind: MacroExpansionKind,
    cache_suppress: bool,
) -> MacroExpansionDiagnostics {
    let exactness = if cache_suppress {
        ExpansionExactness::Incomplete
    } else {
        ExpansionExactness::ExactConcrete
    };
    let execution_status = if cache_suppress {
        ExpansionExecutionStatus::Interrupted
    } else {
        ExpansionExecutionStatus::Completed
    };
    MacroExpansionDiagnostics {
        macro_kind,
        macro_index,
        diagnostics: diags.iter().map(shallow_to_expansion).collect(),
        exactness,
        execution_status,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::semantic_query::DeclIdentity;
    use crate::semantic_query::SemanticNodeId;

    fn dummy_decl() -> DeclIdentity {
        DeclIdentity::synthetic("Dummy")
    }

    fn dummy_node() -> SemanticNodeId {
        SemanticNodeId(42)
    }

    #[test]
    fn shallow_to_expansion_maps_open_conditional_to_indeterminate_reason() {
        let diag = ShallowDiagnostic::OpenConditional { node: dummy_node() };
        let proj = shallow_to_expansion(&diag);
        assert_eq!(proj.reason, ExpansionStopReason::IndeterminateConditional);
        assert!(
            proj.context.contains("open-conditional"),
            "context must surface the variant name; observed `{}`",
            proj.context,
        );
    }

    #[test]
    fn shallow_to_expansion_maps_pathological_input_to_budget_exceeded() {
        let diag = ShallowDiagnostic::PathologicalInput { root: dummy_node() };
        let proj = shallow_to_expansion(&diag);
        assert_eq!(proj.reason, ExpansionStopReason::BudgetExceeded);
    }

    #[test]
    fn operational_limits_map_to_distinguishable_public_expansion_contexts() {
        let root = dummy_node();
        let work = shallow_to_expansion(&ShallowDiagnostic::ProjectionWorkLimit { root });
        let depth = shallow_to_expansion(&ShallowDiagnostic::ConnectedQueryDepthLimit { root });

        assert_eq!(work.reason, ExpansionStopReason::ProjectionWorkLimit);
        assert_eq!(depth.reason, ExpansionStopReason::ConnectedQueryDepthLimit);
        assert_eq!(work.context, "projection-work-limit@SemanticNodeId(42)");
        assert_eq!(
            depth.context,
            "connected-query-depth-limit@SemanticNodeId(42)"
        );
    }

    #[test]
    fn shallow_to_expansion_maps_cyclic_instantiation_to_dedicated_variant() {
        let diag = ShallowDiagnostic::CyclicInstantiation { decl: dummy_decl() };
        let proj = shallow_to_expansion(&diag);
        // Dedicated 1:1 mapping — was previously projected as
        // `UnresolvedReference` with substring inspection on `context`
        // to recover the cycle signal. The dedicated variant lets
        // downstream consumers route on the discriminator alone.
        assert_eq!(proj.reason, ExpansionStopReason::CyclicInstantiation);
        assert_eq!(proj.property_name.as_deref(), Some("Dummy"));
    }

    #[test]
    fn shallow_to_expansion_maps_duplicate_arm_to_idempotent_arm() {
        let diag = ShallowDiagnostic::DuplicateArmShortCircuited { node: dummy_node() };
        let proj = shallow_to_expansion(&diag);
        assert_eq!(proj.reason, ExpansionStopReason::IdempotentArm);
        assert!(proj.context.contains("duplicate-arm-short-circuited"));
    }

    #[test]
    fn shallow_to_expansion_maps_cycle_short_circuit_to_cyclic_reference() {
        let diag = ShallowDiagnostic::CycleShortCircuited { node: dummy_node() };
        let proj = shallow_to_expansion(&diag);
        assert_eq!(proj.reason, ExpansionStopReason::CyclicReference);
        assert!(proj.context.contains("cycle-short-circuited"));
    }

    #[test]
    fn shallow_to_expansion_maps_instantiation_error_to_dedicated_variant() {
        let diag = ShallowDiagnostic::InstantiationError {
            decl: dummy_decl(),
            error: crate::semantic_query::QueryError::Other(std::sync::Arc::from("synthetic")),
        };
        let proj = shallow_to_expansion(&diag);
        assert_eq!(proj.reason, ExpansionStopReason::InstantiationError);
        assert_eq!(proj.property_name.as_deref(), Some("Dummy"));
        assert!(proj.context.contains("instantiation-error"));
    }

    #[test]
    fn shallow_to_expansion_maps_union_arm_empty_to_dedicated_variant() {
        let diag = ShallowDiagnostic::UnionArmEmpty {
            union_node: dummy_node(),
            arm_index: 3,
        };
        let proj = shallow_to_expansion(&diag);
        // Was previously projected as `UnsupportedOperator` (wrong:
        // the arm is empty, not unsupported). Dedicated variant
        // disambiguates union-arm-empty from genuine unsupported
        // operators.
        assert_eq!(proj.reason, ExpansionStopReason::EmptyUnionArm);
        assert!(proj.context.contains("union-arm-empty"));
        assert!(proj.context.contains("#3"));
    }

    #[test]
    fn shallow_to_expansion_maps_unresolved_surface_arm_to_unresolved_reference() {
        let diag = ShallowDiagnostic::UnresolvedSurfaceArm {
            name: std::sync::Arc::from("NotFoundHeritage"),
            owner_canonical: std::sync::Arc::from("/src/types.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        };
        let proj = shallow_to_expansion(&diag);
        assert_eq!(proj.reason, ExpansionStopReason::UnresolvedReference);
        assert_eq!(proj.property_name.as_deref(), Some("NotFoundHeritage"));
        assert!(
            proj.context.contains("unresolved-surface-arm"),
            "context must surface the variant name; observed `{}`",
            proj.context,
        );
        assert!(
            proj.context.contains("/src/types.ts"),
            "context must carry the arm's declaring file; observed `{}`",
            proj.context,
        );
    }

    #[test]
    fn macro_expansion_envelope_carries_macro_index_and_per_diag_projection() {
        let diags = vec![
            ShallowDiagnostic::OpenConditional { node: dummy_node() },
            ShallowDiagnostic::PathologicalInput { root: dummy_node() },
        ];
        let env = shallow_diagnostics_to_macro_expansion(
            &diags,
            7,
            MacroExpansionKind::DefineProps,
            true,
        );
        assert_eq!(env.macro_index, 7);
        assert_eq!(env.diagnostics.len(), 2);
        assert_eq!(
            env.diagnostics[0].reason,
            ExpansionStopReason::IndeterminateConditional
        );
        assert_eq!(
            env.diagnostics[1].reason,
            ExpansionStopReason::BudgetExceeded
        );
        assert_eq!(env.exactness, ExpansionExactness::Incomplete);
        assert_eq!(env.execution_status, ExpansionExecutionStatus::Interrupted);
    }

    #[test]
    fn macro_expansion_envelope_marks_exact_when_no_suppression() {
        let env =
            shallow_diagnostics_to_macro_expansion(&[], 0, MacroExpansionKind::DefineEmits, false);
        assert_eq!(env.exactness, ExpansionExactness::ExactConcrete);
        assert_eq!(env.execution_status, ExpansionExecutionStatus::Completed);
    }
}
