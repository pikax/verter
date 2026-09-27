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
    let union = crate::project_semantic_dispatch::canonical_algebra::intern_ordered_union(
        &graph,
        &members,
        crate::semantic_query::NullabilityPolicy::Strict,
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
            crate::semantic_query::NullabilityPolicy::Strict,
        );
        let arm_union = intern_ordered_union(
            &self.graph,
            &input(self.a_arm, self.b_arm),
            crate::semantic_query::NullabilityPolicy::Strict,
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

/// An array node whose element is `next`, an id the store has not minted yet.
fn array_of(graph: &SemanticGraphStore, next: u64) -> SemanticNodeId {
    graph.intern_node(SemanticNodeData::Array {
        element: SemanticNodeId(next),
        readonly: false,
    })
}

/// A true cycle — a node reaching itself, directly or through another node
/// — terminates at a back-reference to its open frame's level, so its key
/// is finite and the same in stores where the cycle sits at other ids.
#[test]
fn true_cycles_terminate_at_level_back_references() {
    let cycles = |padding: usize| {
        let graph = SemanticGraphStore::new();
        for index in 0..padding {
            let _ = lit_str(&graph, &format!("padding-{index}"));
        }
        let own = graph.node_count() as u64;
        let self_cycle = array_of(&graph, own);
        assert_eq!(
            self_cycle,
            SemanticNodeId(own),
            "the fixture needs a self edge"
        );
        let first = graph.node_count() as u64;
        let head = array_of(&graph, first + 1);
        let tail = graph.intern_node(SemanticNodeData::Tuple {
            elements: Arc::from([crate::semantic_query::TupleElement {
                label: None,
                value: SemanticNodeId(first),
                optional: false,
                rest: false,
            }]),
            readonly: false,
        });
        assert_eq!(
            (head, tail),
            (SemanticNodeId(first), SemanticNodeId(first + 1)),
            "the fixture needs a two-node cycle"
        );
        [
            stable_key_for_node(&graph, self_cycle),
            stable_key_for_node(&graph, head),
            stable_key_for_node(&graph, tail),
        ]
    };
    let near = cycles(0);
    let far = cycles(17);
    assert!(
        near == far,
        "a cycle's key does not depend on where its nodes sit"
    );
    // The self cycle is an array whose element is a back-reference to the
    // root frame, level 0.
    let back_reference = [1u8, 7, 1, 0, 0, 0, 0];
    let mut expected = vec![1u8, 6, 3, 0];
    expected.extend_from_slice(&(back_reference.len() as u32).to_le_bytes());
    expected.extend_from_slice(&back_reference);
    assert_eq!(near[0].exact(), expected.as_slice());
    assert!(
        near[1] != near[2],
        "each entry point of a cycle keys its own shape"
    );
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
