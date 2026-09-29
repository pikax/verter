//! An intersection over unions whose cross product reaches the checker's
//! limit is the checker's TS2590 recovery (`checkCrossProductUnion` in
//! `getIntersectionType`): the product is checked before a constituent is
//! built, after the checker's own strategies — unions of primitives
//! intersect member-wise, unions that all hold `undefined` intersect without
//! it, and three or more written types divide in half — and the answer
//! reads and relates as `any`. Under the limit the intersection distributes.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured with `tsc
//! --declaration --emitDeclarationOnly` on the fixture each test builds,
//! each probe read off a TS2322 against `never`. The four `strictNullChecks`
//! × `noImplicitAny` settings agree on every probe.

use super::checker_probe_lane_tests::{mismatches, mismatches_in_one_host, with_probe};
use crate::semantic_query::{
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
            ("IsAny<R5>", "\"any\""),
            ("[R5] extends [{ k0: 0 }] ? 1 : 2", "1"),
            ("IsAny<R4>", "\"not-any\""),
            ("[R4] extends [{ k0: 0 }] ? 1 : 2", "2"),
            (wide.as_str(), "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The recovery is the checker's error type carrying TS2590, and beyond it
/// the intersection as written: the type Verter still names past the
/// checker's limit.
#[test]
fn the_ts2590_recovery_holds_the_written_intersection_beyond_it() {
    with_probe(&ten_by_ten(), "U0 & U1 & U2 & U3 & U4", |dispatch, node| {
        let data = dispatch.graph().node_data(node);
        let Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
            diagnostic,
            beyond: Some(beyond),
        })) = data.as_deref()
        else {
            panic!("the intersection must be the TS2590 recovery, measured {data:?}");
        };
        assert_eq!(*diagnostic, TS2590);
        assert!(
            matches!(
                dispatch.graph().node_data(*beyond).as_deref(),
                Some(SemanticNodeData::Intersection(arms)) if arms.len() == 5
            ),
            "beyond the limit is the written five-arm intersection"
        );
    });
}

/// Two object unions at the limit: 369 × 271 = 99,999 distributes and 400 ×
/// 250 = 100,000 does not.
///
/// Measured: `IsAny<R>` is `"not-any"` and `[R] extends [{ a: 0 }] ? 1 : 2`
/// is `2` at 369 × 271; `any` and `1` under TS2590 at 400 × 250.
#[test]
fn two_object_unions_meet_the_limit_at_one_hundred_thousand() {
    for (a, b, is_any, relation) in [(369, 271, "\"not-any\"", "2"), (400, 250, "\"any\"", "1")] {
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
        (991, "\"any\"", "1"),
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
/// thousand-member number unions are the thousand numbers they share.
///
/// Measured: `IsAny<R>` is `"not-any"`, `[R] extends [0 | 1] ? 1 : 2` is `2`
/// and `R extends number ? 1 : 2` is `1`, with no diagnostic.
#[test]
fn unions_of_primitives_intersect_without_a_cross_product() {
    let source = format!(
        "{IS_ANY}{}{}type R = A & B;\n",
        literal_union("A", 1000, false, ""),
        literal_union("B", 1000, false, "")
    );
    let failures = mismatches(
        &source,
        &[
            ("IsAny<R>", "\"not-any\""),
            ("[R] extends [0 | 1] ? 1 : 2", "2"),
            ("R extends number ? 1 : 2", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
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
        &[
            ("IsAny<R>", "\"any\""),
            ("[R] extends [{ a: 0 }] ? 1 : 2", "1"),
        ],
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
            ("IsAny<O5[\"m\"]>", "\"any\""),
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
    let over = ("IsAny<R5>", "\"any\"");
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
