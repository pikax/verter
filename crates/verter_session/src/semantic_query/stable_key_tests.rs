//! Discriminating tests for `VerterStableV1` keys, carrier-qualified
//! unions, ReduceIntersection grouping, and identity-vs-relation-vs-display.

use std::sync::Arc;

use crate::semantic_query::composite::{CompositeList, UnionKind};
use crate::semantic_query::stable_key::{
    fingerprint_v1, sort_by_stable_key, stable_key_for_node, StableKey,
};
use crate::semantic_query::{
    IntersectionInputRef, IntersectionPurpose, IntersectionTerm, LiteralValue, PrimitiveKind,
    SemanticContext, SemanticContextId, SemanticNodeData, SemanticNodeId, SemanticQueryKey,
};
use crate::semantic_query_memo::SemanticGraphStore;

fn prim(graph: &SemanticGraphStore, kind: PrimitiveKind) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Primitive(kind))
}

fn lit_str(graph: &SemanticGraphStore, s: &str) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Literal(LiteralValue::String(s.into())))
}

#[test]
fn primitives_use_fixed_tags_independent_of_intern_order() {
    let a = SemanticGraphStore::new();
    let b = SemanticGraphStore::new();
    let a_num = prim(&a, PrimitiveKind::Number);
    let a_str = prim(&a, PrimitiveKind::String);
    let b_str = prim(&b, PrimitiveKind::String);
    let b_num = prim(&b, PrimitiveKind::Number);
    assert_eq!(
        stable_key_for_node(&a, a_num),
        stable_key_for_node(&b, b_num)
    );
    assert_eq!(
        stable_key_for_node(&a, a_str),
        stable_key_for_node(&b, b_str)
    );
    assert_ne!(
        stable_key_for_node(&a, a_num),
        stable_key_for_node(&a, a_str)
    );
}

#[test]
fn pair_locality_holds_under_unrelated_interning() {
    let graph = SemanticGraphStore::new();
    let left = prim(&graph, PrimitiveKind::Number);
    let right = prim(&graph, PrimitiveKind::String);
    let before = (
        stable_key_for_node(&graph, left),
        stable_key_for_node(&graph, right),
    );
    let _ = lit_str(&graph, "unrelated");
    let after = (
        stable_key_for_node(&graph, left),
        stable_key_for_node(&graph, right),
    );
    assert_eq!(before, after);
}

#[test]
fn equal_prefix_literals_order_by_exact_key() {
    let graph = SemanticGraphStore::new();
    let a = lit_str(&graph, "a");
    let ab = lit_str(&graph, "ab");
    let abc = lit_str(&graph, "abc");
    let mut members = [abc, a, ab];
    sort_by_stable_key(&graph, &mut members);
    let keys: Vec<_> = members
        .iter()
        .map(|id| stable_key_for_node(&graph, *id))
        .collect();
    assert!(keys[0] < keys[1] && keys[1] < keys[2]);
    assert_ne!(keys[0], keys[1]);
    assert_ne!(keys[1], keys[2]);
}

#[test]
fn injected_fingerprint_collision_orders_by_exact_key() {
    let short = StableKey::with_forced_fingerprint(b"aa".to_vec(), 0x1111);
    let long = StableKey::with_forced_fingerprint(b"aaa".to_vec(), 0x1111);
    assert_eq!(short.fingerprint(), long.fingerprint());
    assert!(short < long);
}

#[test]
fn negative_zero_and_zero_share_a_literal_key() {
    let graph = SemanticGraphStore::new();
    let z = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(0.0)));
    let nz = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(-0.0)));
    assert_eq!(
        stable_key_for_node(&graph, z),
        stable_key_for_node(&graph, nz)
    );
}

#[test]
fn carrier_qualified_union_keeps_distinct_origins() {
    let graph = SemanticGraphStore::new();
    let n = prim(&graph, PrimitiveKind::Number);
    let s = prim(&graph, PrimitiveKind::String);
    let members: Arc<[SemanticNodeId]> = Arc::from([n, s]);
    let authored = graph.intern_node(SemanticNodeData::Union(
        CompositeList::<UnionKind>::authored_shell(Arc::clone(&members)),
    ));
    let query = graph.intern_node(SemanticNodeData::Union(
        CompositeList::<UnionKind>::query_subject(members),
    ));
    assert_ne!(
        authored, query,
        "distinct origin categories of the same shape must intern apart"
    );
}

#[test]
fn binary_reduce_intersection_allocates_no_recipe() {
    let a = SemanticNodeId(1);
    let b = SemanticNodeId(2);
    let input = IntersectionInputRef::binary(a, b);
    assert!(!input.is_recipe());
    assert_eq!(
        IntersectionInputRef::from_operands(&[a, b]),
        IntersectionInputRef::Binary(a, b)
    );
}

#[test]
fn two_term_list_with_subgroup_does_not_collapse_to_binary() {
    let a = SemanticNodeId(1);
    let b = SemanticNodeId(2);
    let c = SemanticNodeId(3);
    let nested = IntersectionInputRef::binary(b, c);
    let grouped = IntersectionInputRef::from_steps(&[
        IntersectionTerm::Value(a),
        IntersectionTerm::EvaluateSubgroup {
            input: nested,
            purpose: IntersectionPurpose::CheckerReduction,
        },
    ]);
    assert!(grouped.is_recipe());
    assert_ne!(grouped, IntersectionInputRef::from_operands(&[a, b, c]));
    let left_assoc = IntersectionInputRef::from_steps(&[
        IntersectionTerm::EvaluateSubgroup {
            input: IntersectionInputRef::binary(a, b),
            purpose: IntersectionPurpose::CheckerReduction,
        },
        IntersectionTerm::Value(c),
    ]);
    assert_ne!(grouped, left_assoc);
}

#[test]
fn fingerprint_is_fnv_of_exact_bytes() {
    let key = StableKey::from_exact(b"abc".to_vec());
    assert_eq!(key.fingerprint(), fingerprint_v1(b"abc"));
}

#[test]
fn reduce_intersection_key_carries_context() {
    let key = SemanticQueryKey::reduce_intersection_operands(Arc::from([SemanticNodeId(1)]));
    match key {
        SemanticQueryKey::ReduceIntersection { context, .. } => {
            assert_eq!(context, SemanticContextId::production());
        }
        other => panic!("{other:?}"),
    }
    let _ = SemanticContext::production();
}

#[test]
fn identity_audit_four_discriminators() {
    // union member dedup identity != Relate(Identity) != display equality != shape
    let graph = SemanticGraphStore::new();
    let n = prim(&graph, PrimitiveKind::Number);
    let members: Arc<[SemanticNodeId]> = Arc::from([n, n]);
    let union =
        crate::project_semantic_dispatch::canonical_algebra::canonical_union(&graph, &members);
    match graph.node_data(union.node).as_deref() {
        Some(SemanticNodeData::Primitive(PrimitiveKind::Number)) => {}
        Some(SemanticNodeData::Union(list)) => {
            assert_eq!(
                list.len(),
                1,
                "construction-identical first-occurrence dedup"
            );
        }
        other => panic!("unexpected {other:?}"),
    }
    let authored_a = graph.intern_node(SemanticNodeData::Union(
        CompositeList::<UnionKind>::authored_shell(Arc::from([n])),
    ));
    let authored_b = graph.intern_node(SemanticNodeData::Union(
        CompositeList::<UnionKind>::query_subject(Arc::from([n])),
    ));
    assert_ne!(
        authored_a, authored_b,
        "same shape, distinct carriers — not union-member-dedup identity"
    );
    let shown = crate::semantic_query::display::display(
        &graph,
        &crate::semantic_query::SemanticQueryValue::TypeNode(authored_a),
        crate::semantic_query::demand::DisplayNeeds::empty(),
    );
    assert_ne!(
        shown.to_string(),
        format!("{authored_a:?}"),
        "display is not node-id identity"
    );
}
