//! Parity tests for the node-bearing expansion artifact + materialisation
//! facade ([`NodeBearingExpansion`] + [`materialize_node_bearing_expansion`]).
//!
//! The facade is the SINGLE place a node-domain expansion artifact becomes an
//! `ExpandedNormalizedExpr`, materialising at the sink via the authorized-owner
//! `HostManageComponentMetaOutputCap`. These tests prove the facade produces
//! BYTE-EQUAL output to the interim Kind-B bridge
//! (`legacy_semantic_type_expr_bridge`) for the same node — so a node-domain
//! expansion caller that routes the facade instead of the bridge is
//! behaviour-preserving — and that the `None`-miss arm matches the bridge's
//! `None` arm.
//!
//! These tests exercise the facade directly; they do NOT wire it into the
//! eval_env raises (those still route the interim bridge in this tree).

use std::sync::Arc;

use verter_type_expr::{PrimitiveName, TypeExpr};

use super::{materialize_node_bearing_expansion, NodeBearingExpansion};
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{PrimitiveKind, SemanticNodeData, SemanticNodeId};
use crate::VerterHost;

/// The interim bridge's raise for `node` — the parity ORACLE the facade must
/// match. `legacy_semantic_type_expr_bridge` is the sanctioned Kind-B path the
/// eval_env raises currently use.
fn bridge_raise(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> Option<TypeExpr> {
    dispatch.legacy_semantic_type_expr_bridge(node)
}

#[test]
fn facade_materializes_node_bearing_artifact_byte_equal_to_bridge() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let dispatch = ProjectSemanticDispatch::new(&host);

    // A representative carrier node (a bare-ref) — the kind the expansion branch
    // produces. The facade must materialise it to the SAME `TypeExpr` the bridge
    // would, wrapped in `ExpandedNormalizedExpr`.
    let node = graph.intern_node(SemanticNodeData::new_bare_ref(
        Arc::from("ModelValue"),
        crate::semantic_query::NodeScopeId::Global,
        Arc::from(Vec::new().into_boxed_slice()),
    ));

    let artifact = NodeBearingExpansion::new(node, Arc::from(Vec::new().into_boxed_slice()), false);
    let via_facade = materialize_node_bearing_expansion(&dispatch, &artifact)
        .expect("facade must materialise a raisable node-bearing artifact");
    let via_bridge = bridge_raise(&dispatch, node).expect("bridge must raise the same node");

    assert_eq!(
        via_facade.expr, via_bridge,
        "the facade's ExpandedNormalizedExpr.expr must be BYTE-EQUAL to the bridge's raise for the \
         same node (so routing the facade instead of the bridge preserves behaviour)"
    );
    assert!(
        matches!(&via_facade.expr, TypeExpr::Ref { name, .. } if name.as_ref() == "ModelValue"),
        "the bare-ref carrier materialises to Ref{{name=ModelValue}}, got {:?}",
        via_facade.expr
    );
}

#[test]
fn facade_matches_bridge_across_carrier_shapes() {
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let dispatch = ProjectSemanticDispatch::new(&host);

    // A spread of node shapes the expansion branch can produce; for each, the
    // facade's materialised expr must equal the bridge's raise EXACTLY.
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
            NodeBearingExpansion::new(node, Arc::from(Vec::new().into_boxed_slice()), false);
        let via_facade = materialize_node_bearing_expansion(&dispatch, &artifact).map(|e| e.expr);
        let via_bridge = bridge_raise(&dispatch, node);
        assert_eq!(
            via_facade, via_bridge,
            "[{label}] facade materialisation must equal the bridge raise"
        );
    }
}

#[test]
fn facade_none_miss_matches_bridge_none() {
    // A node ABSENT from the graph store makes BOTH the facade and the bridge
    // return `None` — the facade's miss arm mirrors the bridge's `None` arm
    // (where the eval_env caller falls back to its `parsed` expr).
    let host = VerterHost::new_standalone(Default::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let absent = SemanticNodeId(u64::MAX);

    assert!(
        bridge_raise(&dispatch, absent).is_none(),
        "the bridge raises an absent node to None"
    );
    let artifact =
        NodeBearingExpansion::new(absent, Arc::from(Vec::new().into_boxed_slice()), false);
    assert!(
        materialize_node_bearing_expansion(&dispatch, &artifact).is_none(),
        "the facade must return None for an absent node, mirroring the bridge's None arm"
    );
}

#[test]
fn node_bearing_artifact_preserves_node_and_metadata() {
    // The artifact carries the node + cache metadata in node-domain (no
    // `TypeExpr`). This pins the constructor stores the fields a node-domain
    // expansion caller folds into `fact_versions` / the admission gate.
    let dep: crate::semantic_query::DepSignature = Arc::from(Vec::new().into_boxed_slice());
    let artifact = NodeBearingExpansion::new(SemanticNodeId(7), Arc::clone(&dep), true);
    assert_eq!(artifact.node, SemanticNodeId(7), "node round-trips");
    assert!(artifact.result_is_partial, "partial flag round-trips");
    assert_eq!(artifact.dep_signature.len(), 0, "dep signature round-trips");

    // Discrimination: a Primitive node materialises to exactly its primitive
    // expr through the facade (proves the facade is not a constant / stub).
    let host = VerterHost::new_standalone(Default::default());
    let graph = Arc::clone(host.project_type_store().semantic_graph());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let s = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Boolean));
    let s_artifact = NodeBearingExpansion::new(s, Arc::from(Vec::new().into_boxed_slice()), false);
    let expr = materialize_node_bearing_expansion(&dispatch, &s_artifact)
        .expect("primitive materialises")
        .expr;
    assert_eq!(
        expr,
        TypeExpr::Primitive(PrimitiveName::Boolean),
        "the facade materialises a Boolean primitive node to TypeExpr::Primitive(Boolean)"
    );
}
