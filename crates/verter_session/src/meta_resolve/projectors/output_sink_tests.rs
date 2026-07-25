//! Typed-degradation + no-poison coverage for the publication reducer:
//! the sealed output seam alone still spells a raw sentinel string, the
//! input-side gate classifies the INPUT through the node-domain
//! whole-tree miss fact rather than a raised-string walk, and a clean
//! input whose demanded reduction cannot materialise stays the
//! re-resolvable input carrier (never a fabricated root-sentinel shape,
//! never a warm admission).

use std::sync::Arc as StdArc;

use super::reduce_field_value_node;
use crate::meta::MetaProject;
use crate::project_semantic_dispatch::raise::{
    node_contains_semantic_miss_with_dispatch, node_root_is_unmaterialized_sentinel_with_dispatch,
};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::resolver_core::ResolverContext;
use crate::semantic_query::{
    ProjectionMode, ProjectionReductionContext, QueryError, SemanticNodeData,
};
use crate::types::{AnalysisLevel, HostConfig};
use crate::VerterHost;
use verter_type_expr::TypeExpr;

fn ref_indexed_access(object: TypeExpr) -> TypeExpr {
    TypeExpr::IndexedAccess {
        object: StdArc::new(object),
        index: StdArc::new(TypeExpr::string_literal("k")),
    }
}

fn missing_ref() -> TypeExpr {
    TypeExpr::Ref {
        name: StdArc::from("DefinitelyMissingType"),
        type_arguments: StdArc::from(Vec::new().into_boxed_slice()),
    }
}

/// The sealed output seam is UNCHANGED: a typed `Opaque(QueryError::Miss)`
/// carrier still materialises to the byte-identical raw sentinel
/// `Unknown { raw: "semanticMiss" }` at the output boundary — the string
/// spelling survives ONLY there, not as a control channel.
#[test]
fn sealed_output_seam_still_emits_unknown_raw_for_typed_miss() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = StdArc::clone(host.project_type_store().semantic_graph());
    let miss = graph.intern_node(SemanticNodeData::Opaque(QueryError::Miss));
    let dispatch = ProjectSemanticDispatch::new(&host);

    let raised = dispatch
        .materialize_output_type_expr_for_test(miss)
        .expect("the typed miss carrier raises through the sealed output seam");
    assert!(
        matches!(&raised, TypeExpr::Unknown(value) if value.raw() == "semanticMiss"),
        "the sealed output seam must spell the typed miss as the compat projection, got {raised:?}"
    );
}

/// The publication reducer's input-side no-poison gate decides on TYPED
/// node-domain state: the whole-tree miss fact
/// (`node_contains_semantic_miss_with_dispatch`) is read off the SAME
/// observed input node the reducer received — `Some(false)` for a clean
/// operator input, `Some(true)` for an input whose object position
/// carries the miss leaf. End-to-end, a clean input whose demanded
/// reduction cannot materialise never publishes a root-sentinel shape:
/// the published carrier stays the re-resolvable input node (the
/// no-poison contract), and the returned carrier is a plain
/// return-only value — nothing here admits it warm.
#[test]
fn input_side_no_poison_gate_reads_typed_whole_tree_miss_fact() {
    let host = VerterHost::new_standalone(HostConfig {
        analysis_level: AnalysisLevel::Full,
        ..HostConfig::default()
    });
    let project = MetaProject::new(host);
    project
        .upsert_base("/p.ts", "export type Anchor = number\n")
        .unwrap();
    let host = project.host();
    let _store_view = host.resolver_store_view_read().into_owned_view();
    let ctx: &dyn ResolverContext = host;
    let dispatch = ProjectSemanticDispatch::new(ctx);
    let transit =
        ProjectionReductionContext::structural_transit_with_mode(ProjectionMode::Navigate);

    // Typed decision surface: the clean input classifies CONFIDENTLY
    // miss-free; the miss-carrying input classifies as carrying the miss.
    let clean_input = ref_indexed_access(missing_ref());
    let clean_node = dispatch
        .lower_type_expr_in_scope_with_context("/p.ts", &clean_input, transit)
        .expect("the clean operator input lowers");
    assert_eq!(
        node_contains_semantic_miss_with_dispatch(&dispatch, clean_node),
        Some(false),
        "a clean operator input must classify Some(false) (confidently miss-free)"
    );

    // A lowered genuine `UnknownValue` (spelled `semanticMiss`) carries NO
    // classification of its own — but demanding a projection of it cannot
    // succeed, so the locator-view worklist converts the `RawFallback` to a
    // TYPED `Opaque(QueryError::Miss)` and the whole-tree fact reads the miss
    // off that typed carrier (never off the spelling).
    let missy_input = ref_indexed_access(TypeExpr::Unknown(
        verter_type_expr::UnknownValue::unsupported_syntax("semanticMiss"),
    ));
    let missy_node = dispatch
        .lower_type_expr_in_scope_with_context("/p.ts", &missy_input, transit)
        .expect("the miss-carrying input lowers");
    assert_eq!(
        node_contains_semantic_miss_with_dispatch(&dispatch, missy_node),
        Some(true),
        "an input whose object position degrades to a TYPED Miss must classify Some(true)"
    );

    // End-to-end no-poison off the NODE-start reducer: reducing the clean
    // input node (whose demanded reduction cannot materialise — the
    // referenced type does not exist) publishes the INPUT node back as
    // the carrier, never a root-sentinel shape.
    let mut query_engine = crate::resolver_core::ComponentMetaQueryEngine::new(host);
    let carrier = reduce_field_value_node(
        &mut query_engine,
        "/p.ts",
        clean_node,
        ProjectionMode::Navigate,
    );
    let published_node = carrier
        .node_id()
        .expect("the no-poison publish carries the observed input node");
    assert!(
        !node_root_is_unmaterialized_sentinel_with_dispatch(&dispatch, published_node),
        "a clean input must never publish a root-sentinel shape (no-poison)"
    );
}

/// R4-F3 — the `raise_node_to_sealed_carrier` None-arm split is pinned BOTH
/// ways via real graph fixtures: a PRESENT-but-unraisable composite degrades
/// to a typed `Miss` (partial, `semanticMiss` projection); a GENUINELY-absent
/// id stays the exact `missing_output` (non-partial). An arm-swap reopens the
/// laundering and fails here.
#[test]
fn sealed_carrier_none_arm_splits_unraisable_failure_from_genuine_absence() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = StdArc::clone(host.project_type_store().semantic_graph());
    let str_id = graph.intern_node(SemanticNodeData::Primitive(
        crate::semantic_query::PrimitiveKind::String,
    ));
    let dispatch = ProjectSemanticDispatch::new(&host);

    // (1) PRESENT-BUT-UNRAISABLE: the union node mints but a member id is
    // absent — degrades to the typed `Miss` carrier (partial; `semanticMiss`
    // compat projection), and the torn read notes `OutputMaterializationLoss`
    // (NON-CACHEABLE) on the admission rail.
    let unraisable = graph.intern_node(SemanticNodeData::Union(StdArc::from(
        vec![str_id, crate::semantic_query::SemanticNodeId(u64::MAX)].into_boxed_slice(),
    )));
    let (_carrier, facts) = host.with_fact_tracer(|| {
        let carrier = super::raise_node_to_sealed_carrier(
            &dispatch,
            unraisable,
            crate::semantic_query::DepSignature::default(),
        );
        assert!(
            carrier.result_is_partial(),
            "a present-but-unraisable composite must degrade PARTIAL, never admitted complete"
        );
    });
    assert!(
        matches!(
            facts.finalise(),
            crate::resolver_core::FactReadSetFinalise::NonCacheable(_)
        ),
        "the unraisable arm must finalise NON-CACHEABLE on the loss rail"
    );

    // (2) GENUINELY-ABSENT: an id with no arena entry is a real absence —
    // the exact `missing_output`, NON-partial.
    let absent = crate::semantic_query::SemanticNodeId(u64::MAX);
    let carrier = super::raise_node_to_sealed_carrier(
        &dispatch,
        absent,
        crate::semantic_query::DepSignature::default(),
    );
    assert!(
        !carrier.result_is_partial(),
        "a genuinely-absent id stays exact + non-partial"
    );
}
