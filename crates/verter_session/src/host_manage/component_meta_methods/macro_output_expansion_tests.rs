//! Parity tests for the MODULE-PRIVATE node-domain expansion artifact +
//! materialisation sink ([`AdmittedExpansionNode`] +
//! [`materialize_admitted_expansion_node`]) and the sink-owned demand API.
//!
//! The sink is the SINGLE place a node-domain expansion artifact becomes an
//! `ExpandedNormalizedExpr`, materialising via the authorized-owner
//! `HostManageComponentMetaOutputCap`. The artifact and materialiser are
//! MODULE-PRIVATE to `macro_output_expansion`: no module outside it can name,
//! construct, or materialise them — the only crate-visible entrances are the
//! closed-demand methods (resolver ctx + owner canonical + macro index + the
//! per-branch terminal demand — never a raw node). This module is a `#[cfg(test)]`
//! submodule of `macro_output_expansion`, so it can reach the module-private
//! items to pin the sink's byte-equality against the shell-raise ORACLE
//! (`materialize_output_type_expr_for_test`, the sealed `OutputProjector` shell
//! raise the sink itself routes through) — and that the `None`-miss arm matches
//! the oracle's `None` arm.
//!
//! Of the three sink-owned demand methods, only [`expand_define_model_output`]
//! is pinned DIRECTLY here — byte-equal to the shell-raise oracle of its
//! surfaced produced node — proving the demand method drives a real internal
//! resolution and reproduces the former node-bearing path exactly.
//! [`expand_generic_project_path_output`] and [`expand_slot_binding_output`] are
//! exercised end-to-end through their `eval_env` callers plus the corpus gate
//! (not re-pinned here with a heavy direct unit test); this file's direct
//! coverage is the artifact/sink parity plus the `defineModel` demand round-trip.

use std::sync::Arc;

use verter_type_expr::{PrimitiveName, TypeExpr};

use super::{materialize_admitted_expansion_node, AdmittedExpansionNode};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{DepVersion, PrimitiveKind, SemanticNodeData, SemanticNodeId};
use crate::VerterHost;

/// The shell-raise ORACLE for `node` — the `#[cfg(test)]` materialization
/// mirror (`materialize_output_type_expr_for_test`, the sealed `OutputProjector`
/// shell raise the sink itself routes through). The sink must materialise
/// byte-equal to this, so a node-domain expansion caller that routes the sink
/// instead of a mid-flight raise is behaviour-preserving.
fn shell_raise_oracle(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<TypeExpr> {
    dispatch.materialize_output_type_expr_for_test(node)
}

#[test]
fn sink_materializes_node_bearing_artifact_byte_equal_to_shell_raise_oracle() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let dispatch = ProjectSemanticDispatch::new(&host);

    // A representative carrier node (a bare-ref) — the kind the expansion branch
    // produces. The sink must materialise it to the SAME `TypeExpr` the
    // shell-raise oracle would, wrapped in `ExpandedNormalizedExpr`.
    let node = graph.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("ModelValue"),
        crate::semantic_query::NodeScopeId::Global,
        Arc::from(Vec::new().into_boxed_slice()),
    ));

    let artifact =
        AdmittedExpansionNode::new(node, Arc::from(Vec::new().into_boxed_slice()), false);
    let via_sink = materialize_admitted_expansion_node(&dispatch, &artifact)
        .expect("sink must materialise a raisable node-bearing artifact");
    let via_oracle = shell_raise_oracle(&dispatch, node).expect("oracle must raise the same node");

    assert_eq!(
        via_sink.expr, via_oracle,
        "the sink's ExpandedNormalizedExpr.expr must be BYTE-EQUAL to the shell-raise oracle for \
         the same node (so routing the sink instead of a mid-flight raise preserves behaviour)"
    );
    assert!(
        matches!(&via_sink.expr, TypeExpr::Ref { name, .. } if name.as_ref() == "ModelValue"),
        "the bare-ref carrier materialises to Ref{{name=ModelValue}}, got {:?}",
        via_sink.expr
    );
}

#[test]
fn sink_matches_shell_raise_oracle_across_carrier_shapes() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let dispatch = ProjectSemanticDispatch::new(&host);

    // A spread of node shapes the expansion branch can produce; for each, the
    // sink's materialised expr must equal the shell-raise oracle EXACTLY.
    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from(
        vec![string_id, number_id].into_boxed_slice(),
    )));
    let array = graph.intern_node(SemanticNodeData::Array {
        element: string_id,
        readonly: false,
    });
    let raw = graph.intern_node(SemanticNodeData::RawFallback {
        raw: Arc::from("SomeText"),
    });

    for (label, node) in [
        ("primitive", string_id),
        ("union", union),
        ("array", array),
        ("raw-fallback", raw),
    ] {
        let artifact =
            AdmittedExpansionNode::new(node, Arc::from(Vec::new().into_boxed_slice()), false);
        let via_sink = materialize_admitted_expansion_node(&dispatch, &artifact).map(|e| e.expr);
        let via_oracle = shell_raise_oracle(&dispatch, node);
        assert_eq!(
            via_sink, via_oracle,
            "[{label}] sink materialisation must equal the shell-raise oracle"
        );
    }
}

#[test]
fn sink_none_miss_matches_oracle_none() {
    // A node ABSENT from the graph store makes BOTH the sink and the oracle
    // return `None` — the sink's miss arm mirrors the oracle's `None` arm
    // (where the eval_env caller falls back to its `parsed` expr).
    let host = VerterHost::new_standalone(Default::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let absent = SemanticNodeId(u64::MAX);

    assert!(
        shell_raise_oracle(&dispatch, absent).is_none(),
        "the oracle raises an absent node to None"
    );
    let artifact =
        AdmittedExpansionNode::new(absent, Arc::from(Vec::new().into_boxed_slice()), false);
    assert!(
        materialize_admitted_expansion_node(&dispatch, &artifact).is_none(),
        "the sink must return None for an absent node, mirroring the oracle's None arm"
    );
}

#[test]
fn node_bearing_artifact_preserves_node_and_metadata() {
    // The artifact carries the node + cache metadata in node-domain (no
    // `TypeExpr`). This pins the constructor stores the fields a node-domain
    // expansion caller folds into `fact_versions` / the admission gate. The
    // `dep_signature` is built NON-EMPTY and its full content asserted, so a
    // constructor that DROPS / zeroes `dep_signature` (rather than storing the
    // arg verbatim) FAILS — an empty-signature round-trip would not discriminate
    // a field-dropping ctor.
    let dep: crate::semantic_query::DepSignature = Arc::from(vec![(
        Arc::<str>::from("dep-a"),
        DepVersion::WholeHash([7u8; 16]),
    )]);
    let artifact = AdmittedExpansionNode::new(SemanticNodeId(7), Arc::clone(&dep), true);
    assert_eq!(artifact.node, SemanticNodeId(7), "node round-trips");
    assert!(artifact.result_is_partial, "partial flag round-trips");
    assert_eq!(
        artifact.dep_signature.len(),
        1,
        "dep signature round-trips its entries"
    );
    assert_eq!(
        artifact.dep_signature[0].0.as_ref(),
        "dep-a",
        "dep entry name round-trips"
    );
    assert_eq!(
        artifact.dep_signature[0].1,
        DepVersion::WholeHash([7u8; 16]),
        "dep entry version round-trips"
    );

    // Discrimination: a Primitive node materialises to exactly its primitive
    // expr through the sink (proves the sink is not a constant / stub).
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let s = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let s_artifact = AdmittedExpansionNode::new(s, Arc::from(Vec::new().into_boxed_slice()), false);
    let expr = materialize_admitted_expansion_node(&dispatch, &s_artifact)
        .expect("primitive materialises")
        .expr;
    assert_eq!(
        expr,
        TypeExpr::Primitive(PrimitiveName::Boolean),
        "the sink materialises a Boolean primitive node to TypeExpr::Primitive(Boolean)"
    );
}

/// The sink-owned demand API is the ONLY crate-visible entrance: a closed demand
/// (resolver ctx + owner canonical + macro index) in, never a raw node. This test
/// drives the real `defineModel<T>()` branch through `expand_define_model_output`
/// over an indexed SFC fixture and proves: (1) it resolves the macro-argument
/// carrier head INTERNALLY and materialises a `Materialized` outcome (the demand
/// method is not a stub); (2) the materialised `normalized.expr` is BYTE-EQUAL to
/// the shell-raise oracle of the surfaced `produced_node_id` — i.e. the demand
/// method reproduces the former node-bearing path EXACTLY (resolve the same node,
/// materialise it the same way at the sealed sink), with audit-parity
/// `produced_node_id` surfaced to the caller.
#[test]
fn demand_api_define_model_materializes_byte_equal_to_oracle_of_produced_node() {
    use super::expand_define_model_output;
    use super::DefineModelOutputExpansion;

    let host = VerterHost::new_standalone(crate::types::HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..Default::default()
    });
    let _ = host
        .upsert(crate::types::UpsertRequest {
            canonical_id: None,
            input_id: "/Model.vue".to_string(),
            source: Arc::from(
                r#"<script setup lang="ts">
type ModelValue = { a: string; b: number }
const model = defineModel<ModelValue>()
</script>
<template><div /></template>"#,
            ),
            file_language: crate::types::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert should succeed");

    let snapshot = host
        .get_raw_analysis_snapshot("/Model.vue")
        .expect("indexed snapshot for the defineModel SFC");
    let macro_index = snapshot
        .macros
        .iter()
        .position(|m| m.kind == verter_semantic::analysis::AnalyzedMacroKind::DefineModel)
        .expect("the SFC declares a defineModel macro");

    // Closed-demand entrance: ctx (`&host`) + owner canonical + macro index — no
    // node crosses in. The sink resolves the carrier head internally.
    let outcome = expand_define_model_output(&host, "/Model.vue", macro_index);
    let DefineModelOutputExpansion::Materialized {
        produced_node_id,
        normalized,
    } = outcome
    else {
        panic!(
            "expand_define_model_output must Materialize the defineModel<ModelValue> carrier head, \
             got a non-Materialized outcome"
        );
    };

    // The materialised expr must be BYTE-EQUAL to the shell-raise oracle of the
    // SURFACED produced node id — the former node-bearing path set
    // `produced_node_id = Some(node)` then materialised exactly that node, so a
    // faithful demand method materialises the SAME node the SAME way.
    let dispatch = ProjectSemanticDispatch::new(&host);
    let via_oracle = shell_raise_oracle(&dispatch, produced_node_id)
        .expect("the oracle raises the produced carrier-head node");
    assert_eq!(
        normalized.expr, via_oracle,
        "the demand method's materialised expr must equal the shell-raise oracle of the surfaced \
         produced_node_id (byte-identical to the former node-bearing path)"
    );

    // Discrimination: the resolved model value type is the structural object, not
    // a constant/opaque — proves the demand method drove a real internal
    // resolution rather than returning a fixed expr.
    assert!(
        !matches!(normalized.expr, TypeExpr::Unknown { .. }),
        "the defineModel<ModelValue> carrier head resolves to a real type, not Unknown; got {:?}",
        normalized.expr
    );
}

/// An out-of-range macro index (no macro) makes the carrier hot-ref producer
/// miss, so the demand method returns `CarrierMiss` — proving the demand method
/// threads through the real carrier producer (a stub returning a constant outcome
/// would not discriminate the miss).
#[test]
fn demand_api_define_model_carrier_miss_on_absent_macro() {
    use super::expand_define_model_output;
    use super::DefineModelOutputExpansion;

    let host = VerterHost::new_standalone(crate::types::HostConfig {
        analysis_level: crate::types::AnalysisLevel::Full,
        ..Default::default()
    });
    let _ = host
        .upsert(crate::types::UpsertRequest {
            canonical_id: None,
            input_id: "/Empty.vue".to_string(),
            source: Arc::from(
                r#"<script setup lang="ts">
const x = 1
</script>
<template><div /></template>"#,
            ),
            file_language: crate::types::FileLanguage::vue(),
            aliases: Vec::new(),
        })
        .expect("upsert should succeed");
    // Force-index so the producer's cell table is sized (an out-of-range index
    // still returns `None`, never grows the table).
    let _ = host.get_raw_analysis_snapshot("/Empty.vue");

    let outcome = expand_define_model_output(&host, "/Empty.vue", 9999);
    assert!(
        matches!(outcome, DefineModelOutputExpansion::CarrierMiss),
        "an absent macro index makes the carrier hot-ref producer miss → CarrierMiss"
    );
}
