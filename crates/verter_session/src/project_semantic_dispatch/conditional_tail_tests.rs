//! A conditional alias whose selected branch applies the alias again runs
//! as the checker's tail loop (`getConditionalType`): each step is the
//! alias over the next arguments, evaluated in place rather than as a
//! nested instantiation, and a run fails with TS2589 at the checker's tail
//! limit of 1,000 steps. An application with arguments of its own (`A8<Eat<I>>`)
//! evaluates them before the step; arguments that come round again can never
//! reach a value, which is TS2589 at once. The TS2589 recovery is the
//! checker's error type: it relates as `any` does (assignable to everything
//! but `never`) and, as a conditional's check or extends type, is the
//! conditional's answer. It is a resource partial, never kept.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured with `tsc
//! --declaration --emitDeclarationOnly` on [`FIXTURE`], each probe read off a
//! TS2322 against `never`. The four `strictNullChecks` × `noImplicitAny`
//! settings agree on every probe.

use super::checker_probe_lane_tests::{mismatches, mismatches_in_one_host, with_recovered_probe};
use crate::semantic_query::{
    CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation, QueryError,
    SemanticNodeData,
};

const FIXTURE: &str = r#"
type Rep<N extends number, Acc extends unknown[] = []> = Acc["length"] extends N ? Acc : Rep<N, [...Acc, 0]>;
type Len<T extends unknown[]> = T["length"];
type TrimLeft<S extends string> = S extends ` ${infer R}` ? TrimLeft<R> : S;
type Count<S extends string, Acc extends unknown[] = []> = S extends `${string}${infer R}` ? Count<R, [...Acc, 0]> : Acc["length"];
type Rev<T extends unknown[], Acc extends unknown[] = []> = T extends [infer H, ...infer R] ? Rev<R, [H, ...Acc]> : Acc;
type T8<I extends string> = I extends `${infer A}${infer B}` ? T8<B> : "done";
type Eat<T> = T extends `${infer A}${infer B}` ? B : "";
type A8<I extends string> = I extends `${infer A}${infer B}` ? A8<Eat<I>> : "done";
type Tl<T extends any[]> = T extends [any, ...infer R] ? R : [];
type A11<N extends any[]> = N extends [any, ...infer R] ? A11<Tl<N>> : "done";
type L<S extends string, N extends unknown[] = []> = S extends `${infer _}${infer R}` ? L<R, [...N, 0]> : N["length"];
type Aw<T> = T extends { then(f: (v: infer V) => any): any } ? Aw<V> : T;
type Th1 = { then(f: (v: Th2) => any): any };
type Th2 = { then(f: (v: 42) => any): any };
type F<T> = T extends 0 ? 1 : F<0>;
type Same<T> = T extends 0 ? Same<T> : 1;
type Loop<T> = T extends 0 ? Loop<[T]> : Loop<[T]>;
type E = Same<0>;
type R1<I extends string> = I extends `${infer A}${infer B}` ? [A, ...R1<B>] : [];
"#;

/// The TS2589 recovery of a conditional tail run.
const TS2589: CheckerDiagnostic = CheckerDiagnostic {
    code: CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
    operation: CheckerDiagnosticOperation::ConditionalTail,
};

/// Tail-recursive conditional aliases run to their value.
///
/// Measured: `TrimLeft<"   x">` is `"x"`, `Count<"abcd">` is `4`,
/// `Count<"">` is `0`, `Rev<[1, 2, 3]>` is `[3, 2, 1]`, `T8<"aa">` is
/// `"done"`, `A8<"aa">` is `"done"`, `A11<[1, 2, 3]>` is `"done"`,
/// `L<"p\nq">` is `3`, `Aw<Th1>` is `42`, `F<2>` is `1` and `Len<Rep<5>>`
/// is `5`.
#[test]
fn a_tail_recursive_conditional_runs_to_its_value() {
    let failures = mismatches_in_one_host(
        FIXTURE,
        &[
            (r#"TrimLeft<"   x">"#, r#""x""#),
            (r#"Count<"abcd">"#, "4"),
            (r#"Count<"">"#, "0"),
            ("Rev<[1, 2, 3]>", "[3, 2, 1]"),
            (r#"T8<"aa">"#, r#""done""#),
            (r#"A8<"aa">"#, r#""done""#),
            ("A11<[1, 2, 3]>", r#""done""#),
            (r#"L<"p\nq">"#, "3"),
            ("Aw<Th1>", "42"),
            ("F<2>", "1"),
            ("Len<Rep<5>>", "5"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A tail run fails at the checker's 1,000th step: `Rep<999>` builds its
/// 999 elements and `Rep<1000>` is the TS2589 recovery.
///
/// Measured: `Len<Rep<999>>` is `999`; `Rep<1000>` is `any` under TS2589.
#[test]
fn a_tail_run_fails_with_ts2589_at_the_checker_step() {
    let failures = mismatches(FIXTURE, &[("Len<Rep<999>>", "999")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_ts2589(FIXTURE, "Rep<1000>");
}

/// A run whose arguments come round again never reaches a value: the
/// checker's count is certain to run out, so the application is the TS2589
/// recovery at once.
///
/// Measured: `Same<0>` and `Loop<0>` are `any` under TS2589 (`Loop`'s
/// arguments grow, and it reaches the tail limit).
#[test]
fn a_run_that_cannot_reach_a_value_is_ts2589() {
    assert_ts2589(FIXTURE, "Same<0>");
    assert_ts2589(FIXTURE, "Loop<0>");
}

/// The TS2589 recovery relates as the checker's error type: assignable to
/// everything but `never`, and as a conditional's check or extends type it
/// is the conditional's answer. So does `any`.
///
/// Measured: `[any] extends [never] ? 1 : 2` is `2`, `[E] extends [never] ?
/// 1 : 2` is `2`, `[never] extends [E] ? 1 : 2` is `1`, `[E] extends
/// [string] ? 1 : 2` is `1`, and `E extends string ? 1 : 2` and `0 extends E
/// ? 1 : 2` are `any`.
#[test]
fn the_ts2589_recovery_relates_as_the_error_type() {
    let failures = mismatches(
        FIXTURE,
        &[
            ("[any] extends [never] ? 1 : 2", "2"),
            ("[E] extends [never] ? 1 : 2", "2"),
            ("[never] extends [E] ? 1 : 2", "1"),
            ("[E] extends [string] ? 1 : 2", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_ts2589(FIXTURE, "E extends string ? 1 : 2");
    assert_ts2589(FIXTURE, "0 extends E ? 1 : 2");
}

/// The answers are the same read cold or warm, and in either order: a run is
/// counted from its own start, whatever an earlier read left in the memo.
#[test]
fn a_tail_run_answers_the_same_cold_warm_and_reordered() {
    let rows = [
        (r#"Count<"abcd">"#, "4"),
        ("Len<Rep<5>>", "5"),
        ("[E] extends [never] ? 1 : 2", "2"),
        (r#"A8<"aa">"#, r#""done""#),
    ];
    let mut failures = mismatches_in_one_host(
        FIXTURE,
        &[rows[0], rows[1], rows[2], rows[3], rows[0], rows[2]],
    );
    failures.extend(mismatches_in_one_host(
        FIXTURE,
        &[rows[3], rows[2], rows[1], rows[0], rows[3], rows[1]],
    ));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A recursive application that is not the selected branch itself (here a
/// tuple's rest element) is instantiated as the checker instantiates it,
/// nested within its instantiation depth.
///
/// Measured: `R1<"abc">` is `["a", "b", "c"]`.
#[test]
#[ignore = "a recursive application in a tuple rest element stays a recursive reference instead of being instantiated"]
fn a_recursive_application_inside_a_branch_is_instantiated() {
    let failures = mismatches(FIXTURE, &[(r#"R1<"abc">"#, r#"["a", "b", "c"]"#)]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `probe` reads as the TS2589 recovery of a conditional tail run, a
/// resource partial.
fn assert_ts2589(source: &str, probe: &str) {
    with_recovered_probe(source, probe, |dispatch, node| {
        let data = dispatch.graph().node_data(node);
        assert!(
            matches!(
                data.as_deref(),
                Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
                    diagnostic,
                    origin: None,
                })) if *diagnostic == TS2589
            ),
            "`{probe}` must be the TS2589 recovery, measured {data:?}"
        );
    });
}
