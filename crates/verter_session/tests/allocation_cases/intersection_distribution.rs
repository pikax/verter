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
    LiteralValue, PartialReasonSet, PrimitiveKind, SemanticNodeData, SemanticNodeId,
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
/// short of `Y & Z`'s product builds nothing at all.
#[test]
fn a_divided_half_is_paid_for_before_it_is_built() {
    let graph = SemanticGraphStore::new();
    let _scope = graph.enter_node_root_scope();
    let arms = [
        numbers(&graph, 0, 40, true),
        numbers(&graph, 100, 50, true),
        numbers(&graph, 200, 50, true),
    ];
    let half = 51 * 51;
    let built = intern_ordered_intersection_paying_for_tests(&graph, &arms, usize::MAX, usize::MAX);
    assert_eq!(built.exhausted, PartialReasonSet::empty());
    // `Y & Z` keeps only `object` (the numbers are disjoint), so the
    // product of the halves is `X`'s 41.
    assert_eq!(
        built.work,
        half + 41,
        "both products are paid for: {built:?}"
    );
    let PaidConstruction {
        exhausted,
        work,
        bytes,
        ..
    } = intern_ordered_intersection_paying_for_tests(&graph, &arms, half - 1, usize::MAX);
    assert_eq!(exhausted, PartialReasonSet::PROJECTION_WORK_LIMIT);
    assert_eq!((work, bytes), (0, 0), "the unpaid half builds nothing");
}
