//! An intersection over unions whose cross product reaches the checker's
//! limit is the checker's TS2590 recovery (`checkCrossProductUnion` in
//! `getIntersectionType`): the product is checked before a constituent is
//! built, after the checker's own strategies — unions of primitives
//! intersect member-wise, unions that all hold `undefined` intersect without
//! it, and three or more written types divide in half — and the answer
//! reads and relates as `any` — a resource partial, never kept. Under the
//! limit the intersection distributes.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured with `tsc
//! --declaration --emitDeclarationOnly` on the fixture each test builds,
//! each probe read off a TS2322 against `never`. The four `strictNullChecks`
//! × `noImplicitAny` settings agree on every probe.

use super::checker_probe_lane_tests::{
    mismatches, mismatches_in_one_host, with_probe, with_recovered_probe,
};
use verter_type_engine::semantic_query::{
    CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation, QueryError,
    SemanticNodeData,
};

const IS_ANY: &str = "type IsAny<T> = 0 extends 1 & T ? \"any\" : \"not-any\";\n";

/// `type <name> = { <key>: 0 } | … | { <key>: <count - 1> };`
fn object_union(name: &str, key: &str, count: usize) -> String {
    let arms: Vec<String> = (0..count).map(|i| format!("{{ {key}: {i} }}")).collect();
    format!("type {name} = {};\n", arms.join(" | "))
}

/// `type <name> = <prefix>0 | … | <prefix><count - 1><extra>;` over number
/// literals, or string literals when `prefix` is quoted.
fn literal_union(name: &str, count: usize, quoted: bool, extra: &str) -> String {
    let arms: Vec<String> = (0..count)
        .map(|i| {
            if quoted {
                format!("\"s{i}\"")
            } else {
                i.to_string()
            }
        })
        .collect();
    format!("type {name} = {}{extra};\n", arms.join(" | "))
}

/// Five unions of ten objects `U0 … U4`, with `R5 = U0 & … & U4` and `R4 =
/// U0 & … & U3`.
fn ten_by_ten() -> String {
    let mut source = String::from(IS_ANY);
    for i in 0..5 {
        source.push_str(&object_union(&format!("U{i}"), &format!("k{i}"), 10));
    }
    source.push_str("type R5 = U0 & U1 & U2 & U3 & U4;\ntype R4 = U0 & U1 & U2 & U3;\n");
    source
}

/// The TS2590 recovery of an intersection.
const TS2590: CheckerDiagnostic = CheckerDiagnostic {
    code: CheckerDiagnosticCode::UnionTooComplex,
    operation: CheckerDiagnosticOperation::Intersection,
};

/// Five ten-object unions intersected: the checker divides them `U0 & U1`
/// (100) and `U2 & U3 & U4` (1,000), and the halves' product, 100,000, is
/// its limit. Four of them, 10,000, distribute.
///
/// Measured: `IsAny<R5>` is `any` and `[R5] extends [{ k0: 0 }] ? 1 : 2` is
/// `1`, both under TS2590; `IsAny<R4>` is `"not-any"`, `[R4] extends [{ k0:
/// 0 }] ? 1 : 2` is `2` and `[R4] extends [{ k0: 0 | … | 9; k3: 0 | … | 9 }]
/// ? 1 : 2` is `1`.
#[test]
fn five_ten_member_unions_intersect_to_the_ts2590_recovery() {
    let digits = "0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9";
    let wide = format!("[R4] extends [{{ k0: {digits}; k3: {digits} }}] ? 1 : 2");
    let failures = mismatches(
        &ten_by_ten(),
        &[
            ("IsAny<R5>", "any"),
            ("[R5] extends [{ k0: 0 }] ? 1 : 2", "1"),
            ("IsAny<R4>", "\"not-any\""),
            ("[R4] extends [{ k0: 0 }] ? 1 : 2", "2"),
            (wide.as_str(), "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The recovery is the checker's error type carrying TS2590, a resource
/// partial holding the intersection as written as its origin.
#[test]
fn the_ts2590_recovery_is_a_partial_holding_the_written_intersection() {
    with_recovered_probe(&ten_by_ten(), "U0 & U1 & U2 & U3 & U4", |dispatch, node| {
        let data = dispatch.graph().node_data(node);
        let Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
            diagnostic,
            basis: verter_type_engine::semantic_query::RecoveryBasis::Budget,
            origin: Some(origin),
        })) = data.as_deref()
        else {
            panic!("the intersection must be the TS2590 recovery, measured {data:?}");
        };
        assert_eq!(*diagnostic, TS2590);
        assert!(
            matches!(
                dispatch.graph().node_data(*origin).as_deref(),
                Some(SemanticNodeData::Intersection(arms)) if arms.len() == 5
            ),
            "its origin is the written five-arm intersection"
        );
    });
}

/// Two object unions at the limit: 369 × 271 = 99,999 distributes and 400 ×
/// 250 = 100,000 does not. Under the limit the intersection is the type it
/// denotes; at the limit it is the checker's TS2590 recovery, holding the
/// written intersection as its origin.
///
/// Measured: `IsAny<R>` is `"not-any"` and `[R] extends [{ a: 0 }] ? 1 : 2`
/// is `2` at 369 × 271; `any` and `1` under TS2590 at 400 × 250.
#[test]
fn two_object_unions_meet_the_limit_at_one_hundred_thousand() {
    for (a, b, is_any, relation) in [(369, 271, "\"not-any\"", "2"), (400, 250, "any", "1")] {
        let source = format!(
            "{IS_ANY}{}{}type R = A & B;\n",
            object_union("A", "a", a),
            object_union("B", "b", b)
        );
        let failures = mismatches(
            &source,
            &[
                ("IsAny<R>", is_any),
                ("[R] extends [{ a: 0 }] ? 1 : 2", relation),
            ],
        );
        assert!(failures.is_empty(), "{a} × {b}: {}", failures.join("\n"));
        with_probe(&source, "A & B", |dispatch, node| {
            let data = dispatch.graph().node_data(node);
            match data.as_deref() {
                Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
                    diagnostic,
                    basis: verter_type_engine::semantic_query::RecoveryBasis::Budget,
                    origin: Some(origin),
                })) => {
                    assert!(
                        a * b >= 100_000,
                        "{a} × {b} is under the limit, measured {data:?}"
                    );
                    assert_eq!(*diagnostic, TS2590);
                    assert!(
                        matches!(
                            dispatch.graph().node_data(*origin).as_deref(),
                            Some(SemanticNodeData::Intersection(arms)) if arms.len() == 2
                        ),
                        "its origin is the written `A & B`"
                    );
                }
                other => assert!(
                    a * b < 100_000 && !matches!(other, Some(SemanticNodeData::Opaque(_))),
                    "{a} × {b}: measured {other:?}"
                ),
            }
        });
    }
}

/// Three written types divide in half before the product is checked: `X &
/// Y & Z`, with `Y` 50 numbers and `{ y: 1 }` and `Z` 50 strings and `{ z:
/// 1 }`, is `X & (Y & Z)`, and `Y & Z` keeps 101 of its 2,601 combinations
/// (a number and a string are disjoint). The product whole is over the
/// limit at every width of `X` here; divided, it is `|X| × 101`.
///
/// Measured: at `|X|` 40 (4,040) and 990 (99,990) `IsAny<R>` is
/// `"not-any"` and `[R] extends [{ x: 0 }] ? 1 : 2` is `2`; at 991
/// (100,091) they are `any` and `1` under TS2590.
#[test]
fn three_written_types_divide_before_the_product_is_checked() {
    for (x, is_any, relation) in [
        (40, "\"not-any\"", "2"),
        (990, "\"not-any\"", "2"),
        (991, "any", "1"),
    ] {
        let source = format!(
            "{IS_ANY}{}{}{}type R = X & Y & Z;\n",
            object_union("X", "x", x),
            literal_union("Y", 50, false, " | { y: 1 }"),
            literal_union("Z", 50, true, " | { z: 1 }"),
        );
        let failures = mismatches(
            &source,
            &[
                ("IsAny<R>", is_any),
                ("[R] extends [{ x: 0 }] ? 1 : 2", relation),
            ],
        );
        assert!(failures.is_empty(), "|X| = {x}: {}", failures.join("\n"));
    }
}

/// Unions of primitives intersect member-wise, with no cross product: two
/// thousand-member number unions are the thousand numbers they share,
/// written as aliases or inline. A union that also holds an object is not a
/// union of primitives, and its cross product is weighed.
///
/// Measured: `IsAny<R>` is `"not-any"`, `[R] extends [0 | 1] ? 1 : 2` is `2`
/// and `R extends number ? 1 : 2` is `1`, with no diagnostic; written
/// inline, `[(0 | … | 999) & (500 | … | 1499)] extends [500 | 501] ? 1 : 2`
/// is `2` with no diagnostic; and `IsAny<(0 | … | 399 | { x: 1 }) & (0 | … |
/// 399 | { y: 1 })>` (401 × 401 = 160,801) is `any` under TS2590.
#[test]
fn unions_of_primitives_intersect_without_a_cross_product() {
    let numbers = |from: usize, count: usize| {
        (from..from + count)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let source = format!(
        "{IS_ANY}{}{}type R = A & B;
",
        literal_union("A", 1000, false, ""),
        literal_union("B", 1000, false, "")
    );
    let inline = format!(
        "[({}) & ({})] extends [500 | 501] ? 1 : 2",
        numbers(0, 1000),
        numbers(500, 1000)
    );
    let mixed = format!(
        "IsAny<({} | {{ x: 1 }}) & ({} | {{ y: 1 }})>",
        numbers(0, 400),
        numbers(0, 400)
    );
    let failures = mismatches(
        &source,
        &[
            ("IsAny<R>", "\"not-any\""),
            ("[R] extends [0 | 1] ? 1 : 2", "2"),
            ("R extends number ? 1 : 2", "1"),
            (inline.as_str(), "2"),
            (mixed.as_str(), "any"),
        ],
    );
    assert!(
        failures.is_empty(),
        "{}",
        failures.join(
            "
"
        )
    );
}

/// Unions that all hold `undefined` intersect without it and add it back:
/// `(A | undefined) & (B | undefined)` weighs `A & B`. With 316 objects on
/// both sides that is 99,856; with 317 on one, 100,172.
///
/// Measured: at 316 × 316 `[R] extends [{ a: 0 }] ? 1 : 2` is `2` and
/// `[undefined] extends [R] ? 1 : 2` is `1`; at 317 × 316 `IsAny<R>` is
/// `any` and `[R] extends [{ a: 0 }] ? 1 : 2` is `1` under TS2590 (without
/// `strictNullChecks` the unions hold no `undefined`, and the products are
/// the same).
#[test]
fn unions_with_undefined_intersect_without_it() {
    let with_undefined = |name: &str, key: &str, count: usize| {
        object_union(name, key, count).replace(";\n", " | undefined;\n")
    };
    let under = format!(
        "{IS_ANY}{}{}type R = A & B;\n",
        with_undefined("A", "a", 316),
        with_undefined("B", "b", 316)
    );
    let over = format!(
        "{IS_ANY}{}{}type R = A & B;\n",
        with_undefined("A", "a", 317),
        with_undefined("B", "b", 316)
    );
    let mut failures = mismatches(
        &under,
        &[
            ("[R] extends [{ a: 0 }] ? 1 : 2", "2"),
            ("[undefined] extends [R] ? 1 : 2", "1"),
        ],
    );
    failures.extend(mismatches(
        &over,
        &[("IsAny<R>", "any"), ("[R] extends [{ a: 0 }] ? 1 : 2", "1")],
    ));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A member read through an intersection intersects the members' types:
/// `{ m: U0 } & … & { m: U4 }` reads `m` as `U0 & … & U4`, the TS2590
/// recovery; three of them read the distributed type.
///
/// Measured: `IsAny<O5["m"]>` is `any` under TS2590 and `IsAny<O3["m"]>` is
/// `"not-any"`.
#[test]
fn a_member_read_over_the_limit_is_the_ts2590_recovery() {
    let source = format!(
        "{}type O5 = {{ m: U0 }} & {{ m: U1 }} & {{ m: U2 }} & {{ m: U3 }} & {{ m: U4 }};\n\
         type O3 = {{ m: U0 }} & {{ m: U1 }} & {{ m: U2 }};\n",
        ten_by_ten()
    );
    let failures = mismatches(
        &source,
        &[
            ("IsAny<O5[\"m\"]>", "any"),
            ("IsAny<O3[\"m\"]>", "\"not-any\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The answer at the limit is the same read cold or warm, and in either
/// order: the product is weighed from the intersection alone, whatever an
/// earlier read left in the host's memo.
#[test]
fn the_limit_answers_the_same_cold_warm_and_reordered() {
    let over = ("IsAny<R5>", "any");
    let under = ("IsAny<R4>", "\"not-any\"");
    let over_relation = ("[R5] extends [{ k0: 0 }] ? 1 : 2", "1");
    let under_relation = ("[R4] extends [{ k0: 0 }] ? 1 : 2", "2");
    let mut failures = mismatches_in_one_host(
        &ten_by_ten(),
        &[over, under, over_relation, under_relation, over, under],
    );
    failures.extend(mismatches_in_one_host(
        &ten_by_ten(),
        &[
            under_relation,
            over_relation,
            under,
            over,
            under_relation,
            over_relation,
        ],
    ));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A member read through an intersection intersects the members' types the
/// way the checker's own intersection does, so unions of primitives
/// intersect member-wise before any product is weighed: two thousand-member
/// number unions read as the five hundred numbers they share, never refused
/// on their million-wide written product. A union that also holds an object
/// is not a union of primitives, and its cross product is weighed.
///
/// Measured: `IsAny<O["m"]>` is `"not-any"`, `[O["m"]] extends [500 | 501]
/// ? 1 : 2` is `2` and `O["m"] extends number ? 1 : 2` is `1`, with no
/// diagnostic; `IsAny<M["m"]>` (401 × 401 = 160,801) is `any` under TS2590.
#[test]
fn a_member_read_over_unions_of_primitives_intersects_them_member_wise() {
    let numbers = |from: usize, count: usize| {
        (from..from + count)
            .map(|i| i.to_string())
            .collect::<Vec<_>>()
            .join(" | ")
    };
    let source = format!(
        "{IS_ANY}{}type B = {};\ntype O = {{ m: A }} & {{ m: B }};\n\
         type M = {{ m: {} | {{ x: 1 }} }} & {{ m: {} | {{ y: 1 }} }};\n",
        literal_union("A", 1000, false, ""),
        numbers(500, 1000),
        numbers(0, 400),
        numbers(0, 400),
    );
    let failures = mismatches(
        &source,
        &[
            ("IsAny<O[\"m\"]>", "\"not-any\""),
            ("[O[\"m\"]] extends [500 | 501] ? 1 : 2", "2"),
            ("O[\"m\"] extends number ? 1 : 2", "1"),
            ("IsAny<M[\"m\"]>", "any"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The factors a member read distributes `probe`'s tuple elements as (each
/// element's union members, or the element alone), and the elements as
/// the written intersection.
fn tuple_factors(
    dispatch: &super::ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
    node: verter_type_engine::semantic_query::SemanticNodeId,
) -> (
    Vec<Vec<verter_type_engine::semantic_query::SemanticNodeId>>,
    Vec<verter_type_engine::semantic_query::SemanticNodeId>,
) {
    let graph = dispatch.graph();
    let elements: Vec<_> = match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Tuple { elements, .. }) => {
            elements.iter().map(|element| element.value).collect()
        }
        other => panic!("the probe reads a tuple, got {other:?}"),
    };
    let factors = elements
        .iter()
        .map(|element| match graph.node_data(*element).as_deref() {
            Some(SemanticNodeData::Union(members)) => members.iter().copied().collect(),
            _ => vec![*element],
        })
        .collect();
    (factors, elements)
}

/// The work units a member read's distribution of `probe`'s tuple
/// elements charges its connected demand, under `strictNullChecks`.
fn distribution_work(probe: &str) -> usize {
    use verter_session_query::flow::policy::NullabilityPolicy;
    with_probe("", probe, |dispatch, node| {
        let (factors, origin) = tuple_factors(dispatch, node);
        let (_demand, trip) = dispatch.enter_connected_demand(false);
        assert!(trip.is_none(), "a fresh demand has not tripped");
        let before = dispatch.connected_demand().work_used_for_tests();
        dispatch
            .distribute_intersection_for_tests(&factors, &origin, NullabilityPolicy::Strict)
            .expect("the intersection distributes");
        dispatch.connected_demand().work_used_for_tests() - before
    })
}

/// A member read divides `X & (A | B | undefined) & (C | D | undefined)`
/// as the checker does, into `X` and a half, and intersects that half the
/// way the checker's own intersection does: its factors all hold
/// `undefined`, so it intersects without it — four combinations, not the
/// nine its written width multiplies to — and adds it back. Beside the same
/// read without `undefined`, the half pays for no combination more: the
/// read pays only for the `undefined` part the half adds back and the one
/// `X & undefined` combination of the halves' product. Weighed at its
/// written width, the half would pay for five combinations more.
#[test]
fn a_divided_half_intersects_without_the_nullish_member_its_factors_share() {
    let nullish = distribution_work(
        "[{ x: 1 }, { a: 1 } | { b: 1 } | undefined, { c: 1 } | { d: 1 } | undefined]",
    );
    let plain = distribution_work("[{ x: 1 }, { a: 1 } | { b: 1 }, { c: 1 } | { d: 1 }]");
    assert_eq!(
        nullish - plain,
        2,
        "the shared `undefined` costs its part and one combination, not the half's \
         five more: {nullish} against {plain}"
    );
}

/// A distribution outside any connected demand is weighed against the
/// checker's limit and charged to no ledger: an earlier demand's trip,
/// which the ledger still holds once that demand ends, is never inherited
/// as the distribution's own refusal.
#[test]
fn a_distribution_outside_a_demand_never_inherits_an_earlier_trip() {
    use verter_session_query::flow::policy::NullabilityPolicy;
    with_probe(
        "",
        "[{ x: 1 } | { y: 1 }, { a: 1 } | { b: 1 }]",
        |dispatch, node| {
            let (factors, origin) = tuple_factors(dispatch, node);
            dispatch.set_connected_limits_for_tests(1, 24);
            {
                let (_demand, _) = dispatch.enter_connected_demand(false);
                assert!(
                    dispatch.connected_demand().charge_units(2).is_err(),
                    "the earlier demand trips"
                );
            }
            assert!(
                dispatch
                    .distribute_intersection_for_tests(&factors, &origin, NullabilityPolicy::Strict)
                    .is_some(),
                "the intersection distributes outside the demand, never stopped by the \
                 earlier demand's trip"
            );
        },
    );
}

/// A constituent's footprint is sized by the members it holds once its
/// nested intersections flatten, each intersection spliced once as the
/// constituent builder splices it: a shared DAG `I0 = p & q`,
/// `I(n + 1) = I(n) & I(n)` flattens to `p` and `q` however deep, so
/// distributing `(I8 | r) & (s | t)` pays exactly what `(I0 | r) & (s | t)`
/// pays, and fits the byte allowance the shallow form needs.
#[test]
fn a_shared_intersection_dag_is_charged_for_its_flattened_members() {
    use std::sync::Arc;
    use verter_session_query::flow::policy::NullabilityPolicy;
    use verter_type_engine::project_semantic_dispatch::canonical_algebra::{
        intern_ordered_intersection_paying_for_tests, intern_ordered_union,
    };
    use verter_type_engine::semantic_query::composite::CompositeList;
    use verter_type_engine::semantic_query::{DeclIdentity, PartialReasonSet, SemanticNodeId};
    use verter_type_engine::semantic_query_memo::SemanticGraphStore;

    let graph = SemanticGraphStore::new();
    let _scope = graph.enter_node_root_scope();
    let param = |name: &str| {
        graph.intern_node(SemanticNodeData::TypeParam {
            decl: DeclIdentity::synthetic(name),
            param_index: 0,
            constraint: None,
            default: None,
            display_name: verter_type_expr::OwnedStr::from(name),
        })
    };
    let carrier = |members: [SemanticNodeId; 2]| {
        graph.intern_node(SemanticNodeData::Intersection(
            CompositeList::ordered_carrier_for_tests(Arc::from(members)),
        ))
    };
    let [p, q, r, s, t] = ["P", "Q", "R", "S", "T"].map(param);
    let shallow = carrier([p, q]);
    let deep = (0..8).fold(shallow, |inner, _| carrier([inner, inner]));
    let right = intern_ordered_union(&graph, &[s, t], NullabilityPolicy::Strict).node;
    let arms = |nested: SemanticNodeId| {
        [
            intern_ordered_union(&graph, &[nested, r], NullabilityPolicy::Strict).node,
            right,
        ]
    };
    let paying = |nested: SemanticNodeId, bytes: usize| {
        intern_ordered_intersection_paying_for_tests(&graph, &arms(nested), usize::MAX, bytes)
    };
    let control = paying(shallow, usize::MAX);
    assert_eq!(control.exhausted, PartialReasonSet::empty(), "{control:?}");
    assert_eq!(control.work, 4, "two by two constituents");
    let nested = paying(deep, usize::MAX);
    assert_eq!(
        (nested.work, nested.bytes),
        (control.work, control.bytes),
        "the shared DAG flattens to the members the shallow form holds"
    );
    assert_eq!(
        paying(deep, control.bytes).exhausted,
        PartialReasonSet::empty(),
        "the shallow form's allowance pays for the shared DAG"
    );
}
