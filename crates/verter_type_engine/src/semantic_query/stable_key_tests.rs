//! Discriminating tests for `VerterStableV1` keys, carrier-qualified
//! unions, ReduceIntersection grouping, and identity-vs-relation-vs-display.

use std::sync::Arc;

use crate::project_semantic_dispatch::canonical_algebra::{
    compare_structural, CanonicalEvidence, StructuralIdentity,
};
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
    let union = crate::project_semantic_dispatch::canonical_algebra::intern_ordered_union(
        &graph,
        &members,
        verter_session_query::flow::policy::NullabilityPolicy::Strict,
    );
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

/// A union's member view lives with the store whose arena its id indexes.
/// Node ids are arena-local, so the SAME id in two stores names two
/// unrelated unions; a view kept for the process would hand the second store
/// the first store's members.
#[test]
fn union_views_are_scoped_to_their_store() {
    use crate::semantic_query::stable_key::semantic_union_members;
    let a = SemanticGraphStore::new();
    let b = SemanticGraphStore::new();
    let a_members: Arc<[SemanticNodeId]> = Arc::from([
        prim(&a, PrimitiveKind::Number),
        prim(&a, PrimitiveKind::String),
    ]);
    let b_members: Arc<[SemanticNodeId]> = Arc::from([lit_str(&b, "x"), lit_str(&b, "y")]);
    let a_union = a.intern_node(SemanticNodeData::Union(
        CompositeList::<UnionKind>::authored_shell(Arc::clone(&a_members)),
    ));
    let b_union = b.intern_node(SemanticNodeData::Union(
        CompositeList::<UnionKind>::authored_shell(Arc::clone(&b_members)),
    ));
    assert_eq!(
        a_union, b_union,
        "the fixture needs two unrelated unions under one arena-local id"
    );
    let ctx = SemanticContext::production();
    let sorted = |members: &[SemanticNodeId]| {
        let mut members = members.to_vec();
        members.sort();
        members
    };
    let a_view = semantic_union_members(&a, a_union, &ctx);
    let b_view = semantic_union_members(&b, b_union, &ctx);
    assert_eq!(
        sorted(&a_view),
        sorted(&a_members),
        "store A reads its own union"
    );
    assert_eq!(
        sorted(&b_view),
        sorted(&b_members),
        "store B must read ITS union, not the view store A built under the same id"
    );
}

/// `depth` nested arrays around a `leaf` primitive.
fn array_chain(graph: &SemanticGraphStore, depth: usize, leaf: PrimitiveKind) -> SemanticNodeId {
    let mut node = prim(graph, leaf);
    for _ in 0..depth {
        node = graph.intern_node(SemanticNodeData::Array {
            element: node,
            readonly: false,
        });
    }
    node
}

/// The members of a union node, or the node itself when it is not one.
fn union_arms(graph: &SemanticGraphStore, node: SemanticNodeId) -> Vec<SemanticNodeId> {
    match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Union(list)) => list.iter().copied().collect(),
        _ => vec![node],
    }
}

/// A depth past 256 levels.
const PAST_256_LEVELS: usize = 300;

/// One store's fixture: two structures identical through
/// [`PAST_256_LEVELS`] levels that differ only in their leaf, each also
/// wrapped as an intersection arm, interned A-first or B-first.
struct DeepPair {
    graph: SemanticGraphStore,
    a: SemanticNodeId,
    b: SemanticNodeId,
    a_arm: SemanticNodeId,
    b_arm: SemanticNodeId,
}

impl DeepPair {
    fn build(a_first: bool) -> Self {
        let graph = SemanticGraphStore::new();
        let (a, b) = if a_first {
            let a = array_chain(&graph, PAST_256_LEVELS, PrimitiveKind::Number);
            let b = array_chain(&graph, PAST_256_LEVELS, PrimitiveKind::String);
            (a, b)
        } else {
            let b = array_chain(&graph, PAST_256_LEVELS, PrimitiveKind::String);
            let a = array_chain(&graph, PAST_256_LEVELS, PrimitiveKind::Number);
            (a, b)
        };
        let tag = graph.intern_node(SemanticNodeData::DeclRef {
            identity: crate::semantic_query::DeclIdentity::synthetic("Tag"),
        });
        let arm = |deep| {
            crate::project_semantic_dispatch::canonical_algebra::intern_ordered_intersection(
                &graph,
                &[tag, deep],
            )
            .node
        };
        let (a_arm, b_arm) = if a_first {
            let a_arm = arm(a);
            (a_arm, arm(b))
        } else {
            let b_arm = arm(b);
            (arm(a), b_arm)
        };
        Self {
            graph,
            a,
            b,
            a_arm,
            b_arm,
        }
    }

    /// `"A"` / `"B"` for this store's nodes, so orders compare across stores.
    fn label(&self, node: SemanticNodeId) -> &'static str {
        if node == self.a || node == self.a_arm {
            "A"
        } else if node == self.b || node == self.b_arm {
            "B"
        } else {
            panic!("an unexpected node {node:?} in the ordered output")
        }
    }

    fn labels(&self, nodes: &[SemanticNodeId]) -> Vec<&'static str> {
        nodes.iter().map(|node| self.label(*node)).collect()
    }

    /// Every union ordering consumer's answer over the pair, fed A-first or
    /// B-first.
    fn orders(&self, a_first: bool) -> Vec<Vec<&'static str>> {
        use crate::project_semantic_dispatch::canonical_algebra::intern_ordered_union;
        use crate::semantic_query::stable_key::{
            canonicalize_union_members, sort_union_members_by_stable_key,
        };
        let input = |x, y| if a_first { [x, y] } else { [y, x] };
        let mut sorted = input(self.a, self.b);
        sort_union_members_by_stable_key(&self.graph, &mut sorted);
        let canonical = canonicalize_union_members(&self.graph, &input(self.a, self.b));
        let union = intern_ordered_union(
            &self.graph,
            &input(self.a, self.b),
            verter_session_query::flow::policy::NullabilityPolicy::Strict,
        );
        let arm_union = intern_ordered_union(
            &self.graph,
            &input(self.a_arm, self.b_arm),
            verter_session_query::flow::policy::NullabilityPolicy::Strict,
        );
        vec![
            self.labels(&sorted),
            self.labels(&canonical),
            self.labels(&union_arms(&self.graph, union.node)),
            self.labels(&union_arms(&self.graph, arm_union.node)),
        ]
    }
}

/// Two structures identical through more than 256 levels that differ only
/// below them get distinct keys, the same keys in two stores that interned
/// them in opposite orders, and one canonical order through every union
/// ordering consumer — never an order inherited from arrival.
#[test]
fn structures_differing_below_256_levels_keep_distinct_keys_and_one_order() {
    let forward = DeepPair::build(true);
    let reverse = DeepPair::build(false);
    for pair in [&forward, &reverse] {
        assert!(
            stable_key_for_node(&pair.graph, pair.a) != stable_key_for_node(&pair.graph, pair.b),
            "structures that differ below the 256th level must not share a key"
        );
        assert!(
            stable_key_for_node(&pair.graph, pair.a_arm)
                != stable_key_for_node(&pair.graph, pair.b_arm),
            "intersections over them must not share a key"
        );
    }
    for (node_forward, node_reverse) in [
        (forward.a, reverse.a),
        (forward.b, reverse.b),
        (forward.a_arm, reverse.a_arm),
        (forward.b_arm, reverse.b_arm),
    ] {
        assert!(
            stable_key_for_node(&forward.graph, node_forward)
                == stable_key_for_node(&reverse.graph, node_reverse),
            "a key is a function of structure, not of intern order"
        );
    }
    let expected = forward.orders(true);
    for orders in [
        forward.orders(false),
        reverse.orders(true),
        reverse.orders(false),
    ] {
        assert_eq!(
            orders, expected,
            "every ordering consumer answers one order in both stores and for both inputs"
        );
    }
    for order in &expected {
        assert_eq!(order.len(), 2, "no consumer collapses the pair: {order:?}");
    }
}

/// Ten thousand levels encode completely on a 1 MiB thread: the encoder
/// keeps its frames on the heap, so depth never reaches the native stack.
#[test]
fn ten_thousand_levels_encode_on_a_one_mebibyte_thread() {
    const DEPTH: usize = 10_000;
    let worker = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| {
            use crate::semantic_query::stable_key::{
                canonicalize_union_members, sort_union_members_by_stable_key,
            };
            let graph = SemanticGraphStore::new();
            let b = array_chain(&graph, DEPTH, PrimitiveKind::String);
            let a = array_chain(&graph, DEPTH, PrimitiveKind::Number);
            let key_a = stable_key_for_node(&graph, a);
            let key_b = stable_key_for_node(&graph, b);
            assert!(key_a != key_b, "the leaves differ ten thousand levels down");
            assert!(
                key_a == stable_key_for_node(&graph, a),
                "re-encoding reproduces the key"
            );
            let mut forward = [a, b];
            let mut reverse = [b, a];
            sort_union_members_by_stable_key(&graph, &mut forward);
            sort_union_members_by_stable_key(&graph, &mut reverse);
            assert_eq!(forward, reverse, "one order for both inputs");
            assert_eq!(
                canonicalize_union_members(&graph, &[b, a]).as_ref(),
                &forward,
                "the union view agrees with the sort"
            );
            key_a.exact().len()
        })
        .expect("spawn the 1 MiB encoder thread");
    let bytes = worker
        .join()
        .expect("the encoder completes on a 1 MiB stack");
    assert!(
        bytes > DEPTH * 7,
        "every level is in the key ({bytes} bytes for {DEPTH} levels)"
    );
}

fn std_hash(key: &StableKey) -> u64 {
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    key.hash(&mut hasher);
    hasher.finish()
}

/// `Eq`, `Ord` and `Hash` agree on every pair of keys: `a.cmp(b)` is
/// `Equal` exactly when `a == b`, the order is antisymmetric and
/// transitive, and equal keys hash alike. The adversaries are an encoded
/// key deeper than 256 levels beside its byte-identical rebuild, equal
/// prefixes, the empty key, and forced full fingerprint collisions: equal
/// fingerprints over distinct bytes, and equal bytes under distinct
/// fingerprints.
#[test]
fn stable_key_equality_order_and_hash_agree_on_every_pair() {
    use std::cmp::Ordering;
    let graph = SemanticGraphStore::new();
    let deep = stable_key_for_node(
        &graph,
        array_chain(&graph, PAST_256_LEVELS, PrimitiveKind::Number),
    );
    let keys = [
        deep.clone(),
        StableKey::from_exact(deep.exact().to_vec()),
        StableKey::from_exact(Vec::new()),
        StableKey::from_exact(b"aa".to_vec()),
        StableKey::from_exact(b"aaa".to_vec()),
        StableKey::with_forced_fingerprint(Vec::new(), 7),
        StableKey::with_forced_fingerprint(b"aa".to_vec(), 7),
        StableKey::with_forced_fingerprint(b"aaa".to_vec(), 7),
        StableKey::with_forced_fingerprint(b"ab".to_vec(), 7),
        StableKey::with_forced_fingerprint(b"aa".to_vec(), 8),
        StableKey::with_forced_fingerprint(deep.exact().to_vec(), deep.fingerprint()),
    ];
    assert!(
        keys[0] == keys[1] && keys[0].cmp(&keys[1]) == Ordering::Equal,
        "an encoded key equals its byte-identical rebuild under both relations"
    );
    for a in &keys {
        for b in &keys {
            assert_eq!(
                a.cmp(b) == Ordering::Equal,
                a == b,
                "Ord and Eq disagree on {a:?} / {b:?}"
            );
            assert_eq!(a.partial_cmp(b), Some(a.cmp(b)));
            assert_eq!(a.cmp(b), b.cmp(a).reverse(), "antisymmetry");
            if a == b {
                assert_eq!(std_hash(a), std_hash(b), "equal keys hash alike");
            }
            for c in &keys {
                if a <= b && b <= c {
                    assert!(a <= c, "transitivity over {a:?} <= {b:?} <= {c:?}");
                }
            }
        }
    }
}

/// An intrinsic application keys its op by the op's frozen, append-only
/// stable tag, the one tag authority for compiler intrinsics. Every op is
/// listed, each keeps its pinned tag, and no two ops share a key.
#[test]
fn intrinsic_applications_key_by_the_frozen_op_tag() {
    use verter_type_expr::CompilerIntrinsicTypeOp;
    // Exhaustive: a new op fails to compile here until it states its
    // pinned tag, and it joins `every_op`, which `ALL` must cover.
    let pinned = |op: CompilerIntrinsicTypeOp| match op {
        CompilerIntrinsicTypeOp::Awaited => 0u8,
        CompilerIntrinsicTypeOp::NoInfer => 1u8,
    };
    let every_op = [
        CompilerIntrinsicTypeOp::Awaited,
        CompilerIntrinsicTypeOp::NoInfer,
    ];
    assert!(
        every_op
            .iter()
            .all(|op| CompilerIntrinsicTypeOp::ALL.contains(op)),
        "`ALL` lists every op"
    );
    let graph = SemanticGraphStore::new();
    let operand = prim(&graph, PrimitiveKind::String);
    let mut keys: Vec<StableKey> = Vec::new();
    for op in CompilerIntrinsicTypeOp::ALL {
        assert_eq!(op.stable_hash_tag(), pinned(*op), "{op:?} keeps its tag");
        let node = graph.intern_node(
            SemanticNodeData::intrinsic_application(*op, Arc::from(vec![operand; op.arity()]))
                .expect("a well-formed application"),
        );
        let key = stable_key_for_node(&graph, node);
        // Version, the synthetic category, the intrinsic-application
        // sub-tag, then the op tag.
        assert_eq!(&key.exact()[..4], &[1, 6, 21, pinned(*op)], "{op:?}");
        assert!(!keys.contains(&key), "{op:?} shares a key with another op");
        keys.push(key);
    }
}

/// `count` unrelated literals, so the next nodes a store mints sit at other
/// arena ids than in a fresh store.
fn pad(graph: &SemanticGraphStore, count: usize) {
    for index in 0..count {
        let _ = lit_str(graph, &format!("padding-{index}"));
    }
}

fn function_occurrence(
    symbol: &str,
    signature_ordinal: u32,
) -> crate::semantic_query::SignatureNodeOccurrence {
    use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
    use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};
    crate::semantic_query::SignatureNodeOccurrence {
        function: FlowFunctionReturnIdentity {
            anchor: AuthoredAnchor {
                canonical_id: Arc::from("/src/f.ts"),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                symbol: Arc::from(symbol),
                space: LocatorSymbolSpace::Value,
            },
            function_part: FunctionPartIdentity::DeclarationBody,
            overload_ordinal: 0,
        },
        signature_ordinal,
    }
}

/// One deferred callable `(x: param) => <deferred>`.
fn deferred_callable(
    graph: &SemanticGraphStore,
    occurrence: crate::semantic_query::SignatureNodeOccurrence,
    param: PrimitiveKind,
    body_derived: bool,
) -> SemanticNodeId {
    use crate::semantic_query::{
        DeferredCallable, FunctionParam, SignatureKind, SignatureReturnCarrier,
    };
    use verter_type_expr::facts::FunctionReturnSource;
    let ty = prim(graph, param);
    let return_carrier = SignatureReturnCarrier::Function(if body_derived {
        FunctionReturnSource::Flow(occurrence.function.clone())
    } else {
        FunctionReturnSource::Absent
    });
    graph.intern_node(SemanticNodeData::DeferredCallable(
        DeferredCallable::from_parts_for_tests(
            SignatureKind::Call,
            Arc::from([FunctionParam::synthetic(
                Some(Arc::from("x")),
                ty,
                false,
                false,
            )]),
            Arc::from(Vec::new()),
            occurrence,
            return_carrier,
        ),
    ))
}

/// A deferred callable keys its whole closed recipe — the served position
/// it was composed at, its parameters and its deferred return carrier —
/// and never the arena ids of its children.
#[test]
fn deferred_callables_key_their_composed_position_and_parameters() {
    let graph = SemanticGraphStore::new();
    let base = deferred_callable(
        &graph,
        function_occurrence("f", 0),
        PrimitiveKind::Number,
        true,
    );
    let variants = [
        deferred_callable(
            &graph,
            function_occurrence("f", 1),
            PrimitiveKind::Number,
            true,
        ),
        deferred_callable(
            &graph,
            function_occurrence("g", 0),
            PrimitiveKind::Number,
            true,
        ),
        deferred_callable(
            &graph,
            function_occurrence("f", 0),
            PrimitiveKind::String,
            true,
        ),
        deferred_callable(
            &graph,
            function_occurrence("f", 0),
            PrimitiveKind::Number,
            false,
        ),
    ];
    let mut keys = vec![stable_key_for_node(&graph, base)];
    for variant in variants {
        let key = stable_key_for_node(&graph, variant);
        assert!(
            !keys.contains(&key),
            "deferred callables over another position, parameter or return share a key"
        );
        keys.push(key);
    }
    let elsewhere = SemanticGraphStore::new();
    pad(&elsewhere, 11);
    let same = deferred_callable(
        &elsewhere,
        function_occurrence("f", 0),
        PrimitiveKind::Number,
        true,
    );
    assert!(
        stable_key_for_node(&elsewhere, same) == keys[0],
        "the same recipe keys alike wherever its children sit"
    );
}

fn synthetic_binding(
    graph: &SemanticGraphStore,
    scope: &str,
    surface_kind: verter_type_expr::SyntheticCarrierSurfaceKind,
    slot_name: Option<&str>,
    binding_name: &str,
    value: PrimitiveKind,
) -> SemanticNodeId {
    let value = prim(graph, value);
    graph.intern_node(SemanticNodeData::SyntheticBinding {
        id: crate::semantic_query::SyntheticBindingId {
            scope_canonical_id: Arc::from(scope),
            surface_kind,
            slot_name: slot_name.map(Arc::from),
            binding_name: Arc::from(binding_name),
        },
        value_node: value.0,
    })
}

/// A synthetic binding keys its synthesizing scope, its surface role, its
/// slot and bound name, and the key of its bound value, never the arena
/// ordinal of that value.
#[test]
fn synthetic_bindings_key_owner_role_position_and_value() {
    use verter_type_expr::SyntheticCarrierSurfaceKind::{Binding, SlotBinding};
    let graph = SemanticGraphStore::new();
    let number = PrimitiveKind::Number;
    let base = synthetic_binding(
        &graph,
        "/c.vue",
        SlotBinding,
        Some("default"),
        "item",
        number,
    );
    let variants = [
        synthetic_binding(
            &graph,
            "/d.vue",
            SlotBinding,
            Some("default"),
            "item",
            number,
        ),
        synthetic_binding(&graph, "/c.vue", Binding, Some("default"), "item", number),
        synthetic_binding(&graph, "/c.vue", SlotBinding, None, "item", number),
        synthetic_binding(
            &graph,
            "/c.vue",
            SlotBinding,
            Some("header"),
            "item",
            number,
        ),
        synthetic_binding(
            &graph,
            "/c.vue",
            SlotBinding,
            Some("default"),
            "row",
            number,
        ),
        synthetic_binding(
            &graph,
            "/c.vue",
            SlotBinding,
            Some("default"),
            "item",
            PrimitiveKind::String,
        ),
    ];
    let mut keys = vec![stable_key_for_node(&graph, base)];
    for variant in variants {
        let key = stable_key_for_node(&graph, variant);
        assert!(
            !keys.contains(&key),
            "bindings with another owner, role, position or value share a key"
        );
        keys.push(key);
    }
    let elsewhere = SemanticGraphStore::new();
    pad(&elsewhere, 5);
    let same = synthetic_binding(
        &elsewhere,
        "/c.vue",
        SlotBinding,
        Some("default"),
        "item",
        number,
    );
    assert!(
        stable_key_for_node(&elsewhere, same) == keys[0],
        "the bound value keys by structure, not by its arena ordinal"
    );
}

/// A raw fallback keys its raw text, the whole equality identity of the
/// payload: the diagnostic provenance, which never distinguishes two
/// payloads, never distinguishes two keys either.
#[test]
fn raw_fallbacks_key_their_raw_text_not_their_provenance() {
    use verter_type_expr::UnknownValue;
    let raw = |value: UnknownValue| {
        let graph = SemanticGraphStore::new();
        let node = graph.intern_node(SemanticNodeData::RawFallback { value });
        stable_key_for_node(&graph, node)
    };
    assert!(
        raw(UnknownValue::unsupported_syntax("Foo<"))
            == raw(UnknownValue::jsdoc_parse_fallback("Foo<")),
        "one raw text keys alike under every provenance"
    );
    assert!(
        raw(UnknownValue::unsupported_syntax("Foo<"))
            != raw(UnknownValue::unsupported_syntax("Bar<")),
        "distinct raw text keys apart"
    );
}

fn import_type(graph: &SemanticGraphStore, importer: Option<&str>) -> SemanticNodeId {
    use crate::semantic_query::NodeScopeId;
    let data = SemanticNodeData::new_import_type(
        Arc::from("./m"),
        Arc::from([Arc::<str>::from("G")]),
        Arc::from(Vec::new()),
        false,
    );
    match importer {
        None => graph.intern_node(data),
        Some(canonical) => graph.intern_node_with_scope(
            data,
            NodeScopeId::File {
                canonical_id: Arc::from(canonical),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: [3u8; 16],
                local_scope: None,
            },
        ),
    }
}

/// An import-type carrier keys the importing source unit with its
/// specifier (the whole resolver input), so one spelling written in two
/// files, which can name two modules, keys as two identities, and the same
/// import keys alike in every store.
#[test]
fn import_types_key_their_importing_unit_with_the_specifier() {
    let graph = SemanticGraphStore::new();
    let from_a = stable_key_for_node(&graph, import_type(&graph, Some("/src/a.ts")));
    let from_nested = stable_key_for_node(&graph, import_type(&graph, Some("/src/lib/b.ts")));
    let unscoped = stable_key_for_node(&graph, import_type(&graph, None));
    assert!(
        from_a != from_nested,
        "one specifier from two importing units keys apart"
    );
    assert!(
        unscoped != from_a && unscoped != from_nested,
        "an unscoped carrier is not the import of any file"
    );
    let elsewhere = SemanticGraphStore::new();
    pad(&elsewhere, 3);
    assert!(
        stable_key_for_node(&elsewhere, import_type(&elsewhere, Some("/src/a.ts"))) == from_a,
        "the same import keys alike in another store"
    );
}

/// No encoding in the stable-key module is Rust `Debug` text: a `Debug`
/// form is not a versioned schema, and it prints fields (a diagnostic
/// provenance, an arena ordinal) that are not identity.
#[test]
fn stable_key_encoder_formats_no_debug_text() {
    let source = include_str!("stable_key.rs");
    for (index, line) in source.lines().enumerate() {
        let code = line.split("//").next().unwrap_or_default();
        assert!(
            !code.contains(":?") && !code.contains("format!"),
            "stable_key.rs line {}: `{}` formats text into a key",
            index + 1,
            line.trim()
        );
    }
}

/// `import("<specifier>").G` written in `importer`.
fn import_type_spelled(
    graph: &SemanticGraphStore,
    importer: &str,
    specifier: &str,
) -> SemanticNodeId {
    import_type_at(graph, importer, specifier, [3u8; 16])
}

/// `import("<specifier>").G` written in `importer`, whose content is
/// `whole_hash`: two hashes of one importer are two arena scopes of one
/// importing unit.
fn import_type_at(
    graph: &SemanticGraphStore,
    importer: &str,
    specifier: &str,
    whole_hash: [u8; 16],
) -> SemanticNodeId {
    graph.intern_node_with_scope(
        SemanticNodeData::new_import_type(
            Arc::from(specifier),
            Arc::from([Arc::<str>::from("G")]),
            Arc::from(Vec::new()),
            false,
        ),
        crate::semantic_query::NodeScopeId::File {
            canonical_id: Arc::from(importer),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash,
            local_scope: None,
        },
    )
}

/// Two spellings of one module from one importer — `./a` and `../x/a`
/// written in `/x/b.ts` both name `/x/a` — should key as one identity. The
/// key of an unresolved carrier is its importing unit plus its authored
/// specifier, so today the two key apart and converge only once
/// demand-time resolution produces the declaration node.
#[test]
#[ignore = "import-type identity converges only after resolution; needs an indexing-time resolved-module producer"]
fn two_spellings_of_one_module_key_alike() {
    let graph = SemanticGraphStore::new();
    let direct = import_type_spelled(&graph, "/x/b.ts", "./a");
    let roundabout = import_type_spelled(&graph, "/x/b.ts", "../x/a");
    assert!(
        stable_key_for_node(&graph, direct) == stable_key_for_node(&graph, roundabout),
        "`./a` and `../x/a` from `/x/b.ts` name one module and key alike"
    );
}

/// One spelling written in two importing units can name two modules, so a
/// union of the two unresolved carriers keeps both arms, directly and nested
/// in a structure the comparator descends. Measured with tsc 7.0.2
/// (`--declaration --emitDeclarationOnly`, every `strictNullChecks` x
/// `noImplicitAny` setting): with `src/m.ts` exporting `G = { a: 1 }`,
/// `src/lib/m.ts` exporting `G = { b: 2 }`, `src/a.ts` and `src/lib/b.ts`
/// each exporting a value of `import("./m").G`, and `src/c.ts` exporting
/// `u = Math.random() ? x : y`, the emitted type is
/// `import("./m").G | import("./lib/m").G`.
#[test]
fn one_spelling_from_two_importers_keeps_two_union_arms() {
    let graph = SemanticGraphStore::new();
    let from_a = import_type_spelled(&graph, "/src/a.ts", "./m");
    let from_lib = import_type_spelled(&graph, "/src/lib/b.ts", "./m");
    assert_eq!(
        reduced_union_arms(&graph, from_a, from_lib),
        2,
        "`import(\"./m\").G` from two importing units keeps two arms"
    );
    assert_eq!(
        reduced_union_arms(
            &graph,
            array_of_node(&graph, from_a),
            array_of_node(&graph, from_lib)
        ),
        2,
        "`import(\"./m\").G[]` from two importing units keeps two arms"
    );
    assert_eq!(
        structural_identity(&graph, from_a, from_lib),
        StructuralIdentity::Distinct,
        "the comparator tells the two importing units apart"
    );
}

/// One spelling written in one importing unit is one module: two carriers
/// of it that sit in two arena scopes of that unit (here, two content
/// hashes) are one union arm, directly and nested, because the rest of the
/// scope stays provenance.
#[test]
fn one_spelling_from_one_importer_is_one_union_arm() {
    let graph = SemanticGraphStore::new();
    let before = import_type_at(&graph, "/src/a.ts", "./m", [3u8; 16]);
    let after = import_type_at(&graph, "/src/a.ts", "./m", [4u8; 16]);
    assert_ne!(before, after, "premise: two arena scopes, two nodes");
    assert_eq!(
        reduced_union_arms(&graph, before, after),
        1,
        "`import(\"./m\").G` from one importing unit is one arm"
    );
    assert_eq!(
        reduced_union_arms(
            &graph,
            array_of_node(&graph, before),
            array_of_node(&graph, after)
        ),
        1,
        "`import(\"./m\").G[]` from one importing unit is one arm"
    );
    assert_eq!(
        structural_identity(&graph, before, after),
        StructuralIdentity::Equal,
        "the comparator reads only the importing unit of the scope"
    );
}

fn array_of_node(graph: &SemanticGraphStore, element: SemanticNodeId) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Array {
        element,
        readonly: false,
    })
}

/// The union reducer's structural comparator verdict on `a` and `b`.
fn structural_identity(
    graph: &SemanticGraphStore,
    a: SemanticNodeId,
    b: SemanticNodeId,
) -> StructuralIdentity {
    let mut evidence = CanonicalEvidence::default();
    let mut budget = u32::MAX;
    compare_structural(graph, a, b, &mut evidence, &mut budget)
}

/// The arm count of the union reducer's answer over `a | b`.
fn reduced_union_arms(graph: &SemanticGraphStore, a: SemanticNodeId, b: SemanticNodeId) -> usize {
    let union = crate::project_semantic_dispatch::canonical_algebra::intern_ordered_union(
        graph,
        &[a, b],
        verter_session_query::flow::policy::NullabilityPolicy::Strict,
    );
    union_arms(graph, union.node).len()
}

/// One generic call signature `<T …>(x: T) => T` over a shared bound-free
/// binder, with the declaration's own bounds, constness and return carrier.
fn generic_signature(
    graph: &SemanticGraphStore,
    binder: SemanticNodeId,
    constraint: Option<SemanticNodeId>,
    default: Option<SemanticNodeId>,
    is_const: bool,
    return_carrier: Option<crate::semantic_query::SignatureReturnCarrier>,
) -> SemanticNodeId {
    use crate::semantic_query::{
        FunctionParam, SignatureKind, SignatureReturnCarrier, TypeParamDecl,
    };
    graph.intern_node(SemanticNodeData::Signature {
        kind: SignatureKind::Call,
        params: Arc::from([FunctionParam::synthetic(
            Some(Arc::from("x")),
            binder,
            false,
            false,
        )]),
        return_type: binder,
        type_parameters: Arc::from([TypeParamDecl {
            name: Arc::from("T"),
            param: binder,
            constraint,
            default,
            is_const,
        }]),
        occurrence: None,
        return_carrier: return_carrier.unwrap_or(SignatureReturnCarrier::Declared(binder)),
        signature_span: None,
        return_type_span: None,
        predicate: None,
        is_abstract: false,
    })
}

/// A bound-free binder shared by every generic signature in a test.
fn bound_free_binder(graph: &SemanticGraphStore) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("T"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("T"),
    })
}

/// Callable keys carry each binder declaration's constraint, default and
/// constness, and a signature's return carrier: `(<T extends string>(x: T)
/// => T) | (<T extends number>(x: T) => T)` over one bound-free binder keeps
/// two keys. The union reducer's comparator rejects every such pair, and a
/// key-equality collapse after it must never merge what it rejected, in
/// either input order.
#[test]
fn callable_keys_carry_binder_bounds_constness_and_return_carrier() {
    use crate::semantic_query::SignatureReturnCarrier;
    use verter_type_expr::facts::FunctionReturnSource;
    let graph = SemanticGraphStore::new();
    let binder = bound_free_binder(&graph);
    let string = prim(&graph, PrimitiveKind::String);
    let number = prim(&graph, PrimitiveKind::Number);
    let base = generic_signature(&graph, binder, Some(string), None, false, None);
    let variants = [
        generic_signature(&graph, binder, Some(number), None, false, None),
        generic_signature(&graph, binder, None, None, false, None),
        generic_signature(&graph, binder, Some(string), Some(string), false, None),
        generic_signature(&graph, binder, Some(string), None, true, None),
        generic_signature(
            &graph,
            binder,
            Some(string),
            None,
            false,
            Some(SignatureReturnCarrier::Function(
                FunctionReturnSource::Absent,
            )),
        ),
    ];
    for variant in variants {
        assert_eq!(
            structural_identity(&graph, base, variant),
            StructuralIdentity::Distinct,
            "premise: the comparator rejects the pair"
        );
        for (first, second) in [(base, variant), (variant, base)] {
            assert_eq!(
                reduced_union_arms(&graph, first, second),
                2,
                "a key-equality collapse never merges a pair the comparator rejected"
            );
        }
        assert!(
            stable_key_for_node(&graph, base) != stable_key_for_node(&graph, variant),
            "signatures that differ in a binder bound, constness or return carrier key apart"
        );
    }
}

/// A deferred callable's binder declarations key their bounds and constness
/// exactly as a signature's do.
#[test]
fn deferred_callable_keys_carry_binder_bounds_and_constness() {
    use crate::semantic_query::{
        DeferredCallable, FunctionParam, SignatureKind, SignatureReturnCarrier, TypeParamDecl,
    };
    use verter_type_expr::facts::FunctionReturnSource;
    let graph = SemanticGraphStore::new();
    let binder = bound_free_binder(&graph);
    let string = prim(&graph, PrimitiveKind::String);
    let number = prim(&graph, PrimitiveKind::Number);
    let deferred = |constraint, default, is_const| {
        let key = graph.intern_node(SemanticNodeData::DeferredCallable(
            DeferredCallable::from_parts_for_tests(
                SignatureKind::Call,
                Arc::from([FunctionParam::synthetic(None, binder, false, false)]),
                Arc::from([TypeParamDecl {
                    name: Arc::from("T"),
                    param: binder,
                    constraint,
                    default,
                    is_const,
                }]),
                function_occurrence("f", 0),
                SignatureReturnCarrier::Function(FunctionReturnSource::Absent),
            ),
        ));
        stable_key_for_node(&graph, key)
    };
    let base = deferred(Some(string), None, false);
    for variant in [
        deferred(Some(number), None, false),
        deferred(None, None, false),
        deferred(Some(string), Some(string), false),
        deferred(Some(string), None, true),
    ] {
        assert!(
            base != variant,
            "deferred callables that differ in a binder bound or constness key apart"
        );
    }
}

/// An object whose public properties `names` all hold `value`.
fn object_of(graph: &SemanticGraphStore, names: &[&str], value: SemanticNodeId) -> SemanticNodeId {
    use crate::semantic_query::{IndexSignature, SurfaceMember};
    let members: Vec<SurfaceMember> = names
        .iter()
        .map(|name| SurfaceMember {
            excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            visibility: verter_type_expr::MemberVisibility::Public,
            key: crate::semantic_query::AuthoredPropertyKey::string(*name),
            value,
            optional: false,
            readonly: false,
            method_kind: None,
            has_implementation_body: false,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            spans: Default::default(),
            declaration_origin: None,
        })
        .collect();
    graph.intern_node(SemanticNodeData::Object(crate::test_surface_view! {
        members: Arc::from(members.into_boxed_slice()),
        call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
        index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
        keyspace: None,
        has_index_signature: false,
    }))
}

/// The key of `depth` nested objects, each holding the next under every
/// one of `names`, around `number`.
fn nested_object_key_len(names: &[&str], depth: usize) -> usize {
    let graph = SemanticGraphStore::new();
    let mut node = prim(&graph, PrimitiveKind::Number);
    for _ in 0..depth {
        node = object_of(&graph, names, node);
    }
    stable_key_for_node(&graph, node).exact().len()
}

/// A key is linear in the structure it encodes, however often a subtree is
/// shared: `{ p: { p: … } }` reaches each level through its member entry
/// and its derived positive member, and `{ a: X, b: X }` reaches `X` twice,
/// so an encoding that writes every occurrence doubles per level.
#[test]
fn shared_subtrees_keep_keys_linear_in_depth() {
    for names in [&["p"][..], &["a", "b"][..]] {
        for depth in [18usize, 400] {
            let bytes = nested_object_key_len(names, depth);
            assert!(
                bytes <= 1024 * depth,
                "{names:?} nested {depth} deep keys in {bytes} bytes, not linear in depth"
            );
        }
    }
}

/// Sharing is decided by structure, never by node identity: a subtree held
/// twice through one node and the same subtree built twice as two nodes
/// (here, in two arena scopes) give one key, and the key refers back to
/// the first copy either way.
#[test]
fn shared_subtrees_are_decided_by_structure_not_node_identity() {
    let graph = SemanticGraphStore::new();
    let deep = |scope: &str| {
        let mut node = graph.intern_node_with_scope(
            SemanticNodeData::Primitive(PrimitiveKind::Number),
            crate::semantic_query::NodeScopeId::File {
                canonical_id: Arc::from(scope),
                owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
                whole_hash: [5u8; 16],
                local_scope: None,
            },
        );
        for _ in 0..12 {
            node = object_of(&graph, &["p"], node);
        }
        node
    };
    let one = deep("/one.ts");
    let other = deep("/other.ts");
    assert_ne!(one, other, "premise: two nodes of one structure");
    let shared = object_of(&graph, &["a", "b"], one);
    let separate = {
        use crate::semantic_query::{IndexSignature, SurfaceMember};
        let member = |name: &str, value| SurfaceMember {
            excess_origin: verter_type_expr::ExcessPropertyOrigin::NonLiteral,
            visibility: verter_type_expr::MemberVisibility::Public,
            key: crate::semantic_query::AuthoredPropertyKey::string(name),
            value,
            optional: false,
            readonly: false,
            method_kind: None,
            has_implementation_body: false,
            declared_in_macro_type_arg: crate::semantic_query::MacroOwnBodyStamp::NEUTRAL,
            merge_role: crate::semantic_query::MergeRoleStamp::NEUTRAL,
            spans: Default::default(),
            declaration_origin: None,
        };
        graph.intern_node(SemanticNodeData::Object(crate::test_surface_view! {
            members: Arc::from([member("a", one), member("b", other)]),
            call_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            construct_signatures: Arc::from(Vec::<SemanticNodeId>::new().into_boxed_slice()),
            index_signatures: Arc::from(Vec::<IndexSignature>::new().into_boxed_slice()),
            keyspace: None,
            has_index_signature: false,
        }))
    };
    assert_ne!(shared, separate, "premise: two object nodes");
    let key = stable_key_for_node(&graph, shared);
    assert!(
        key == stable_key_for_node(&graph, separate),
        "one structure keys alike whether its repeated subtree is one node or two"
    );
    let reference = [1u8, 7, 3];
    assert!(
        key.exact().windows(3).any(|window| window == reference),
        "the repeated subtree is written once and referred back to"
    );
}

/// Ten thousand nested objects, each reaching the next twice, encode on a
/// 1 MiB thread in a key linear in the depth.
#[test]
fn ten_thousand_shared_levels_encode_linearly_on_a_one_mebibyte_thread() {
    const DEPTH: usize = 10_000;
    let bytes = std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(|| nested_object_key_len(&["p"], DEPTH))
        .expect("spawn the 1 MiB encoder thread")
        .join()
        .expect("the encoder completes on a 1 MiB stack");
    assert!(
        bytes <= 1024 * DEPTH,
        "{DEPTH} shared levels key in {bytes} bytes"
    );
}

/// `(x: <param>) => 0` with no authored occurrence, whose parameter is or is
/// not written as a literal type.
fn literal_param_signature(
    graph: &SemanticGraphStore,
    param: SemanticNodeId,
    declared_literal: bool,
) -> SemanticNodeId {
    use crate::semantic_query::{FunctionParam, SignatureKind, SignatureReturnCarrier};
    let zero = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(0.0)));
    graph.intern_node(SemanticNodeData::Signature {
        kind: SignatureKind::Call,
        params: Arc::from([FunctionParam {
            declared_literal,
            ..FunctionParam::synthetic(Some(Arc::from("x")), param, false, false)
        }]),
        return_type: zero,
        type_parameters: Arc::from(Vec::new()),
        occurrence: None,
        return_carrier: SignatureReturnCarrier::Declared(zero),
        signature_span: None,
        return_type_span: None,
        predicate: None,
        is_abstract: false,
    })
}

/// A parameter written as a literal type makes a signature specialized
/// (overload priority reads it), so `((x: "a") => 0) | ((x: T) => 0)` with
/// `T = "a"` keeps both signatures whichever arrives first.
#[test]
fn union_reducer_keeps_a_literal_specialized_signature_apart() {
    let graph = SemanticGraphStore::new();
    let a = lit_str(&graph, "a");
    let written_literal = literal_param_signature(&graph, a, true);
    let instantiated = literal_param_signature(&graph, a, false);
    assert_eq!(
        structural_identity(&graph, written_literal, instantiated),
        StructuralIdentity::Distinct,
        "the comparator tells a literal-declared parameter apart"
    );
    for (first, second) in [
        (written_literal, instantiated),
        (instantiated, written_literal),
    ] {
        assert_eq!(
            reduced_union_arms(&graph, first, second),
            2,
            "both signatures survive in either input order"
        );
    }
}

/// `{ [K in keyof T]: V }` over a type variable (homomorphic) and the same
/// recipe over concrete keys evaluate differently, so a union keeps both,
/// with few arms and with enough child-bearing arms to bucket them.
#[test]
fn union_reducer_keeps_homomorphic_and_concrete_mappings_apart() {
    use crate::semantic_query::{MapperKey, MapperKind, OptionalityMod, ReadonlyMod};
    let graph = SemanticGraphStore::new();
    let source = bound_free_binder(&graph);
    let key_space = graph.intern_node(SemanticNodeData::KeyOf { base: source });
    let parameter = graph.intern_node(SemanticNodeData::TypeParam {
        decl: crate::semantic_query::DeclIdentity::synthetic("K"),
        param_index: 0,
        constraint: None,
        default: None,
        display_name: Arc::from("K"),
    });
    let value = prim(&graph, PrimitiveKind::Number);
    let mapped = |over_type_variable| {
        graph.intern_node(SemanticNodeData::Mapped {
            source,
            mapper: MapperKey {
                parameter_node: parameter,
                key_space,
                value_expr: value,
                optionality: OptionalityMod::Keep,
                readonly: ReadonlyMod::Keep,
                name_remap: None,
                kind: MapperKind::Computed,
                over_type_variable,
            },
        })
    };
    let homomorphic = mapped(true);
    let concrete = mapped(false);
    assert_eq!(
        structural_identity(&graph, homomorphic, concrete),
        StructuralIdentity::Distinct,
        "the comparator tells the two mappings apart"
    );
    let bystanders: Vec<SemanticNodeId> = (0..7)
        .map(|index| {
            let element = lit_str(&graph, &format!("bystander-{index}"));
            graph.intern_node(SemanticNodeData::Array {
                element,
                readonly: false,
            })
        })
        .collect();
    for extra in [&[][..], &bystanders[..]] {
        for pair in [[homomorphic, concrete], [concrete, homomorphic]] {
            let mut members = pair.to_vec();
            members.extend_from_slice(extra);
            let union = crate::project_semantic_dispatch::canonical_algebra::intern_ordered_union(
                &graph,
                &members,
                verter_session_query::flow::policy::NullabilityPolicy::Strict,
            );
            let arms = union_arms(&graph, union.node);
            assert!(
                arms.contains(&homomorphic) && arms.contains(&concrete),
                "both mappings survive beside {} other arms",
                extra.len()
            );
        }
    }
}

/// `() => number` whose return carrier names the declared return type,
/// given as `carrier`.
fn nullary_signature(
    graph: &SemanticGraphStore,
    return_type: SemanticNodeId,
    carrier: SemanticNodeId,
) -> SemanticNodeId {
    use crate::semantic_query::{SignatureKind, SignatureReturnCarrier};
    graph.intern_node(SemanticNodeData::Signature {
        kind: SignatureKind::Call,
        params: Arc::from(Vec::new()),
        return_type,
        type_parameters: Arc::from(Vec::new()),
        occurrence: None,
        return_carrier: SignatureReturnCarrier::Declared(carrier),
        signature_span: None,
        return_type_span: None,
        predicate: None,
        is_abstract: false,
    })
}

/// A signature's key depends on its return carrier's structure, never on
/// whether the carrier is the same node as the return type or an equal
/// node interned apart.
#[test]
fn a_return_carrier_keys_by_structure_not_node_sharing() {
    let graph = SemanticGraphStore::new();
    let number = prim(&graph, PrimitiveKind::Number);
    let number_elsewhere = graph.intern_node_with_scope(
        SemanticNodeData::Primitive(PrimitiveKind::Number),
        crate::semantic_query::NodeScopeId::File {
            canonical_id: Arc::from("/elsewhere.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            whole_hash: [6u8; 16],
            local_scope: None,
        },
    );
    assert_ne!(
        number, number_elsewhere,
        "premise: two nodes of one structure"
    );
    let shared = nullary_signature(&graph, number, number);
    let apart = nullary_signature(&graph, number, number_elsewhere);
    assert!(
        stable_key_for_node(&graph, shared) == stable_key_for_node(&graph, apart),
        "one return carrier structure keys alike however it is shared"
    );
    let string = prim(&graph, PrimitiveKind::String);
    assert!(
        stable_key_for_node(&graph, shared)
            != stable_key_for_node(&graph, nullary_signature(&graph, number, string)),
        "a carrier of another structure keys apart"
    );
}

/// A mapped binder's name, for a mapping `{ [K in "a"]: <synthetic slot
/// binding> }` whose binding's backing value is `number`, lowered after
/// `padding` unrelated literals.
fn slot_binding_mapper_name(padding: usize) -> Arc<str> {
    use verter_type_expr::{
        MappedModifier, SyntheticCarrierKey, SyntheticCarrierSurfaceKind, TypeExpr,
    };
    let graph = SemanticGraphStore::new();
    pad(&graph, padding);
    let backing = prim(&graph, PrimitiveKind::Number);
    let source = Arc::new(TypeExpr::Literal(LiteralValue::String("a".into())));
    let value = Arc::new(TypeExpr::SyntheticSlotBinding(Arc::new(
        SyntheticCarrierKey {
            scope_canonical_id: Arc::from("/c.vue"),
            surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
            slot_name: Some(Arc::from("default")),
            binding_name: Arc::from("item"),
            value_node: backing.0,
        },
    )));
    crate::mapper_binder_registry::mapper_binder_decl_name(
        &graph,
        &source,
        &value,
        MappedModifier::None,
        MappedModifier::None,
        None,
    )
}

/// A mapped binder's name never carries an arena ordinal: a synthetic slot
/// binding in the mapping contributes its logical identity and its backing
/// value's structure, so interning an unrelated literal first leaves the
/// name, and every key that holds the binder, unchanged.
#[test]
fn mapped_binder_names_do_not_depend_on_arena_ordinals() {
    assert_eq!(slot_binding_mapper_name(0), slot_binding_mapper_name(1));
}

/// The arena is acyclic by contract: a payload naming a child the arena
/// could still allocate — its own id, or any later one — interns as the
/// typed `ForeignSemanticOperand` refusal, never as the payload, so no
/// cycle can form. A child from the never-allocated range dangles (and keys
/// as an absent child) without being refused.
#[test]
fn a_forward_child_fails_closed_at_interning() {
    use crate::semantic_query::QueryError;
    let graph = SemanticGraphStore::new();
    let array_of = |element: u64| {
        graph.intern_node(SemanticNodeData::Array {
            element: SemanticNodeId(element),
            readonly: false,
        })
    };
    let own = graph.node_count() as u64;
    for element in [own, own + 5] {
        let node = array_of(element);
        assert!(
            matches!(
                graph.node_data(node).as_deref(),
                Some(SemanticNodeData::Opaque(QueryError::ForeignSemanticOperand))
            ),
            "a forward child ({element}, arena at {own}) fails closed"
        );
    }
    let number = prim(&graph, PrimitiveKind::Number);
    let backward = array_of(number.0);
    assert!(
        matches!(
            graph.node_data(backward).as_deref(),
            Some(SemanticNodeData::Array { element, .. }) if *element == number
        ),
        "a child the arena holds interns as written"
    );
    let dangling = array_of(u64::MAX);
    assert!(
        matches!(
            graph.node_data(dangling).as_deref(),
            Some(SemanticNodeData::Array { .. })
        ),
        "a never-allocated child dangles"
    );
    assert!(
        stable_key_for_node(&graph, dangling)
            == stable_key_for_node(&graph, array_of(u64::MAX - 1)),
        "a dangling child keys as an absent one"
    );
}

/// A synthetic slot binding whose backing value is a node the arena could
/// still allocate is a forward reference: its constructor answers the typed
/// `StaleSemanticOperand` refusal, a not-yet-known type that never publishes
/// as a clean answer. A never-allocated ordinal dangles like any absent child.
#[test]
fn a_synthetic_binding_over_a_future_node_fails_closed() {
    use crate::semantic_query::QueryError;
    use verter_type_expr::{SyntheticCarrierKey, SyntheticCarrierSurfaceKind};
    let graph = SemanticGraphStore::new();
    let backing = prim(&graph, PrimitiveKind::Number);
    let key = |value_node: u64| SyntheticCarrierKey {
        scope_canonical_id: Arc::from("/c.vue"),
        surface_kind: SyntheticCarrierSurfaceKind::SlotBinding,
        slot_name: Some(Arc::from("default")),
        binding_name: Arc::from("item"),
        value_node,
    };
    let count = graph.node_count();
    for value_node in [count as u64, count as u64 + 5] {
        let data = SemanticNodeData::synthetic_binding(&key(value_node), count);
        assert!(
            matches!(
                data,
                SemanticNodeData::Opaque(QueryError::StaleSemanticOperand)
            ),
            "backing value {value_node} past the arena ({count}) fails closed"
        );
        assert!(
            data.means_type_is_not_yet_known(),
            "the refusal is degraded"
        );
    }
    assert!(
        matches!(
            SemanticNodeData::synthetic_binding(&key(backing.0), count),
            SemanticNodeData::SyntheticBinding { value_node, .. } if value_node == backing.0
        ),
        "a backing value the arena holds builds the carrier"
    );
    assert!(
        matches!(
            SemanticNodeData::synthetic_binding(&key(u64::MAX), count),
            SemanticNodeData::SyntheticBinding {
                value_node: u64::MAX,
                ..
            }
        ),
        "a never-allocated backing value dangles, and its consumers' seed gate refuses it"
    );
}

/// Every node is classified once: a chain of diamonds (`A(i) = [B(i),
/// C(i)]`, `B(i) = A(i + 1)[]`, `C(i) = readonly A(i + 1)[]`) reaches each
/// `A` twice, and its key stays linear in the chain.
#[test]
fn classification_visits_each_node_once() {
    use crate::semantic_query::stable_key::CLASSIFICATION_FRAMES;
    const DIAMONDS: usize = 5_000;
    let graph = SemanticGraphStore::new();
    let mut next = prim(&graph, PrimitiveKind::Number);
    for _ in 0..DIAMONDS {
        let b = graph.intern_node(SemanticNodeData::Array {
            element: next,
            readonly: false,
        });
        let c = graph.intern_node(SemanticNodeData::Array {
            element: next,
            readonly: true,
        });
        let element = |value| crate::semantic_query::TupleElement {
            label: None,
            value,
            optional: false,
            rest: false,
        };
        next = graph.intern_node(SemanticNodeData::Tuple {
            elements: Arc::from([element(b), element(c)]),
            readonly: false,
        });
    }
    let nodes = 3 * DIAMONDS as u64 + 1;
    CLASSIFICATION_FRAMES.with(|frames| frames.set(0));
    let key = stable_key_for_node(&graph, next);
    let frames = CLASSIFICATION_FRAMES.with(std::cell::Cell::get);
    assert_eq!(frames, nodes, "each of the {nodes} nodes opens one frame");
    assert!(
        key.exact().len() <= 1024 * DIAMONDS,
        "{DIAMONDS} diamonds key in {} bytes",
        key.exact().len()
    );
}

/// A chain of unions nested `depth` deep, each level `U(k) = U(k - 1)[] | k`,
/// reduced level by level through the canonical union authority.
fn nested_union_chain(graph: &SemanticGraphStore, depth: usize) -> Vec<SemanticNodeId> {
    let mut node = prim(graph, PrimitiveKind::Number);
    let mut levels = Vec::with_capacity(depth);
    for index in 0..depth {
        let wrapped = graph.intern_node(SemanticNodeData::Array {
            element: node,
            readonly: false,
        });
        let literal = graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(
            index as f64,
        )));
        node = crate::project_semantic_dispatch::canonical_algebra::intern_ordered_union(
            graph,
            &[wrapped, literal],
            verter_session_query::flow::policy::NullabilityPolicy::Strict,
        )
        .node;
        levels.push(node);
    }
    levels
}

/// Union reduction is linear in the depth of a nested union chain: each
/// level classifies its array, literal and union once, and orders the two
/// members by fingerprints folded from the class table without writing the
/// deep member's key. The bytes hashed are bounded by 256 times the key
/// bytes of the distinct subtrees (each memoizing class is folded from at
/// most 256 start states) plus one direct run per level, and the bytes
/// hashed per level stay flat as the depth quadruples. Reducing every
/// level again classifies and hashes nothing.
#[test]
fn union_reduction_work_is_linear_in_the_chain_depth() {
    use crate::semantic_query::stable_key::{CLASSIFICATION_FRAMES, FINGERPRINTED_BYTES};
    let work = || {
        (
            CLASSIFICATION_FRAMES.with(std::cell::Cell::get),
            FINGERPRINTED_BYTES.with(std::cell::Cell::get),
        )
    };
    let mut hashed = Vec::new();
    for depth in [1_000usize, 2_000, 4_000] {
        let graph = SemanticGraphStore::new();
        CLASSIFICATION_FRAMES.with(|frames| frames.set(0));
        FINGERPRINTED_BYTES.with(|bytes| bytes.set(0));
        let first = nested_union_chain(&graph, depth);
        let (frames, bytes) = work();
        assert_eq!(
            frames,
            3 * depth as u64,
            "{depth} levels classify the array, the literal and the union once each"
        );
        let distinct = graph.with_key_classes(|classes| classes.distinct_key_bytes_for_tests());
        let bound =
            256 * distinct + depth as u64 * crate::semantic_query::stable_key::FOLD_MEMO_MIN_BYTES;
        assert!(
            bytes <= bound,
            "{depth} levels hashed {bytes} bytes, more than 256 times the {distinct} key bytes of their distinct subtrees plus one direct run per level ({bound})"
        );
        assert!(
            distinct <= 64 * depth as u64,
            "the distinct subtrees hold {distinct} key bytes, more than a bounded amount per level"
        );
        hashed.push(bytes);
        let cold = work();
        let again = nested_union_chain(&graph, depth);
        assert_eq!(again, first, "premise: the same chain");
        assert_eq!(
            work(),
            cold,
            "reducing every level again classifies and hashes nothing new"
        );
        assert!(
            graph.stable_key_class_count() <= graph.stable_key_classified_count() + 1,
            "one class at most per classified node"
        );
        assert!(
            graph.stable_key_memo_count() <= 256 * graph.stable_key_class_count() + 16 * depth,
            "the memoized orders and hash-map offsets stay proportional to the classes"
        );
    }
    let per_level: Vec<u64> = hashed
        .iter()
        .zip([1_000u64, 2_000, 4_000])
        .map(|(bytes, depth)| bytes / depth)
        .collect();
    assert!(
        per_level[2] <= per_level[0] + per_level[0] / 2,
        "the bytes hashed per level grew with the depth: {per_level:?}"
    );
}

/// A folded fingerprint is exactly FNV-1a over the key's written bytes, at
/// every level of a nested union chain, and for a structure whose shared
/// subtree the key writes once and refers back to (which is hashed from
/// its written bytes instead).
#[test]
fn a_folded_fingerprint_is_the_hash_of_the_written_key() {
    use crate::semantic_query::stable_key::class_fingerprint_for_tests;
    let graph = SemanticGraphStore::new();
    let levels = nested_union_chain(&graph, 300);
    for &level in levels.iter().rev().chain(levels.iter()) {
        let key = stable_key_for_node(&graph, level);
        assert_eq!(
            class_fingerprint_for_tests(&graph, level),
            key.fingerprint(),
            "level {} ({} key bytes)",
            level.0,
            key.exact().len()
        );
        let wrapped = graph.intern_node(SemanticNodeData::Array {
            element: level,
            readonly: true,
        });
        let key = stable_key_for_node(&graph, wrapped);
        assert_eq!(
            class_fingerprint_for_tests(&graph, wrapped),
            key.fingerprint()
        );
    }
    let deep = *levels.last().expect("a chain");
    let element = |value| crate::semantic_query::TupleElement {
        label: None,
        value,
        optional: false,
        rest: false,
    };
    let shared = graph.intern_node(SemanticNodeData::Tuple {
        elements: Arc::from([element(deep), element(deep)]),
        readonly: false,
    });
    let key = stable_key_for_node(&graph, shared);
    assert!(
        key.exact().len() < 2 * stable_key_for_node(&graph, deep).exact().len(),
        "premise: the repeated subtree is written once"
    );
    assert_eq!(
        class_fingerprint_for_tests(&graph, shared),
        key.fingerprint()
    );
}

/// The key table forgets released nodes and empties once released entries
/// outnumber live ones; a key computed after either is unchanged.
#[test]
fn the_key_table_releases_the_classes_of_released_nodes() {
    let graph = SemanticGraphStore::new();
    let levels = nested_union_chain(&graph, 64);
    let deep = *levels.last().expect("a chain");
    let key = stable_key_for_node(&graph, deep);
    let classified = graph.stable_key_classified_count();
    assert!(classified >= 3 * 64, "premise: the chain is classified");
    let released =
        graph.with_key_classes(|classes| classes.release_nodes(|id| levels[..8].contains(&id)));
    assert_eq!(released, 8);
    assert_eq!(graph.stable_key_classified_count(), classified - 8);
    assert!(
        graph.stable_key_class_count() > 0,
        "a minority keeps the table"
    );
    let released = graph.with_key_classes(|classes| classes.release_nodes(|id| id.0 < deep.0));
    assert!(released > 0);
    assert_eq!(
        (
            graph.stable_key_class_count(),
            graph.stable_key_classified_count(),
            graph.stable_key_memo_count()
        ),
        (0, 0, 0),
        "a majority released empties the table"
    );
    assert_eq!(
        stable_key_for_node(&graph, deep),
        key,
        "the key rebuilds identically"
    );
}

/// Every node id a payload retains is visited by the retention walk,
/// including the ids the semantic descent treats as leaves — a recursive
/// back-edge's instantiation arguments, a class expression's prototype and
/// a pending conditional frame's parameters — so a releasing holder needs
/// no arm of its own per variant, and a forward reference hidden in any of
/// them fails closed at interning.
#[test]
fn the_retention_walk_visits_every_retained_id() {
    use crate::semantic_query::QueryError;
    let graph = SemanticGraphStore::new();
    let argument = prim(&graph, PrimitiveKind::Number);
    let back_edge = SemanticNodeData::Opaque(QueryError::RecursiveRef {
        name: Arc::from("Tree"),
        args: Arc::from([argument]),
    });
    let mut semantic = Vec::new();
    let _ = back_edge.for_each_child(|child| semantic.push(child));
    let mut retained = Vec::new();
    back_edge.for_each_retained_child(|child| retained.push(child));
    assert!(
        semantic.is_empty(),
        "premise: the semantic walk treats it as a leaf"
    );
    assert_eq!(
        retained,
        vec![argument],
        "the retention walk visits its arguments"
    );

    let prototype = prim(&graph, PrimitiveKind::String);
    let surface = prim(&graph, PrimitiveKind::Boolean);
    let instance = SemanticNodeData::ClassExpressionInstance {
        identity: Arc::new(crate::semantic_query::ClassExpressionIdentity {
            canonical_id: Arc::from("file:///a.ts"),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            offset: 0,
            name: Arc::from("C"),
            outer_clauses: Arc::from([]),
            own_arity: 0,
            constructor_visibility: None,
            prototype: Some(prototype),
            object_literal: false,
        }),
        type_arguments: Arc::from([]),
        surface,
    };
    let mut retained = Vec::new();
    instance.for_each_retained_child(|child| retained.push(child));
    assert!(
        retained.contains(&prototype) && retained.contains(&surface),
        "the retention walk visits a class expression's prototype"
    );

    let parameter = prim(&graph, PrimitiveKind::Null);
    let pending = crate::semantic_query::ConditionalPendingSubstitution::empty()
        .append_true(parameter, argument);
    let conditional = SemanticNodeData::Conditional {
        check: argument,
        extends: argument,
        true_branch_ref: argument,
        false_branch_ref: argument,
        distributive: false,
        pending: Some(Arc::new(pending)),
    };
    let mut retained = Vec::new();
    conditional.for_each_retained_child(|child| retained.push(child));
    assert!(
        retained.contains(&parameter),
        "the retention walk visits a pending frame's parameters"
    );

    let forward = graph.intern_node(SemanticNodeData::Opaque(QueryError::RecursiveRef {
        name: Arc::from("Tree"),
        args: Arc::from([SemanticNodeId(graph.node_count() as u64 + 3)]),
    }));
    assert!(
        matches!(
            graph.node_data(forward).as_deref(),
            Some(SemanticNodeData::Opaque(QueryError::ForeignSemanticOperand))
        ),
        "a forward argument of a back-edge fails closed"
    );
}

/// A checker recovery holds the authored form of the operation it refused
/// as its origin: the semantic walk treats the recovery as a leaf
/// (it is the checker's error type), the retention walk visits the
/// origin, and the stable key tells recoveries apart by that origin and by
/// their diagnostic and basis.
#[test]
fn a_checker_recovery_keys_and_retains_its_origin() {
    use crate::semantic_query::{
        CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation, QueryError,
    };
    let graph = SemanticGraphStore::new();
    let ts2590 = CheckerDiagnostic {
        code: CheckerDiagnosticCode::UnionTooComplex,
        operation: CheckerDiagnosticOperation::Intersection,
    };
    let origin = prim(&graph, PrimitiveKind::Number);
    let other_origin = prim(&graph, PrimitiveKind::String);
    let recovery = |diagnostic: CheckerDiagnostic, origin: Option<SemanticNodeId>| {
        graph.intern_node(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
            diagnostic,
            basis: crate::semantic_query::RecoveryBasis::Budget,
            origin,
        }))
    };
    let with_origin = recovery(ts2590, Some(origin));
    let data = graph.node_data(with_origin).expect("interned");
    let mut semantic = Vec::new();
    let _ = data.for_each_child(|child| semantic.push(child));
    let mut retained = Vec::new();
    data.for_each_retained_child(|child| retained.push(child));
    assert!(semantic.is_empty(), "the recovery is a semantic leaf");
    assert_eq!(
        retained,
        vec![origin],
        "the retention walk visits its origin"
    );

    let key = |node| stable_key_for_node(&graph, node);
    assert_eq!(key(with_origin), key(recovery(ts2590, Some(origin))));
    assert_ne!(key(with_origin), key(recovery(ts2590, Some(other_origin))));
    assert_ne!(key(with_origin), key(recovery(ts2590, None)));
    // A certified recovery and a budget recovery of the same diagnostic are
    // different answers: one complete, one partial.
    let with_basis = |basis| {
        graph.intern_node(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
            diagnostic: ts2590,
            basis,
            origin: Some(origin),
        }))
    };
    assert_ne!(
        key(with_basis(crate::semantic_query::RecoveryBasis::Certified)),
        key(with_basis(crate::semantic_query::RecoveryBasis::Budget))
    );
    assert_ne!(
        key(with_origin),
        key(recovery(
            CheckerDiagnostic {
                code: CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
                operation: CheckerDiagnosticOperation::Intersection,
            },
            Some(origin),
        ))
    );
    assert_ne!(
        key(with_origin),
        key(recovery(
            CheckerDiagnostic {
                code: CheckerDiagnosticCode::UnionTooComplex,
                operation: CheckerDiagnosticOperation::TemplateLiteral,
            },
            Some(origin),
        ))
    );
}

/// A document close releases a checker recovery whose origin the
/// closed document held: the recovery is interned without a scope, and the
/// cascade reaches it through the type it retains.
#[test]
fn closing_a_document_releases_a_recovery_holding_its_type() {
    use crate::semantic_query::{
        CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation, QueryError,
    };
    let graph = SemanticGraphStore::new();
    let scope = crate::semantic_query::NodeScopeId::File {
        canonical_id: Arc::from("/w/closed.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: [0u8; 16],
        local_scope: None,
    };
    let origin =
        graph.intern_node_with_scope(SemanticNodeData::Literal(LiteralValue::Number(1.0)), scope);
    let recovery = graph.intern_node(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
        diagnostic: CheckerDiagnostic {
            code: CheckerDiagnosticCode::UnionTooComplex,
            operation: CheckerDiagnosticOperation::Intersection,
        },
        basis: crate::semantic_query::RecoveryBasis::Budget,
        origin: Some(origin),
    }));
    let kept = prim(&graph, PrimitiveKind::String);
    let report = graph.release_canonical("/w/closed.ts");
    assert!(report.nodes_released >= 2, "{report:?}");
    assert!(!graph.node_is_live(origin), "the closed type is released");
    assert!(
        !graph.node_is_live(recovery),
        "the recovery holding it is released with it"
    );
    assert!(graph.node_is_live(kept), "an unrelated node stays");
}

/// A document close forgets the key classes of the nodes it released: a
/// chain interned under the closed document's scope, keyed, then released
/// with the document, leaves the key table holding at most the classes of
/// the nodes that stay live.
#[test]
fn closing_a_document_releases_its_key_classes() {
    let graph = SemanticGraphStore::new();
    let scope = crate::semantic_query::NodeScopeId::File {
        canonical_id: Arc::from("/w/closed.ts"),
        owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
        whole_hash: [0u8; 16],
        local_scope: None,
    };
    let kept = prim(&graph, PrimitiveKind::String);
    let mut node = kept;
    for index in 0..64 {
        let literal = graph.intern_node_with_scope(
            SemanticNodeData::Literal(LiteralValue::Number(f64::from(index))),
            scope.clone(),
        );
        node = graph.intern_node_with_scope(
            SemanticNodeData::Tuple {
                elements: Arc::from([
                    crate::semantic_query::TupleElement {
                        label: None,
                        value: node,
                        optional: false,
                        rest: false,
                    },
                    crate::semantic_query::TupleElement {
                        label: None,
                        value: literal,
                        optional: false,
                        rest: false,
                    },
                ]),
                readonly: false,
            },
            scope.clone(),
        );
    }
    let kept_key = stable_key_for_node(&graph, kept);
    let _ = stable_key_for_node(&graph, node);
    assert!(
        graph.stable_key_classified_count() > 64,
        "premise: the closed document's chain is classified"
    );
    let report = graph.release_canonical("/w/closed.ts");
    assert!(report.nodes_released >= 128, "premise: {report:?}");
    assert!(
        graph.stable_key_classified_count() <= 1,
        "only the live primitive may keep its class, not {}",
        graph.stable_key_classified_count()
    );
    assert!(graph.stable_key_class_count() <= 1);
    assert_eq!(stable_key_for_node(&graph, kept), kept_key);
}

/// The positions a per-entry search of the sorted list recovers: each
/// entry ranks at its node's first occurrence in union order.
fn searched_union_rank_order(graph: &SemanticGraphStore, members: &[SemanticNodeId]) -> Vec<usize> {
    let mut sorted = members.to_vec();
    crate::semantic_query::stable_key::sort_union_members_by_stable_key(graph, &mut sorted);
    let mut order: Vec<usize> = (0..members.len()).collect();
    order.sort_by_key(|&index| {
        sorted
            .iter()
            .position(|node| *node == members[index])
            .expect("every member is in its own sorted list")
    });
    order
}

/// Shuffled arms over `distinct` number literals, every third literal
/// listed twice at separate positions.
fn shuffled_arms_with_duplicates(
    graph: &SemanticGraphStore,
    distinct: usize,
) -> Vec<SemanticNodeId> {
    let literals: Vec<SemanticNodeId> = (0..distinct)
        .map(|value| {
            graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(
                value as f64,
            )))
        })
        .collect();
    let mut arms = literals.clone();
    arms.extend(literals.iter().step_by(3).copied());
    // A fixed multiplicative walk shuffles the arms without a seed source.
    let len = arms.len();
    let stride = (0..len)
        .map(|offset| len / 2 + 1 + offset)
        .find(|stride| gcd(*stride, len) == 1)
        .expect("some stride is coprime with the arm count");
    (0..len).map(|step| arms[(step * stride) % len]).collect()
}

fn gcd(a: usize, b: usize) -> usize {
    if b == 0 {
        a
    } else {
        gcd(b, a % b)
    }
}

/// Union rank recovery answers exactly the order a per-entry search of the
/// sorted list does — repeated node ids tie at their first occurrence and
/// keep input order, in the forward and the reversed store order — and its
/// rank-map probes grow linearly with the arm count from 128 to 1024 arms.
#[test]
fn union_rank_order_matches_searched_ranks_with_linear_probes() {
    use crate::semantic_query::stable_key::union_rank_order_observed;
    let mut probes_per_arm: Vec<(usize, usize)> = Vec::new();
    for distinct in [96, 192, 384, 768] {
        for reversed in [false, true] {
            let graph = SemanticGraphStore::new();
            if reversed {
                graph.reverse_union_order_for_tests();
            }
            let arms = shuffled_arms_with_duplicates(&graph, distinct);
            let (order, probes) = union_rank_order_observed(&graph, &arms);
            assert_eq!(
                order,
                searched_union_rank_order(&graph, &arms),
                "{} arms (reversed: {reversed}) recover the searched ranks",
                arms.len()
            );
            for pair in order.windows(2) {
                if arms[pair[0]] == arms[pair[1]] {
                    assert!(pair[0] < pair[1], "repeated arms keep input order");
                }
            }
            if !reversed {
                probes_per_arm.push((arms.len(), probes));
            }
        }
    }
    assert_eq!(
        probes_per_arm
            .iter()
            .map(|(arms, _)| *arms)
            .collect::<Vec<_>>(),
        [128, 256, 512, 1024]
    );
    for (arms, probes) in &probes_per_arm {
        assert_eq!(
            *probes,
            2 * arms,
            "{arms} arms recover their ranks with one insert and one lookup per arm"
        );
    }
}
