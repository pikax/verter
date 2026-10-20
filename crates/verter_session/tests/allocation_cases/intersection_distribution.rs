//! An intersection's distribution against the allocator: its cross product
//! is weighed from the arms' widths with nothing allocated, and is paid for
//! — work units and construction bytes — before its first constituent
//! exists, so a product the checker refuses, or one the demand cannot pay
//! for, allocates the same however wide it is written.

use verter_session_query::flow::policy::NullabilityPolicy;
use verter_type_engine::project_semantic_dispatch::canonical_algebra::{
    intern_ordered_intersection_paying_for_tests, intern_ordered_union,
    weigh_cross_product_for_tests, PaidConstruction,
};
use verter_type_engine::semantic_query::{
    DeclIdentity, LiteralValue, PartialReasonSet, PrimitiveKind, SemanticNodeData, SemanticNodeId,
};
use verter_type_engine::semantic_query_memo::SemanticGraphStore;

use super::{alloc_bytes, alloc_count, reset_alloc_counter};

/// `0 | … | count - 1`, from `offset`, beside `object` when `with_object`:
/// a union of primitive types without it, which the checker intersects
/// member-wise, and with it one whose cross product the checker builds.
fn numbers(
    graph: &SemanticGraphStore,
    offset: usize,
    count: usize,
    with_object: bool,
) -> SemanticNodeId {
    let mut members: Vec<SemanticNodeId> = (offset..offset + count)
        .map(|i| graph.intern_node(SemanticNodeData::Literal(LiteralValue::Number(i as f64))))
        .collect();
    if with_object {
        members.push(graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Object)));
    }
    intern_ordered_union(graph, &members, NullabilityPolicy::Strict).node
}

/// The allocations, and the bytes requested, of `body`.
fn allocations<R>(body: impl FnOnce() -> R) -> (R, u64, u64) {
    reset_alloc_counter();
    let result = body();
    (result, alloc_count(), alloc_bytes())
}

/// Weighing reads each arm's width off its node: under the checker's limit
/// and at it, the product's width is answered with nothing allocated.
#[test]
fn weighing_a_cross_product_allocates_nothing() {
    let graph = SemanticGraphStore::new();
    let _scope = graph.enter_node_root_scope();
    for (left, right, expected) in [
        (369, 271, Some(99_999)),
        (400, 250, None),
        (4_000, 2_500, None),
    ] {
        let arms = [
            numbers(&graph, 0, left, false),
            numbers(&graph, 0, right, false),
        ];
        let (width, count, bytes) = allocations(|| weigh_cross_product_for_tests(&graph, &arms));
        assert_eq!(width, expected, "{left} × {right}");
        assert_eq!(
            (count, bytes),
            (0, 0),
            "{left} × {right}: weighing allocates nothing"
        );
    }
}

/// A product the checker refuses (TS2590) is refused before anything is
/// built or paid: written ten times wider, it allocates exactly as much.
#[test]
fn a_refused_product_allocates_the_same_however_wide() {
    let graph = SemanticGraphStore::new();
    let _scope = graph.enter_node_root_scope();
    let mut measured = Vec::new();
    for scale in [1, 10] {
        let arms = [
            numbers(&graph, 0, 400 * scale, true),
            numbers(&graph, 0, 250 * scale, true),
        ];
        // The first construction interns the recovery; the measured one
        // finds every node it names already interned.
        let first =
            intern_ordered_intersection_paying_for_tests(&graph, &arms, usize::MAX, usize::MAX);
        let (paid, count, bytes) = allocations(|| {
            intern_ordered_intersection_paying_for_tests(&graph, &arms, usize::MAX, usize::MAX)
        });
        assert_eq!(paid, first);
        assert!(
            matches!(
                graph.node_data(paid.node).as_deref(),
                Some(SemanticNodeData::Opaque(_))
            ),
            "the refused product is its recovery"
        );
        assert_eq!(
            (paid.work, paid.bytes),
            (0, 0),
            "a refused product is never paid for"
        );
        measured.push((count, bytes));
    }
    assert_eq!(
        measured[0], measured[1],
        "a refusal ten times wider allocates the same: {measured:?}"
    );
}

/// A product under the checker's limit is paid for whole, one work unit
/// per constituent, before any is built: an allowance one unit short — of
/// work, or of construction bytes with work to spare — builds nothing. Its
/// allocations follow the arms it reads, never the product: arms ten times
/// wider, a product ninety times wider, allocate at most ten times as much.
#[test]
fn an_unpaid_product_builds_nothing() {
    let graph = SemanticGraphStore::new();
    let _scope = graph.enter_node_root_scope();
    let mut measured = Vec::new();
    for count in [20, 200] {
        let arms = [
            numbers(&graph, 0, count, true),
            numbers(&graph, 1_000, count, true),
        ];
        let width = (count + 1) * (count + 1);
        let built =
            intern_ordered_intersection_paying_for_tests(&graph, &arms, usize::MAX, usize::MAX);
        assert_eq!(built.exhausted, PartialReasonSet::empty());
        assert_eq!(built.work, width, "every constituent is paid for, once");
        let short_of_work =
            intern_ordered_intersection_paying_for_tests(&graph, &arms, width - 1, usize::MAX);
        let (short_of_bytes, calls, bytes) = allocations(|| {
            intern_ordered_intersection_paying_for_tests(&graph, &arms, usize::MAX, built.bytes - 1)
        });
        for (short, rail) in [
            (short_of_work, PartialReasonSet::PROJECTION_WORK_LIMIT),
            (short_of_bytes, PartialReasonSet::CONNECTED_MEMORY_LIMIT),
        ] {
            assert_eq!(short.exhausted, rail, "{count}: the rail that refused");
            assert_eq!(short.bytes, 0, "{count}: nothing built is paid for");
            assert!(
                matches!(
                    graph.node_data(short.node).as_deref(),
                    Some(SemanticNodeData::Intersection(members)) if members.len() == 2
                ),
                "{count}: the unpaid product stands as the written intersection"
            );
        }
        measured.push((calls, bytes));
    }
    let [(_, narrow), (_, wide)] = measured[..] else {
        unreachable!()
    };
    assert!(
        wide <= 10 * narrow,
        "an unpaid product allocates with its arms, not its product: {measured:?}"
    );
}

/// A divided intersection's halves are built, so each is paid for before it
/// is: `X & Y & Z` divides into `X` and `Y & Z`, and an allowance one unit
/// short of `Y & Z`'s product builds nothing at all. Measured cold, before
/// anything of the product exists, the refused construction allocates with
/// the arms it reads: halves whose arms are ten times wider, a product
/// ninety-four times wider, allocate at most twenty times as much.
#[test]
fn a_divided_half_is_paid_for_before_it_is_built() {
    {
        let graph = SemanticGraphStore::new();
        let _scope = graph.enter_node_root_scope();
        let arms = [
            numbers(&graph, 0, 40, true),
            numbers(&graph, 100, 50, true),
            numbers(&graph, 200, 50, true),
        ];
        let half = 51 * 51;
        let built =
            intern_ordered_intersection_paying_for_tests(&graph, &arms, usize::MAX, usize::MAX);
        assert_eq!(built.exhausted, PartialReasonSet::empty());
        // `Y & Z` keeps only `object` (the numbers are disjoint), so the
        // product of the halves is `X`'s 41.
        assert_eq!(
            built.work,
            half + 41,
            "both products are paid for: {built:?}"
        );
    }
    let mut measured = Vec::new();
    for count in [30, 300] {
        // A graph of its own: nothing of the product is interned before the
        // refused construction runs.
        let graph = SemanticGraphStore::new();
        let _scope = graph.enter_node_root_scope();
        let arms = [
            numbers(&graph, 0, 40, true),
            numbers(&graph, 1_000, count, true),
            numbers(&graph, 2_000, count, true),
        ];
        let half = (count + 1) * (count + 1);
        let (
            PaidConstruction {
                exhausted,
                work,
                bytes,
                ..
            },
            calls,
            allocated,
        ) = allocations(|| {
            intern_ordered_intersection_paying_for_tests(&graph, &arms, half - 1, usize::MAX)
        });
        assert_eq!(exhausted, PartialReasonSet::PROJECTION_WORK_LIMIT);
        assert_eq!(
            (work, bytes),
            (0, 0),
            "{count}: the unpaid half pays nothing"
        );
        measured.push((calls, allocated));
    }
    let [(_, narrow), (_, wide)] = measured[..] else {
        unreachable!()
    };
    assert!(
        wide <= 20 * narrow,
        "the unpaid half builds nothing: it allocates with its arms, not its product: \
         {measured:?}"
    );
}

/// `<prefix>0 | … | <prefix><count - 1>`: a union of distinct type
/// parameters, no two of which an intersection reduces.
fn parameters(graph: &SemanticGraphStore, prefix: &str, count: usize) -> SemanticNodeId {
    let members: Vec<SemanticNodeId> = (0..count)
        .map(|i| {
            let name = format!("{prefix}{i}");
            graph.intern_node(SemanticNodeData::TypeParam {
                decl: DeclIdentity::synthetic(&name),
                param_index: 0,
                constraint: None,
                default: None,
                display_name: verter_type_expr::OwnedStr::from(name.as_str()),
            })
        })
        .collect();
    intern_ordered_union(graph, &members, NullabilityPolicy::Strict).node
}

/// A divided product's constituents hold every member its halves'
/// constituents hold: `A & B & C & D` over four ten-member unions divides
/// into `A & B` and `C & D`, a hundred two-member intersections each, and
/// its ten thousand constituents are four-member intersections. Each is
/// paid for at the members it flattens to: beside the two halves and the
/// two-member product `E & F` of the same width, the divided product pays
/// exactly two more member slots per constituent.
#[test]
fn a_divided_product_pays_for_the_members_its_constituents_flatten_to() {
    let graph = SemanticGraphStore::new();
    let _scope = graph.enter_node_root_scope();
    let [a, b, c, d] = ["a", "b", "c", "d"].map(|prefix| parameters(&graph, prefix, 10));
    let [e, f] = ["e", "f"].map(|prefix| parameters(&graph, prefix, 100));
    let paid = |arms: &[SemanticNodeId]| {
        let paid =
            intern_ordered_intersection_paying_for_tests(&graph, arms, usize::MAX, usize::MAX);
        assert_eq!(paid.exhausted, PartialReasonSet::empty(), "{paid:?}");
        paid
    };
    let half = paid(&[a, b]);
    assert_eq!(half.work, 100, "a half is a hundred constituents");
    let divided = paid(&[a, b, c, d]);
    assert_eq!(divided.work, 100 + 100 + 10_000, "{divided:?}");
    let pair = paid(&[e, f]);
    assert_eq!(pair.work, 10_000, "{pair:?}");
    assert_eq!(
        divided.bytes - 2 * half.bytes - pair.bytes,
        10_000 * 2 * std::mem::size_of::<SemanticNodeId>(),
        "each constituent pays for the four members it holds, not two"
    );
    let short = intern_ordered_intersection_paying_for_tests(
        &graph,
        &[a, b, c, d],
        usize::MAX,
        divided.bytes - 1,
    );
    assert_eq!(
        short.exhausted,
        PartialReasonSet::CONNECTED_MEMORY_LIMIT,
        "an enabled byte rail one byte short of that footprint refuses it"
    );
}
