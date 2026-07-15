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
        matches!(&raised, TypeExpr::Unknown { raw } if raw == "semanticMiss"),
        "the sealed output seam must spell the typed miss as the raw sentinel, got {raised:?}"
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

    let missy_input = ref_indexed_access(TypeExpr::Unknown {
        raw: "semanticMiss".to_string(),
    });
    let missy_node = dispatch
        .lower_type_expr_in_scope_with_context("/p.ts", &missy_input, transit)
        .expect("the miss-carrying input lowers");
    assert_eq!(
        node_contains_semantic_miss_with_dispatch(&dispatch, missy_node),
        Some(true),
        "an input whose object position is the miss leaf must classify Some(true)"
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
