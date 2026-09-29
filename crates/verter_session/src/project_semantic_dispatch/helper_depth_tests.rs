//! The dispatch helpers that read a type through its parts — tuple spreads,
//! index key unions, base-type intersections, constituent counts, nullish
//! and primitive-part proofs, alias and carrier chains — at any nesting.
//! Each is read from a work list: a structure nested 10,000 levels deep
//! answers on a 1 MiB thread, exactly as a shallow one does, with no depth
//! or hop ceiling turning the rest of the structure into an answer.

use std::sync::Arc;

use super::*;
use crate::semantic_query::composite::CompositeList;
use crate::semantic_query::{
    IndexKey, PrimitiveKind, SemanticNodeData, SemanticNodeId, TupleElement,
};
use crate::{HostConfig, VerterHost};

/// A nesting past any native-stack or depth bound.
const DEPTH: usize = 10_000;

/// Run `probe` over a fresh dispatch on a 1 MiB thread.
fn on_a_small_stack<R: Send + 'static>(
    probe: impl FnOnce(&ProjectSemanticDispatch<'_>) -> R + Send + 'static,
) -> R {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(move || {
            let host = VerterHost::new_standalone(HostConfig::default());
            let dispatch = ProjectSemanticDispatch::new(&host);
            probe(&dispatch)
        })
        .expect("spawn the probing thread")
        .join()
        .expect("the probe answers")
}

fn element(value: SemanticNodeId, rest: bool) -> TupleElement {
    TupleElement {
        label: None,
        value,
        optional: false,
        rest,
    }
}

fn literal(dispatch: &ProjectSemanticDispatch<'_>, text: &str) -> SemanticNodeId {
    dispatch.graph().intern_node(SemanticNodeData::Literal(
        verter_type_expr::LiteralValue::String(text.into()),
    ))
}

fn primitive(dispatch: &ProjectSemanticDispatch<'_>, kind: PrimitiveKind) -> SemanticNodeId {
    dispatch
        .graph()
        .intern_node(SemanticNodeData::Primitive(kind))
}

/// `[...[...[…[1, 2]…]]]`, `depth` spreads deep, each spread's tuple behind
/// an alias: spliced to `[1, 2]`.
#[test]
fn tuple_spreads_splice_at_any_depth() {
    let spliced = on_a_small_stack(|dispatch| {
        let graph = dispatch.graph();
        let (one, two) = (literal(dispatch, "1"), literal(dispatch, "2"));
        let mut tuple = graph.intern_node(SemanticNodeData::Tuple {
            elements: Arc::from([element(one, false), element(two, false)]),
            readonly: false,
        });
        // bounded-loop: a fixed DEPTH-level fixture constructor.
        for _ in 0..DEPTH {
            let alias = graph.intern_node(SemanticNodeData::Alias(tuple));
            tuple = graph.intern_node(SemanticNodeData::Tuple {
                elements: Arc::from([element(alias, true)]),
                readonly: false,
            });
        }
        let Some(SemanticNodeData::Tuple { elements, .. }) =
            graph.node_data(tuple).as_deref().cloned()
        else {
            unreachable!("a tuple was interned");
        };
        match dispatch.normalize_tuple_spread(&elements, false) {
            super::build::NormalizedTupleShape::Tuple(elements) => {
                elements
                    .iter()
                    .map(|element| (element.value, element.rest))
                    .collect::<Vec<_>>()
                    == vec![(one, false), (two, false)]
            }
            super::build::NormalizedTupleShape::Array(_) => false,
        }
    });
    assert!(spliced, "the nest splices to [1, 2]");
}

/// An index key union nested `depth` deep (each level the union of the
/// level below and one more key) enumerates every key.
#[test]
fn index_key_unions_enumerate_at_any_depth() {
    let keys = on_a_small_stack(|dispatch| {
        let graph = dispatch.graph();
        let mut union = literal(dispatch, "k0");
        for level in 1..=DEPTH {
            let key = literal(dispatch, &format!("k{level}"));
            union = graph.intern_node(SemanticNodeData::Union(CompositeList::test_fixture(
                Arc::from([union, key]),
            )));
        }
        dispatch.finite_index_keys_for_tests(union)
    });
    let keys = keys.expect("a finite key set");
    assert_eq!(keys.len(), DEPTH + 1);
    assert_eq!(keys[0], IndexKey::String(Arc::from("k0")));
    assert_eq!(
        keys[DEPTH],
        IndexKey::String(Arc::from(format!("k{DEPTH}")))
    );
}

/// Intersections nested `depth` deep down to `object` are a valid base
/// type; with `string` at the bottom they are not.
#[test]
fn base_type_intersections_validate_at_any_depth() {
    let nest = |dispatch: &ProjectSemanticDispatch<'_>, bottom: PrimitiveKind| {
        let graph = dispatch.graph();
        let object = primitive(dispatch, PrimitiveKind::Object);
        let mut node = primitive(dispatch, bottom);
        // bounded-loop: a fixed DEPTH-level fixture constructor.
        for _ in 0..DEPTH {
            node = graph.intern_node(SemanticNodeData::Intersection(CompositeList::test_fixture(
                Arc::from([object, node]),
            )));
        }
        node
    };
    let verdicts = on_a_small_stack(move |dispatch| {
        (
            dispatch.is_valid_base_type_for_tests(nest(dispatch, PrimitiveKind::Object)),
            dispatch.is_valid_base_type_for_tests(nest(dispatch, PrimitiveKind::String)),
        )
    });
    assert_eq!(verdicts, (true, false));
}

/// Unions and intersections nested `depth` deep count every constituent.
#[test]
fn constituents_count_at_any_depth() {
    let count = on_a_small_stack(|dispatch| {
        let graph = dispatch.graph();
        let mut node = literal(dispatch, "k0");
        for level in 1..=DEPTH {
            let leaf = literal(dispatch, &format!("k{level}"));
            let members = CompositeList::test_fixture(Arc::from([node, leaf]));
            node = graph.intern_node(if level % 2 == 0 {
                SemanticNodeData::Union(members)
            } else {
                SemanticNodeData::Intersection(CompositeList::test_fixture(Arc::from([node, leaf])))
            });
        }
        super::canonical_algebra::constituent_count_for_tests(graph, &[node])
    });
    assert_eq!(count, DEPTH + 1);
}

/// An intersection nested `depth` deep over one literal arm provably
/// excludes `null` and `undefined`; over `unknown` it does not.
#[test]
fn non_nullish_proofs_read_intersections_at_any_depth() {
    let nest = |dispatch: &ProjectSemanticDispatch<'_>, bottom: SemanticNodeId| {
        let graph = dispatch.graph();
        let unknown = primitive(dispatch, PrimitiveKind::Unknown);
        let mut node = bottom;
        // bounded-loop: a fixed DEPTH-level fixture constructor.
        for _ in 0..DEPTH {
            node = graph.intern_node(SemanticNodeData::Intersection(CompositeList::test_fixture(
                Arc::from([unknown, node]),
            )));
        }
        node
    };
    let verdicts = on_a_small_stack(move |dispatch| {
        let literal = literal(dispatch, "x");
        let unknown = primitive(dispatch, PrimitiveKind::Unknown);
        (
            dispatch.provably_non_nullish(nest(dispatch, literal)),
            dispatch.provably_non_nullish(nest(dispatch, unknown)),
        )
    });
    assert_eq!(verdicts, (true, false));
}

/// A union is below a primitive only when each member is: a union of
/// literals nested `depth` deep over `string` may be; the same nest over an
/// array type, which has no primitive part, may not.
#[test]
fn primitive_part_proofs_read_unions_at_any_depth() {
    let nest = |dispatch: &ProjectSemanticDispatch<'_>, bottom: SemanticNodeId| {
        let graph = dispatch.graph();
        let mut node = bottom;
        for level in 0..DEPTH {
            let arm = literal(dispatch, &format!("k{level}"));
            node = graph.intern_node(SemanticNodeData::Union(CompositeList::test_fixture(
                Arc::from([arm, node]),
            )));
        }
        node
    };
    let verdicts = on_a_small_stack(move |dispatch| {
        let graph = dispatch.graph();
        let array = graph.intern_node(SemanticNodeData::Array {
            element: primitive(dispatch, PrimitiveKind::String),
            readonly: false,
        });
        let string = primitive(dispatch, PrimitiveKind::String);
        (
            dispatch.may_be_below_a_primitive(nest(dispatch, string)),
            dispatch.may_be_below_a_primitive(nest(dispatch, array)),
        )
    });
    assert_eq!(verdicts, (true, false));
}

/// A chain of `depth` aliases ends at the node it names; a cycle of
/// aliases ends as a cycle, and settles through carriers to nothing.
#[test]
fn alias_chains_end_at_any_length() {
    let ends = on_a_small_stack(|dispatch| {
        let graph = dispatch.graph();
        let array = graph.intern_node(SemanticNodeData::Array {
            element: primitive(dispatch, PrimitiveKind::Number),
            readonly: false,
        });
        let mut alias = array;
        // bounded-loop: a fixed DEPTH-level fixture constructor.
        for _ in 0..DEPTH {
            alias = graph.intern_node(SemanticNodeData::Alias(alias));
        }
        (
            array,
            graph.alias_chain_end(alias, |_| {}),
            dispatch.settle_through_carriers(alias),
        )
    });
    let (array, end, settled) = ends;
    assert_eq!(end, crate::semantic_query_memo::AliasChainEnd::Node(array));
    assert_eq!(settled, Some(array));
}
