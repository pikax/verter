//! Three answers stop a recursive type operation, and each is told apart:
//!
//! - a DECLARATION ERROR — the checker's TS2456 for an alias whose declared
//!   type requires itself — decided by the declarations alone;
//! - a CERTIFIED DIVERGENCE — TS2589 at once, a complete answer kept like
//!   any semantic answer — minted only from a sound proof, through steps
//!   that are each a deterministic, complete function of their state, that
//!   the operation either re-enters an exact state (same declaration,
//!   arguments, context and phase) or grows one argument by the same
//!   structural wrapper at every step while the checker's own generic test
//!   selects the recursive branch for whatever the wrapper holds;
//! - a BUDGET REFUSAL — TS2589 at Verter's own allowance, a resource
//!   partial that is never kept.
//!
//! A same-path re-entry, repetition of a name, growth, cancellation or a step
//! that read a partial (a refused child, a missing input) proves nothing on
//! its own: without the proof the run falls back to work-budgeted
//! evaluation, and a same-path partial is never retained as a refusal
//! either (`classify_final_component_retention`). A certificate's
//! dependencies are its reads, so editing a helper the loop runs through
//! retires it.
//!
//! Every checker answer below is TypeScript 7.0.2's (`tsc --noEmit
//! --strict`): `Same<0>`, `Pair<0, 0>`, `Outer<0>`, `C<0>` over `H<T> = T`,
//! `G<string>`, `O<{}>`, `W<number>` and `GL<[]>` report TS2589; `Cyc`
//! reports TS2456; `Alt<0>` is `"end"`, `G<never>` is `never`, `K<0>` is
//! `1`, `C<0>` over `H<T> = 1` is `1`, `Rep<3>["length"]` is `3`,
//! `Stop<string>` is `"deep"` and `D<string>["a"]["a"]` is `D<[[string]]>`,
//! each with no diagnostic.

use super::checker_probe_lane_tests::{
    default_probe_host, mismatches_in_one_host, upsert_probe_files, with_probe_outcome_on_host,
    ProbeProject,
};
use super::evaluate::StructuralFactDemandOutcome;
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
use verter_type_engine::project_semantic_dispatch::connected_demand::TailBudgetForTests;
use verter_type_engine::semantic_query::{
    CheckerDiagnosticCode, PartialReasonSet, QueryError, RecoveryBasis, SemanticNodeData,
};

const FIXTURE: &str = r#"
type Same<T> = T extends 0 ? Same<T> : 1;
type Pair<A, B> = A extends 0 ? Pair<B, A> : 1;
type Outer<T> = Inner<T>;
type Inner<T> = T extends 0 ? Inner<T> : 1;
type G<T> = T extends never ? never : G<[T]>;
type O<T> = T extends object ? O<[T]> : 1;
type W<T> = T extends string ? "s" : W<{ v: T }>;
type GL<T extends unknown[]> = T["length"] extends -1 ? 1 : GL<[...T, 0]>;
type Stop<T> = T extends [[[unknown]]] ? "deep" : Stop<[T]>;
type Alt<T> = T extends 0 ? Alt<1> : T extends 1 ? Alt<2> : "end";
type D<T> = { a: D<[T]> };
type Rep<N extends number, Acc extends unknown[] = []> = Acc["length"] extends N ? Acc : Rep<N, [...Acc, 0]>;
type Cyc<T> = Cyc2<[T]>;
type Cyc2<T> = T extends any ? Cyc<T> : never;
type K<T> = [T] extends [0] ? K<[0]> : 1;
type Spin<T> = [T] extends [0] ? Spin<Rep<20>> : 1;
"#;

/// How a probe's read ended.
#[derive(Debug, PartialEq, Eq)]
enum Ending {
    /// A complete answer that is not a checker recovery.
    Value,
    /// The checker recovery for `code` on `basis`, complete or partial as
    /// its basis requires.
    Recovery {
        code: CheckerDiagnosticCode,
        basis: RecoveryBasis,
        partial: Option<PartialReasonSet>,
    },
    /// A partial that is not a checker recovery, with its reasons.
    Partial(PartialReasonSet),
}

/// Read `probe` over `source` on `host`.
fn ending_on(host: &std::sync::Arc<crate::VerterHost>, source: &str, probe: &str) -> Ending {
    with_probe_outcome_on_host(
        host,
        ProbeProject::default(),
        source,
        probe,
        |dispatch, outcome| {
            let (node, partial) = match outcome {
                StructuralFactDemandOutcome::Complete(node) => (node, None),
                StructuralFactDemandOutcome::Recovered { node, reasons } => (node, Some(reasons)),
                StructuralFactDemandOutcome::Partial(reasons) => return Ending::Partial(reasons),
            };
            match dispatch.graph().node_data(node).as_deref() {
                Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
                    diagnostic,
                    basis,
                    ..
                })) => Ending::Recovery {
                    code: diagnostic.code,
                    basis: *basis,
                    partial,
                },
                _ => partial.map_or(Ending::Value, Ending::Partial),
            }
        },
    )
}

fn ending(probe: &str) -> Ending {
    ending_on(&default_probe_host(), FIXTURE, probe)
}

const CERTIFIED_TS2589: Ending = Ending::Recovery {
    code: CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
    basis: RecoveryBasis::Certified,
    partial: None,
};

const BUDGET_TS2589: Ending = Ending::Recovery {
    code: CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
    basis: RecoveryBasis::Budget,
    partial: Some(PartialReasonSet::OPERATION_BUDGET),
};

/// An exact loop — the run re-entering the arguments it took, directly, by
/// swapping them, or under an eager wrapper alias — is a certified
/// divergence: TS2589 at once, complete.
#[test]
fn exact_loops_and_eager_wrappers_are_certified_divergences() {
    for probe in ["Same<0>", "Pair<0, 0>", "Outer<0>"] {
        assert_eq!(ending(probe), CERTIFIED_TS2589, "`{probe}`");
    }
}

/// A run that wraps its argument in the same tuple or object type at every
/// step while the conditional is decided for whatever the wrapper holds —
/// definitely false over the wildcard (`G`, `W`), definitely true over the
/// bare parameter (`O`) — can never take its other branch: a certified
/// divergence, the same complete TS2589 under any tail allowance.
#[test]
fn uniform_growth_is_certified_under_any_allowance() {
    for allowance in [Some(50), Some(5), None] {
        let _budget = allowance.map(TailBudgetForTests::install);
        for probe in ["G<string>", "O<{}>", "W<number>"] {
            assert_eq!(
                ending(probe),
                CERTIFIED_TS2589,
                "`{probe}` under the tail allowance {allowance:?}"
            );
        }
    }
}

/// Growth the checker's generic test does not decide proves nothing:
/// `GL<[]>` checks `T["length"]`, not the growing parameter itself, and
/// spreads it, so it stops only at the tail budget, a resource partial.
#[test]
fn growth_without_a_uniform_decision_is_a_budget_refusal() {
    let _budget = TailBudgetForTests::install(50);
    assert_eq!(ending("GL<[]>"), BUDGET_TS2589);
}

/// A run whose step reads a budget-refused child is not an exact state:
/// `Spin<0>` re-enters `Spin<R>` with `R` the TS2589 recovery of
/// `Rep<20>` under a tail budget of 20, but each `R` is the budget's answer,
/// not the types'. The run falls back to its own budget.
#[test]
fn a_step_through_a_budget_refusal_is_never_certified() {
    let _budget = TailBudgetForTests::install(20);
    assert_eq!(ending("Spin<0>"), BUDGET_TS2589);
}

/// A declaration error is the checker's TS2456, complete, and no
/// divergence.
#[test]
fn a_circular_declaration_is_a_declaration_error_not_a_divergence() {
    assert_eq!(
        ending("Cyc<string>"),
        Ending::Recovery {
            code: CheckerDiagnosticCode::CircularTypeAlias,
            basis: RecoveryBasis::Certified,
            partial: None,
        }
    );
}

/// Changing arguments, a distribution over `never`, branches that move on,
/// and a recursive structure the checker defers — which re-enters its own
/// declaration on the path, proving nothing — all reach their values.
#[test]
fn controls_reach_their_values_without_a_certificate() {
    let rows = [
        (r#"Rep<3>["length"]"#, "3"),
        ("G<never>", "never"),
        ("Alt<0>", r#""end""#),
        ("K<0>", "1"),
        ("Stop<string>", r#""deep""#),
        (r#"D<string>["a"]["a"]"#, "D<[[string]]>"),
    ];
    let failures = mismatches_in_one_host(FIXTURE, &rows);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Removes the producer hook a test installed.
struct UnhookOnDrop<'s>(&'s verter_type_engine::semantic_query_memo::SemanticGraphStore);

impl Drop for UnhookOnDrop<'_> {
    fn drop(&mut self) {
        self.0.set_produce_hook_for_tests(None);
    }
}

/// A certificate is kept, and depends on the declarations its loop runs
/// through: a second read builds no instantiation of `C`, and once a helper
/// in another module changes so the arguments stop coming round, the same
/// host builds `C` again and answers the new value instead of the kept
/// recovery.
#[test]
fn a_helper_edit_retires_a_certificate() {
    use std::sync::atomic::{AtomicUsize, Ordering};
    let host = default_probe_host();
    let store = host.project_type_store();
    let graph = store.semantic_graph();
    let builds = std::sync::Arc::new(AtomicUsize::new(0));
    let counted = std::sync::Arc::clone(&builds);
    graph.set_produce_hook_for_tests(Some(std::sync::Arc::new(move |key, _| {
        if let verter_type_engine::semantic_query::SemanticQueryKey::Instantiate(instantiate) = key
        {
            if instantiate.base().merged_symbol_name.as_ref() == "C" {
                counted.fetch_add(1, Ordering::SeqCst);
            }
        }
    })));
    let _unhook = UnhookOnDrop(graph);
    let builds_of_c = || builds.swap(0, Ordering::SeqCst);

    let source = "import type { H } from \"./helper\";\n\
                  type C<T> = T extends 0 ? C<H<T>> : 1;\n";
    upsert_probe_files(&host, &[("helper.ts", "export type H<T> = T;\n")]);
    assert_eq!(
        ending_on(&host, source, "C<0>"),
        CERTIFIED_TS2589,
        "the loop through `H<T> = T`"
    );
    assert!(builds_of_c() > 0, "the first read builds `C<0>`");
    assert_eq!(
        ending_on(&host, source, "C<0>"),
        CERTIFIED_TS2589,
        "warm, the kept certificate"
    );
    assert_eq!(
        builds_of_c(),
        0,
        "the kept certificate answers without a build"
    );

    upsert_probe_files(&host, &[("helper.ts", "export type H<T> = 1;\n")]);
    let expected = checker_syntax::parse("1").expect("the checker print parses");
    let answered = with_probe_outcome_on_host(
        &host,
        ProbeProject::default(),
        source,
        "C<0>",
        |dispatch, outcome| {
            let StructuralFactDemandOutcome::Complete(node) = outcome else {
                panic!("`C<0>` over `H<T> = 1` answers, got {outcome:?}");
            };
            checker_syntax::matches_node(dispatch, node, &expected, 0)
                .then_some(())
                .ok_or_else(|| render_node(dispatch, node, 0))
        },
    );
    assert_eq!(answered, Ok(()), "`C<0>` over the edited helper is `1`");
    assert!(builds_of_c() > 0, "the edit retires the kept certificate");
}
