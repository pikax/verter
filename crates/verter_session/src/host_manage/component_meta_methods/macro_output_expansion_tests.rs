//! Parity tests for the MODULE-PRIVATE node-domain expansion artifact +
//! materialisation sink ([`AdmittedExpansionNode`] +
//! [`materialize_admitted_expansion_node`]) and the sink-owned demand API.
//!
//! The sink is the SINGLE place a node-domain expansion artifact becomes an
//! `ExpandedNormalizedExpr` by publishing a content-free semantic source. The
//! artifact and materialiser are
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

use verter_type_expr::facts::{ClosedTypeFact, LeafTypeFact, SemanticTypeSource};
use verter_type_expr::{PrimitiveName, TypeExpr};

use super::{materialize_admitted_expansion_node, AdmittedExpansionNode};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{DepVersion, PrimitiveKind, SemanticNodeData, SemanticNodeId};
use crate::VerterHost;

/// A caller-side fallback source clearly DISTINCT from every leaf fact the
/// sink can project, so an assertion on the sink output discriminates
/// "projected the resolved leaf" from "published the caller's fallback".
fn distinct_fallback_source() -> SemanticTypeSource {
    SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Ref(
        "FallbackCarrier".to_string(),
    )))
}

/// The shell-raise ORACLE for `node` — the `#[cfg(test)]` materialization
/// mirror (`materialize_output_type_expr_for_test`, the sealed `OutputProjector`
/// shell raise). Used to CONFIRM a node's resolved shape while asserting the
/// sink's content-free SOURCE projection.
fn shell_raise_oracle(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<TypeExpr> {
    dispatch.materialize_output_type_expr_for_test(node)
}

/// The sink publishes a content-free SOURCE: a resolved LEAF node projects to
/// its complete closed leaf fact; a NON-leaf resolved node preserves the
/// caller's `fallback_source` verbatim (the demand side re-raises it through the
/// one engine). A bare-ref carrier is NOT a leaf, so the sink preserves the
/// distinct fallback rather than materialising the ref.
///
/// Discriminating: a sink that materialised the node (the retired byte-parity
/// behaviour) would publish a `Closed(Leaf(Ref("ModelValue")))` / ref source, not
/// the DISTINCT `FallbackCarrier` fallback — the equality below would fail.
#[test]
fn sink_preserves_fallback_source_for_non_leaf_carrier_node() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let dispatch = ProjectSemanticDispatch::new(&host);

    // A representative carrier node (a bare-ref) — the kind the expansion branch
    // produces. It is a NON-leaf, so the sink preserves the fallback source.
    let node = graph.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("ModelValue"),
        crate::semantic_query::NodeScopeId::Global,
        Arc::from(Vec::new().into_boxed_slice()),
    ));
    // Confirm the node resolves to a bare ref (fixture premise).
    assert!(
        matches!(shell_raise_oracle(&dispatch, node), Some(TypeExpr::Ref { ref name, .. }) if name.as_ref() == "ModelValue"),
        "fixture premise: the carrier node resolves to Ref{{name=ModelValue}}",
    );

    let fallback = distinct_fallback_source();
    let artifact =
        AdmittedExpansionNode::new(node, Arc::from(Vec::new().into_boxed_slice()), false);
    let via_sink = materialize_admitted_expansion_node(&dispatch, &artifact, &fallback)
        .expect("sink must publish a source for a raisable node-bearing artifact");

    assert_eq!(
        via_sink.expr, fallback,
        "a NON-leaf carrier node preserves the caller's fallback source verbatim \
         (the sink projects a content-free SOURCE, it does NOT materialise the node)"
    );
}

/// Per carrier shape: a resolved LEAF node projects to its complete closed
/// leaf fact, and a resolved UNION whose every member is a complete leaf
/// projects to the closed ORDERED leaf-union fact; every richer resolved
/// shape (a union with a non-leaf arm / array / raw-fallback) preserves the
/// caller's `fallback_source`.
///
/// Discriminating: the retired sink materialised each shape to a distinct
/// `TypeExpr`; the new sink projects the closed fact only for the complete
/// leaf/leaf-union shapes and preserves the fallback for the rest — a
/// materialising sink would fail both the closed-fact and the fallback
/// assertions, and a sink that still degraded the leaf union to the fallback
/// would fail the leaf-union assertion.
#[test]
fn sink_projects_leaf_fact_or_preserves_fallback_across_carrier_shapes() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let dispatch = ProjectSemanticDispatch::new(&host);

    let string_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number_id = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let union = graph.intern_node(SemanticNodeData::Union(Arc::from(
        vec![string_id, number_id].into_boxed_slice(),
    )));
    let array = graph.intern_node(SemanticNodeData::Array {
        element: string_id,
        readonly: false,
    });
    let mixed_union = graph.intern_node(SemanticNodeData::Union(Arc::from(
        vec![string_id, array].into_boxed_slice(),
    )));
    let raw = graph.intern_node(SemanticNodeData::RawFallback {
        raw: Arc::from("SomeText"),
    });

    let fallback = distinct_fallback_source();

    // The LEAF node projects its complete closed leaf fact.
    let leaf_artifact =
        AdmittedExpansionNode::new(string_id, Arc::from(Vec::new().into_boxed_slice()), false);
    let leaf_out = materialize_admitted_expansion_node(&dispatch, &leaf_artifact, &fallback)
        .expect("a leaf node publishes a source");
    assert_eq!(
        leaf_out.expr,
        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
            PrimitiveName::String
        ))),
        "a resolved primitive-string leaf node projects to the closed String leaf fact",
    );

    // A UNION whose every member is a complete leaf projects to the closed
    // ORDERED leaf-union fact — never the fallback (the union is a decided
    // closed result, e.g. an instantiated `string | number` payload param).
    let union_artifact =
        AdmittedExpansionNode::new(union, Arc::from(Vec::new().into_boxed_slice()), false);
    let union_out = materialize_admitted_expansion_node(&dispatch, &union_artifact, &fallback)
        .expect("a leaf-union node publishes a source");
    assert_eq!(
        union_out.expr,
        SemanticTypeSource::Closed(ClosedTypeFact::LeafUnion(Arc::from(
            vec![
                LeafTypeFact::Primitive(PrimitiveName::String),
                LeafTypeFact::Primitive(PrimitiveName::Number),
            ]
            .into_boxed_slice(),
        ))),
        "a resolved union of complete leaves projects to the ordered closed leaf-union fact",
    );

    // Every RICHER resolved shape preserves the caller's fallback verbatim —
    // including a union with a non-leaf arm (a partial fact is never
    // published).
    for (label, node) in [
        ("mixed-union", mixed_union),
        ("array", array),
        ("raw-fallback", raw),
    ] {
        let artifact =
            AdmittedExpansionNode::new(node, Arc::from(Vec::new().into_boxed_slice()), false);
        let out = materialize_admitted_expansion_node(&dispatch, &artifact, &fallback)
            .expect("a non-closed resolved node publishes a source");
        assert_eq!(
            out.expr, fallback,
            "[{label}] a resolved shape with no complete closed fact preserves the caller's \
             fallback source",
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
        materialize_admitted_expansion_node(&dispatch, &artifact, &distinct_fallback_source())
            .is_none(),
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

    // Discrimination: a Primitive node projects to exactly its complete
    // closed leaf fact through the sink — NOT the caller's fallback and not a
    // constant (proves the sink is not a constant / stub).
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let s = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let s_artifact = AdmittedExpansionNode::new(s, Arc::from(Vec::new().into_boxed_slice()), false);
    let expr =
        materialize_admitted_expansion_node(&dispatch, &s_artifact, &distinct_fallback_source())
            .expect("primitive materialises")
            .expr;
    assert_eq!(
        expr,
        SemanticTypeSource::Closed(ClosedTypeFact::Leaf(LeafTypeFact::Primitive(
            PrimitiveName::Boolean,
        ))),
        "the sink projects a Boolean primitive node to its complete closed Boolean leaf fact"
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

    // The model's fallback SOURCE is its own T — the macro type-argument
    // payload position, exactly what the eval_env `defineModel` branch passes.
    let model_fallback = snapshot.macros[macro_index]
        .parsed_type_argument
        .as_ref()
        .map(|locator| {
            SemanticTypeSource::Authored(
                verter_type_expr::locators::AuthoredBodyLocator::MacroPayload(locator.clone()),
            )
        })
        .unwrap_or_else(|| {
            panic!("defineModel<ModelValue> carries a parsed type-argument payload")
        });

    // Closed-demand entrance: ctx (`&host`) + owner canonical + macro index +
    // the authored fallback source — no node crosses in. The sink resolves the
    // carrier head internally.
    let outcome = expand_define_model_output(&host, "/Model.vue", macro_index, &model_fallback);
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

    // Publication demand is Navigate-only (the
    // `publication_routes_never_demand_expanded` guard pins ZERO
    // `Published(Expanded)` contexts during publication), so the produced node
    // is the model type's resolved CARRIER head — pin its identity
    // structurally, then pin that the published source demand-materialises
    // BYTE-EQUAL to the demand of that same identity through the one engine
    // (the parity the former Expanded-time shell-raise oracle pinned).
    let produced_data = crate::project_semantic_dispatch::node_data_for(&host, produced_node_id)
        .expect("the produced carrier-head node is present in the graph");
    let crate::semantic_query::SemanticNodeData::DeclRef { identity } = produced_data.as_ref()
    else {
        panic!(
            "the Navigate-published defineModel carrier head must be the model type's \
             DeclRef identity carrier, got {produced_data:?}"
        );
    };
    assert_eq!(
        identity.decl_name.as_ref(),
        "ModelValue",
        "the produced carrier head resolves the macro's own type argument"
    );
    let via_oracle = crate::test_only::semantic_source_probe::demand_type_expr(
        &host,
        "/Model.vue",
        &SemanticTypeSource::Synthesized(verter_type_expr::facts::ResolvedLocalShape::Ref(
            verter_type_expr::locators::SymbolBodyLocator {
                anchor: verter_type_expr::locators::AuthoredAnchor {
                    canonical_id: Arc::clone(&identity.canonical_id),
                    owner: identity.owner,
                    symbol: Arc::clone(&identity.decl_name),
                    space: verter_type_expr::locators::LocatorSymbolSpace::Type,
                },
            },
        )),
    )
    .expect("the produced carrier identity demand-materializes");
    let via_demand = crate::test_only::semantic_source_probe::demand_type_expr(
        &host,
        "/Model.vue",
        &normalized.expr,
    )
    .unwrap_or_else(|| panic!("the demand method's published source must demand-materialize"));
    assert_eq!(
        via_demand, via_oracle,
        "the demand method's published source must demand-materialise to the demand of the \
         surfaced produced_node_id's identity (the same node through the one engine)"
    );

    // Discrimination: the resolved model value type is the structural object, not
    // a constant/opaque — proves the demand method drove a real internal
    // resolution rather than returning a fixed expr.
    assert!(
        !matches!(via_demand, TypeExpr::Unknown { .. }),
        "the defineModel<ModelValue> carrier head resolves to a real type, not Unknown; got {via_demand:?}",
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

    let outcome =
        expand_define_model_output(&host, "/Empty.vue", 9999, &distinct_fallback_source());
    assert!(
        matches!(outcome, DefineModelOutputExpansion::CarrierMiss),
        "an absent macro index makes the carrier hot-ref producer miss → CarrierMiss"
    );
}
