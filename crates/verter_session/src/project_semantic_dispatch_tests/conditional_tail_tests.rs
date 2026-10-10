//! A conditional alias whose selected branch applies the alias again runs
//! as the checker's tail loop (`getConditionalType`): each step is the
//! alias over the next arguments, evaluated in place rather than as a
//! nested instantiation. The checker fails a run with TS2589 at its tail
//! limit of 1,000 steps; Verter runs on to the answer and reports the same
//! TS2589 only at its own, far larger tail budget. An application with
//! arguments of its own (`A8<Eat<I>>`)
//! evaluates them before the step; arguments that come round again can never
//! reach a value, a certified divergence: TS2589 at once, a complete answer.
//! The TS2589 recovery is the checker's error type: it relates as `any`
//! does (assignable to everything but `never`) and, as a conditional's
//! check or extends type, is the conditional's answer. At Verter's tail
//! budget it is a resource partial, never kept.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured with `tsc
//! --declaration --emitDeclarationOnly` on [`FIXTURE`], each probe read off a
//! TS2322 against `never`. The four `strictNullChecks` × `noImplicitAny`
//! settings agree on every probe.

use super::checker_probe_lane_tests::{mismatches, mismatches_in_one_host, with_recovered_probe};
use verter_type_engine::semantic_query::{
    CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation, QueryError,
    RecoveryBasis, SemanticNodeData,
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

/// A tail run goes on past the checker's 1,000th step to its answer, and
/// fails with the checker's TS2589 only at Verter's own tail budget. Three
/// points on one alias: the checker's limit, where Verter answers; just
/// inside Verter's budget, where it still answers; and at the budget's
/// step, the checker's TS2589. A lower budget than production's stands in
/// for it on the same path, so the run stays quick (each step of `Rep`
/// copies the tuple built so far, so a run's cost grows with the square of
/// its length: the checker's 1,000th step is the one run of that length
/// here, and the budget's two points are taken at 200).
///
/// Measured: `Len<Rep<999>>` is `999`; `Rep<1000>` is `any` under TS2589,
/// the checker's limit, where Verter's full answer has 1,000 elements.
#[test]
fn a_tail_run_answers_past_the_checker_step_and_fails_at_verters_budget() {
    const BUDGET: u32 = 200;
    let failures = mismatches(FIXTURE, &[("Len<Rep<1000>>", "1000")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let _budget = super::connected_demand::TailBudgetForTests::install(BUDGET);
    let inside = BUDGET - 1;
    let failures = mismatches(
        FIXTURE,
        &[(&format!("Len<Rep<{inside}>>"), &inside.to_string())],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_ts2589(FIXTURE, &format!("Rep<{BUDGET}>"), RecoveryBasis::Budget);
}

/// A run whose arguments come round again is in a state it already left,
/// and repeats it forever: a certified divergence, the TS2589 recovery at
/// once and a complete answer. A run whose arguments grow repeats nothing;
/// it stops only at its allowance, a resource partial.
///
/// Measured (all four settings agree): `Same<0>`, `Pair<0, 0>` for `type
/// Pair<A, B> = A extends 0 ? Pair<B, A> : 1`, and `Loop<0>` are `any`
/// under TS2589 (`Loop`'s arguments grow, and it reaches the tail limit —
/// Verter's tail budget here, a lower one than production's standing in on
/// the same path); `Same<1>` is `1`.
#[test]
fn a_run_that_cannot_reach_a_value_is_ts2589() {
    assert_ts2589(FIXTURE, "Same<0>", RecoveryBasis::Certified);
    let _budget = super::connected_demand::TailBudgetForTests::install(200);
    assert_ts2589(FIXTURE, "Loop<0>", RecoveryBasis::Budget);
}

/// A certified divergence holds under every allowance, so it is kept: a
/// second read on the same host is the same complete recovery.
#[test]
fn a_certified_divergence_is_complete_cold_and_warm() {
    let host = super::checker_probe_lane_tests::default_probe_host();
    for read in ["cold", "warm"] {
        super::checker_probe_lane_tests::with_probe_outcome_on_host(
            &host,
            Default::default(),
            FIXTURE,
            "Same<0>",
            |dispatch, outcome| match outcome {
                super::evaluate::StructuralFactDemandOutcome::Complete(node) => assert!(
                    matches!(
                        dispatch.graph().node_data(node).as_deref(),
                        Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
                            basis: RecoveryBasis::Certified,
                            ..
                        }))
                    ),
                    "{read}: the certified TS2589 recovery"
                ),
                other => panic!("{read}: a certified divergence is complete, got {other:?}"),
            },
        );
    }
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
    assert_ts2589(
        FIXTURE,
        "E extends string ? 1 : 2",
        RecoveryBasis::Certified,
    );
    assert_ts2589(FIXTURE, "0 extends E ? 1 : 2", RecoveryBasis::Certified);
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

/// `probe` reads as the TS2589 recovery of a conditional tail run: a
/// complete answer when a certified divergence decides it, a resource
/// partial when the run's allowance did.
fn assert_ts2589(source: &str, probe: &str, basis: RecoveryBasis) {
    let check =
        |dispatch: &super::ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
         node| {
            let data = dispatch.graph().node_data(node);
            assert!(
                matches!(
                    data.as_deref(),
                    Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
                        diagnostic,
                        basis: measured,
                        origin: None,
                    })) if *diagnostic == TS2589 && *measured == basis
                ),
                "`{probe}` must be the {basis:?} TS2589 recovery, measured {data:?}"
            );
        };
    match basis {
        RecoveryBasis::Certified => super::checker_probe_lane_tests::with_probe_outcome_on_host(
            &super::checker_probe_lane_tests::default_probe_host(),
            Default::default(),
            source,
            probe,
            |dispatch, outcome| match outcome {
                super::evaluate::StructuralFactDemandOutcome::Complete(node) => {
                    check(dispatch, node);
                }
                other => panic!("`{probe}` is a certified divergence, complete; got {other:?}"),
            },
        ),
        RecoveryBasis::Budget => with_recovered_probe(source, probe, check),
    }
}

/// The lib `Awaited<T>` conditional's tail run goes on past the checker's
/// 1,000th step to its answer (the one run of that length here: each step
/// of `P` copies the counter tuple built so far, so a run's cost grows with
/// the square of its length, and the runs under the checker's limit are
/// [`an_awaited_tail_run_fails_at_verters_budget`]'s shorter ones).
///
/// TypeScript 7.0.2, all four `strictNullChecks` × `noImplicitAny`
/// settings, over [`THENABLES`]: `Awaited<P<999>>` is `"done"` and
/// `Awaited<P<1000>>` is `any` under TS2589, the checker's limit, where
/// Verter's full answer is `"done"`.
#[test]
fn an_awaited_tail_run_answers_past_the_checker_step() {
    let failures = mismatches(THENABLES, &[("Awaited<P<1000>>", r#""done""#)]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The lib `Awaited<T>` conditional's tail run takes Verter's tail budget:
/// just inside it the run answers, and at the budget's step it is the
/// checker's TS2589. A far lower budget than production's stands in for it
/// on the same path, so the runs stay quick.
///
/// TypeScript 7.0.2, over [`THENABLES`]: `Awaited<P<K>>` is `"done"` for
/// every `K` below the checker's limit of 1,000.
#[test]
fn an_awaited_tail_run_fails_at_verters_budget() {
    const BUDGET: u32 = 200;
    let _budget = super::connected_demand::TailBudgetForTests::install(BUDGET);
    let inside = BUDGET - 1;
    let failures = mismatches(
        THENABLES,
        &[(&format!("Awaited<P<{inside}>>"), r#""done""#)],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    with_recovered_probe(
        THENABLES,
        &format!("Awaited<P<{BUDGET}>>"),
        |dispatch, node| {
            let data = dispatch.graph().node_data(node);
            assert!(
                matches!(
                    data.as_deref(),
                    Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
                        diagnostic,
                        basis: RecoveryBasis::Budget,
                        ..
                    }))
                        if diagnostic.code == CheckerDiagnosticCode::ExcessivelyDeepInstantiation
                            && diagnostic.operation == CheckerDiagnosticOperation::LibAwaited
                ),
                "`Awaited<P<{BUDGET}>>` must be the TS2589 recovery, measured {data:?}"
            );
        },
    );
}

/// A thenable whose `then` hands on the next one, `K` of them before
/// `"done"`: `Awaited` over it runs one tail step per thenable.
const THENABLES: &str = r#"
type P<K extends number, N extends unknown[] = []> = N["length"] extends K ? "done" : { then(f: (v: P<K, [...N, 0]>) => any): any };
"#;

/// Whether a tail run reaches Verter's tail budget depends on the budget,
/// so its TS2589 is never kept: on the same host, a read under the
/// production budget runs the tail again and answers.
///
/// Measured: `Len<Rep<999>>` is `999`; `Rep<1000>` is `any` under TS2589,
/// the checker's limit, where Verter's full answer has 1,000 elements.
#[test]
fn a_tail_budget_recovery_is_never_kept_in_the_memo() {
    let host = super::checker_probe_lane_tests::default_probe_host();
    let is_ts2589 = |probe: &str| {
        super::checker_probe_lane_tests::with_probe_on_host(
            &host,
            Default::default(),
            FIXTURE,
            probe,
            |dispatch, node| {
                matches!(
                    dispatch.graph().node_data(node).as_deref(),
                    Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { diagnostic, .. }))
                        if *diagnostic == TS2589
                )
            },
        )
    };
    {
        let _budget = super::connected_demand::TailBudgetForTests::install(200);
        assert!(
            is_ts2589("Rep<200>"),
            "Rep<200> reaches the tail budget of 200"
        );
    }
    assert!(
        !is_ts2589("Rep<200>"),
        "under the production budget Rep<200> answers, not the kept recovery"
    );
}
