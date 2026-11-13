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
    mismatches, mismatches_in_one_host, mismatches_on_host, probe_host, upsert_probe_files,
    with_probe, with_probe_on_host, with_recovered_probe, ProbeProject,
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

/// The probe module over the `A` and `B` an [`operands_host`] declares.
const OVER_OPS: &str = "import type { A, B } from './ops';\n\
                        type IsAny<T> = 0 extends 1 & T ? \"any\" : \"not-any\";\n\
                        type R = A & B;\n";

/// `ops.ts` exporting the declarations `a` and `b`.
fn ops(a: String, b: String) -> String {
    format!("export {a}export {b}")
}

/// A host whose `ops.ts` is `operands`, for every probe of one fixture to
/// read: the operands' file stays as it is while each probe rewrites the
/// probe module, so the probes read one reduction of their intersection
/// and only the first pays for its cross product.
fn operands_host(operands: &str) -> std::sync::Arc<crate::VerterHost> {
    let host = probe_host(ProbeProject::default());
    upsert_probe_files(&host, &[("ops.ts", operands)]);
    host
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
///
/// Each size is read on one host ([`operands_host`]), so its probes read
/// one reduction of the intersection.
#[test]
fn two_object_unions_meet_the_limit_at_one_hundred_thousand() {
    for (a, b, is_any, relation) in [(369, 271, "\"not-any\"", "2"), (400, 250, "any", "1")] {
        let host = operands_host(&ops(object_union("A", "a", a), object_union("B", "b", b)));
        let failures = mismatches_on_host(
            &host,
            ProbeProject::default(),
            OVER_OPS,
            &[
                ("IsAny<R>", is_any),
                ("[R] extends [{ a: 0 }] ? 1 : 2", relation),
            ],
        );
        assert!(failures.is_empty(), "{a} × {b}: {}", failures.join("\n"));
        with_probe_on_host(
            &host,
            ProbeProject::default(),
            OVER_OPS,
            "A & B",
            |dispatch, node| {
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
            },
        );
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
///
/// The gate checks 40, whose whole product would be over the limit, and 991,
/// the first width whose divided product is. 990, the widest that builds
/// (99,990 constituents), is checked by
/// [`nightly_scale::three_written_types_build_up_to_the_limit`]. The limit
/// itself counts as the checker counts
/// (`checker_policy::tests::the_cross_product_rule_counts_as_the_checker_counts`).
#[test]
fn three_written_types_divide_before_the_product_is_checked() {
    assert_three_written_types_divide(&[(40, "\"not-any\"", "2"), (991, "any", "1")]);
}

mod nightly_scale {
    use super::*;

    /// [`super::three_written_types_divide_before_the_product_is_checked`] at
    /// `|X|` 990, whose divided product of 99,990 constituents is built.
    #[test]
    fn three_written_types_build_up_to_the_limit() {
        assert_three_written_types_divide(&[(990, "\"not-any\"", "2")]);
    }
}

/// `X & Y & Z` at each `(|X|, IsAny<R>, relation)` row answers as the
/// checker does.
#[track_caller]
fn assert_three_written_types_divide(rows: &[(usize, &str, &str)]) {
    for &(x, is_any, relation) in rows {
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
///
/// Each size is read on one host ([`operands_host`]), so its probes read
/// one reduction of the intersection.
#[test]
fn unions_with_undefined_intersect_without_it() {
    let with_undefined = |name: &str, key: &str, count: usize| {
        object_union(name, key, count).replace(";\n", " | undefined;\n")
    };
    let under = operands_host(&ops(
        with_undefined("A", "a", 316),
        with_undefined("B", "b", 316),
    ));
    let over = operands_host(&ops(
        with_undefined("A", "a", 317),
        with_undefined("B", "b", 316),
    ));
    let mut failures = mismatches_on_host(
        &under,
        ProbeProject::default(),
        OVER_OPS,
        &[
            ("[R] extends [{ a: 0 }] ? 1 : 2", "2"),
            ("[undefined] extends [R] ? 1 : 2", "1"),
        ],
    );
    failures.extend(mismatches_on_host(
        &over,
        ProbeProject::default(),
        OVER_OPS,
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
    let graph = dispatch.graph_arc();
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
    let right = intern_ordered_union(
        verter_type_engine::semantic_query_memo::GraphIn::<verter_audit::observe::Uncaptured>::of(
            &graph,
        ),
        &[s, t],
        NullabilityPolicy::Strict,
    )
    .node;
    let arms = |nested: SemanticNodeId| {
        [
            intern_ordered_union(verter_type_engine::semantic_query_memo::GraphIn::<verter_audit::observe::Uncaptured>::of(&graph), &[nested, r], NullabilityPolicy::Strict).node,
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

/// The cross products of intersections over unions built on `host`,
/// wherever they ran.
fn products_built(host: &crate::VerterHost) -> u64 {
    host.project_type_store()
        .semantic_graph()
        .stats_snapshot()
        .intersection_products_built
}

/// One intersection over unions is reduced once for all its readers on a
/// host: the first relation from `A & B` builds its cross product, and every
/// later probe of it — another relation, a conditional over it — reads that
/// reduction instead of building the product again. The reduction is its
/// operands': an edit to one operand builds the new operands' product once,
/// and the answers read it.
#[test]
fn every_reader_of_one_intersection_reads_one_product() {
    let five = || ops(object_union("A", "a", 5), object_union("B", "b", 4));
    let host = operands_host(&five());
    let read = |row: (&str, &str)| {
        let before = products_built(&host);
        let failures = mismatches_on_host(&host, ProbeProject::default(), OVER_OPS, &[row]);
        assert!(failures.is_empty(), "{}", failures.join("\n"));
        products_built(&host) - before
    };
    let within = "[R] extends [{ a: 0 | 1 | 2 | 3 | 4 }] ? 1 : 2";
    let readers = |within_answer: &'static str| {
        [
            (within, within_answer),
            ("[R] extends [{ a: 0 }] ? 1 : 2", "2"),
            ("IsAny<R>", "\"not-any\""),
            (within, within_answer),
        ]
    };
    let first: Vec<u64> = readers("1").into_iter().map(read).collect();
    assert_eq!(
        first,
        [1, 0, 0, 0],
        "the first probe builds the product once, and every later reader reads it"
    );
    upsert_probe_files(
        &host,
        &[(
            "ops.ts",
            ops(object_union("A", "a", 6), object_union("B", "b", 4)).as_str(),
        )],
    );
    let edited: Vec<u64> = readers("2").into_iter().map(read).collect();
    assert_eq!(
        edited,
        [1, 0, 0, 0],
        "an edited operand's product is built once and read for every answer"
    );
}

/// `{ <key>: 0 } | … | { <key>: <count - 1> }`, written inline.
fn inline_objects(key: &str, count: usize) -> String {
    (0..count)
        .map(|i| format!("{{ {key}: {i} }}"))
        .collect::<Vec<_>>()
        .join(" | ")
}

/// The checker's weighing of whether it refuses the intersection of
/// `probe`'s tuple elements (`intersection_cross_product_refused`), paying
/// an allowance of `work` units, and the cross products built while it ran.
fn weigh_tuple(
    probe: &str,
    work: usize,
) -> (
    verter_type_engine::project_semantic_dispatch::canonical_algebra::Weighing,
    u64,
) {
    use verter_session_query::flow::policy::NullabilityPolicy;
    use verter_type_engine::project_semantic_dispatch::canonical_algebra::intersection_cross_product_refused_paying_for_tests;
    with_probe("", probe, |dispatch, node| {
        let (_, elements) = tuple_factors(dispatch, node);
        let store = dispatch.graph().store();
        let before = store.stats_snapshot().intersection_products_built;
        let weighing = intersection_cross_product_refused_paying_for_tests(
            store,
            &elements,
            NullabilityPolicy::Strict,
            work,
            usize::MAX,
        );
        let built = store.stats_snapshot().intersection_products_built - before;
        (weighing, built)
    })
}

/// `[1, A | undefined, B | undefined]` over 316 objects a side: the
/// checker divides it into `1` and `(A | undefined) & (B | undefined)`,
/// whose 99,856 combinations and the `undefined` added back are under the
/// limit, so `1 & …` is too.
fn one_and_316_with_undefined() -> String {
    format!(
        "[1, {} | undefined, {} | undefined]",
        inline_objects("a", 316),
        inline_objects("b", 316)
    )
}

/// Weighing whether the checker refuses a divided intersection builds
/// neither half when their widths prove it under the limit: `1 & R` with
/// `R` the 99,857-wide `(A | undefined) & (B | undefined)` is accepted with
/// no cross product built. A whole `IsAny<R>` probe, which weighs it from
/// every reader, builds at most the one product the relation reads.
#[test]
fn weighing_a_divided_intersection_builds_neither_half() {
    let (weighing, built) = weigh_tuple(&one_and_316_with_undefined(), usize::MAX);
    assert!(!weighing.refused, "1 × 99,857 is under the limit");
    assert_eq!(built, 0, "the weighing builds no cross product");

    let with_undefined = |name: &str, key: &str, count: usize| {
        object_union(name, key, count).replace(";\n", " | undefined;\n")
    };
    let host = operands_host(&ops(
        with_undefined("A", "a", 316),
        with_undefined("B", "b", 316),
    ));
    let before = products_built(&host);
    let failures = mismatches_on_host(
        &host,
        ProbeProject::default(),
        OVER_OPS,
        &[("IsAny<R>", "\"not-any\"")],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let built = products_built(&host) - before;
    assert!(built <= 1, "the probe builds {built} cross products");
}

/// A weighing that cannot prove a divided intersection under the limit
/// from widths builds it as the checker does, and answers what the built
/// halves answer: `X & Y & Z` with `Y` 50 numbers and `{ y: 1 }` and `Z` 50
/// strings and `{ z: 1 }` bounds `Y & Z` at 2,601 but builds 101, so at
/// `|X|` 40 (4,040) it is accepted and at 991 (100,091) refused, as
/// [`three_written_types_divide_before_the_product_is_checked`] measured.
#[test]
fn a_weighing_the_widths_cannot_decide_builds_the_halves() {
    let numbers = (0..50)
        .map(|i| i.to_string())
        .collect::<Vec<_>>()
        .join(" | ");
    let strings = (0..50)
        .map(|i| format!("\"s{i}\""))
        .collect::<Vec<_>>()
        .join(" | ");
    for (x, refused) in [(40, false), (991, true)] {
        let probe = format!(
            "[{}, {numbers} | {{ y: 1 }}, {strings} | {{ z: 1 }}]",
            inline_objects("x", x)
        );
        let (weighing, _) = weigh_tuple(&probe, usize::MAX);
        assert_eq!(weighing.refused, refused, "|X| = {x}");
    }
}

/// The bound adds back one constituent for each `null` / `undefined`
/// member the checker strips: `W & (A | undefined) & (B | undefined)` with
/// four objects in `W`, 39 in `A` and 641 in `B` divides into `W` and a
/// half of 39 × 641 = 24,999 combinations and `undefined`, 25,000, and the
/// halves' product, 100,000, is the checker's limit.
#[test]
fn a_stripped_nullish_member_counts_in_the_bound() {
    let probe = format!(
        "[{}, {} | undefined, {} | undefined]",
        inline_objects("w", 4),
        inline_objects("a", 39),
        inline_objects("b", 641)
    );
    let (weighing, _) = weigh_tuple(&probe, usize::MAX);
    assert!(weighing.refused, "4 × 25,000 reaches the limit");
}

/// A half the checker refuses on its own stops the weighing with that
/// refusal before the other half is built: `X & Y & (A | undefined) & (B |
/// undefined)` divides into `X & Y` (100) and a half of 317 × 316 =
/// 100,172, which is refused, with no cross product built.
#[test]
fn a_half_refused_on_its_own_refuses_with_nothing_built() {
    let probe = format!(
        "[{}, {}, {} | undefined, {} | undefined]",
        inline_objects("x", 10),
        inline_objects("y", 10),
        inline_objects("a", 317),
        inline_objects("b", 316)
    );
    let (weighing, built) = weigh_tuple(&probe, usize::MAX);
    assert!(weighing.refused, "the second half is over the limit");
    assert_eq!(built, 0, "neither half is built");
}

/// Weighing pays for no product: an allowance far too small for the
/// 99,856 combinations of `(A | undefined) & (B | undefined)` still weighs
/// `1 & …` as accepted, complete, with nothing paid.
#[test]
fn weighing_a_divided_intersection_pays_for_no_product() {
    let (weighing, _) = weigh_tuple(&one_and_316_with_undefined(), 10);
    assert!(!weighing.refused, "1 × 99,857 is under the limit");
    assert!(
        weighing.exhausted.is_empty(),
        "the weighing exhausts no rail: {:?}",
        weighing.exhausted
    );
    assert_eq!(weighing.work, 0, "the weighing pays for no product");
}
