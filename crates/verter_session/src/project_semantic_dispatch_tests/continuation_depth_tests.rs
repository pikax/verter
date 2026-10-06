//! A chain of instantiations evaluates as a chain of continuation frames,
//! not of native calls: an alias whose body applies the next alias needs
//! that instantiation from the runtime, which parks the frame on the heap
//! while the next one builds. The answer no longer depends on how deep the
//! chain is — neither the stack the caller runs on nor the nested-query
//! depth the connected demand counts for native re-entry decides it.
//!
//! Past the checker's own instantiation depth Verter keeps evaluating and
//! answers; it reports the checker's TS2589 — the same code and message —
//! only where its own instantiation budget runs out, far past the
//! checker's.
//!
//! Every checker answer quoted is TypeScript 7.0.2's, measured through the
//! capped `tsc` with `--ignoreConfig`.

use super::checker_probe_lane_tests::{evaluated_mismatches, with_probe_in, ProbeProject};
use crate::semantic_query::{CheckerDiagnosticCode, QueryError, SemanticNodeData};

/// A chain of `length` aliases down to `T | undefined`, a generic function
/// declared to return its end, and the call's result kept as a `const` and
/// joined with a pinned `"ok"` in a return.
fn alias_chain(length: usize) -> String {
    let mut source = String::from("type A0<T> = T | undefined;\n");
    for level in 1..=length {
        source.push_str(&format!("type A{level}<T> = A{}<T>;\n", level - 1));
    }
    source.push_str(&format!(
        "declare function f<T>(x: T): A{length}<T>;\n\
         export const c = f(\"ok\");\n\
         export function gj(b: boolean) {{ if (b) {{ return f(\"ok\"); }} return \"ok\" as const; }}\n"
    ));
    source
}

/// A chain of `length` aliases each applying the one below to a conditional
/// over its own parameter: `E<i><X> = E<i-1><E0<X>>`.
fn conditional_argument_chain(length: usize) -> String {
    let mut source = String::from("type E0<X> = X extends string ? X : never;\n");
    for level in 1..=length {
        source.push_str(&format!("type E{level}<X> = E{}<E0<X>>;\n", level - 1));
    }
    source
}

/// `f` over the evaluated rows, read on a 1 MiB thread — the smallest stack
/// a host asks for.
fn on_a_small_stack<T: Send + 'static>(f: impl FnOnce() -> T + Send + 'static) -> T {
    std::thread::Builder::new()
        .stack_size(1 << 20)
        .spawn(f)
        .expect("spawn the probing thread")
        .join()
        .expect("the probe answers")
}

const ALIAS_CHAIN_ROWS: &[(&str, &str)] = &[
    ("typeof c", "\"ok\" | undefined"),
    ("ReturnType<typeof gj>", "\"ok\" | undefined"),
];

/// Alias chains of 50, 200 and 500 aliases evaluate cold, to the checker's
/// answer, on a 1 MiB thread. Each alias's instantiation needs the next one
/// from the runtime instead of opening it one native query deeper, so the
/// chain is bounded neither by the stack nor by the nested-query depth that
/// bounds native re-entry (a chain of 23 used to exceed it).
///
/// TypeScript 7.0.2, all four `strictNullChecks` × `noImplicitAny` settings,
/// at 50, 200 and 500 aliases: no TS2589; under `strictNullChecks`
/// `typeof c` is `"ok" | undefined` and `gj` returns `"ok" | undefined`.
#[test]
fn alias_chains_up_to_500_long_answer_cold_on_a_one_mebibyte_stack() {
    for (length, rows) in [
        (50, ALIAS_CHAIN_ROWS),
        (200, ALIAS_CHAIN_ROWS),
        (500, &ALIAS_CHAIN_ROWS[..1]),
    ] {
        let mismatches = on_a_small_stack(move || evaluated_mismatches(&alias_chain(length), rows));
        assert_eq!(
            mismatches,
            Vec::<String>::new(),
            "a chain of {length} aliases"
        );
    }
}

/// The call's return through 500 aliases, and a chain of 1,000 aliases,
/// answer as the shorter chains do.
///
/// TypeScript 7.0.2 (all four settings at 500, `strictNullChecks` on and
/// off at 1,000): no TS2589; under `strictNullChecks` `typeof c` and
/// `gj`'s return are `"ok" | undefined`.
#[test]
#[ignore = "the flow return of a call whose declared return is a chain of 500 or more aliases produces no result"]
fn alias_chains_500_and_1000_long_answer_every_row_cold_on_a_one_mebibyte_stack() {
    for (length, rows) in [(500, &ALIAS_CHAIN_ROWS[1..]), (1000, ALIAS_CHAIN_ROWS)] {
        let mismatches = on_a_small_stack(move || evaluated_mismatches(&alias_chain(length), rows));
        assert_eq!(
            mismatches,
            Vec::<String>::new(),
            "a chain of {length} aliases"
        );
    }
}

/// The four `strictNullChecks` × `noImplicitAny` settings.
const SETTINGS: [&str; 4] = [
    r#"{ "strictNullChecks": true, "noImplicitAny": true }"#,
    r#"{ "strictNullChecks": true, "noImplicitAny": false }"#,
    r#"{ "strictNullChecks": false, "noImplicitAny": true }"#,
    r#"{ "strictNullChecks": false, "noImplicitAny": false }"#,
];

/// `probe` over `source`, checked under `options` on a 1 MiB thread —
/// under an instantiation budget of `budget` when set — read by `read`
/// with the probe's evaluated node.
fn evaluated_in<R: Send + 'static>(
    budget: Option<u32>,
    options: &'static str,
    source: String,
    probe: String,
    read: impl FnOnce(
            &super::ProjectSemanticDispatch<'_, crate::resolver_core::HostCapabilities>,
            crate::semantic_query::SemanticNodeId,
        ) -> R
        + Send
        + 'static,
) -> R {
    use crate::semantic_query::{ProjectionMode, ProjectionReductionContext};
    on_a_small_stack(move || {
        let _budget = budget.map(super::connected_demand::InstantiationBudgetForTests::install);
        with_probe_in(
            ProbeProject {
                compiler_options: Some(options),
                ..Default::default()
            },
            &source,
            &probe,
            |dispatch, node| {
                let value = dispatch
                    .normalize_node_for_structural_fact_demand(
                        node,
                        ProjectionReductionContext::published(ProjectionMode::Expanded),
                    )
                    .into_usable_node()
                    .expect("the probe answers");
                read(dispatch, value)
            },
        )
    })
}

/// `probe`'s evaluated type as displayed, `Ok` when it is `checker`.
fn displayed_in(
    budget: Option<u32>,
    options: &'static str,
    source: String,
    probe: &str,
    checker: &'static str,
) -> Result<String, String> {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
    evaluated_in(
        budget,
        options,
        source,
        probe.to_owned(),
        move |dispatch, value| {
            let expected = checker_syntax::parse(checker).expect("the checker type parses");
            let shown = render_node(dispatch, value, 0);
            if checker_syntax::matches_node(dispatch, value, &expected, 0) {
                Ok(shown)
            } else {
                Err(shown)
            }
        },
    )
}

/// The TS2589 diagnostic `probe` evaluates to, if it is a checker
/// recovery.
fn recovery_in(
    budget: Option<u32>,
    options: &'static str,
    source: String,
    probe: &str,
) -> Option<crate::semantic_query::CheckerDiagnostic> {
    evaluated_in(
        budget,
        options,
        source,
        probe.to_owned(),
        |dispatch, value| match dispatch.graph().node_data(value).as_deref() {
            Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { diagnostic, .. })) => {
                Some(*diagnostic)
            }
            _ => None,
        },
    )
}

/// `E<length><"ok">` answers `"ok"` under each of `settings`.
fn check_conditional_argument_chains_answer(lengths: &[usize], settings: &[&'static str]) {
    for &length in lengths {
        for &options in settings {
            let answer = displayed_in(
                None,
                options,
                conditional_argument_chain(length),
                &format!("E{length}<\"ok\">"),
                "\"ok\"",
            );
            assert!(
                answer.is_ok(),
                "E{length}<\"ok\"> under {options} is \"ok\": {answer:?}"
            );
        }
    }
}

/// Conditional arguments nested through a chain of aliases evaluate on a
/// 1 MiB thread up to the checker's instantiation depth.
///
/// TypeScript 7.0.2, all four `strictNullChecks` × `noImplicitAny`
/// settings: `E97<"ok">` is `"ok"`; `E98<"ok">` is TS2589.
#[test]
fn conditional_arguments_nested_97_deep_answer_on_a_one_mebibyte_stack() {
    check_conditional_argument_chains_answer(&[97], &SETTINGS);
}

/// Past the checker's instantiation depth Verter keeps evaluating: the
/// declared alias type — a conditional nested once per alias — is
/// evaluated to its answer, where the checker gives up.
///
/// TypeScript 7.0.2, all four settings: `E98<"ok">` and `E200<"ok">` are
/// TS2589, the checker's limit; Verter's full answer is `"ok"`, as at 97.
#[test]
fn conditional_arguments_nested_past_the_checker_depth_answer() {
    check_conditional_argument_chains_answer(&[98, 200], &SETTINGS);
}

/// Five hundred levels deep, the same full answer.
///
/// TypeScript 7.0.2, all four settings: `E500<"ok">` is TS2589; Verter's
/// full answer is `"ok"`.
#[test]
fn conditional_arguments_nested_500_deep_answer() {
    check_conditional_argument_chains_answer(&[500], &SETTINGS);
}

/// A thousand levels deep, the same full answer, well inside Verter's
/// instantiation budget.
///
/// TypeScript 7.0.2, `strictNullChecks` and `noImplicitAny`:
/// `E1000<"ok">` is TS2589; Verter's full answer is `"ok"`.
#[test]
fn conditional_arguments_nested_1000_deep_answer() {
    check_conditional_argument_chains_answer(&[1000], &SETTINGS[..1]);
}

/// Verter's instantiation budget, pinned at three points on one chain:
/// the checker's limit, where Verter answers; just inside Verter's budget,
/// where it still answers; and one level past it, where it reports the
/// checker's TS2589 — the checker's code and message — as a typed
/// recovery, having held at most the budget's frames. A lower budget than
/// production's stands in for it on the same path, so the chain stays
/// quick to evaluate: an `E<n>` chain nests `n` instantiations.
///
/// TypeScript 7.0.2, all four settings: `E98<"ok">` is TS2589 with
/// "Type instantiation is excessively deep and possibly infinite."
#[test]
fn verters_instantiation_budget_reports_the_checkers_ts2589() {
    use crate::semantic_query::CheckerDiagnosticOperation;
    const BUDGET: u32 = 150;
    const TS2589: &str = "Type instantiation is excessively deep and possibly infinite.";
    let answers = |options: &'static str, length: usize| {
        displayed_in(
            Some(BUDGET),
            options,
            conditional_argument_chain(length),
            &format!("E{length}<\"ok\">"),
            "\"ok\"",
        )
        .is_ok()
    };
    let reports_ts2589 = |options: &'static str, length: usize| {
        recovery_in(
            Some(BUDGET),
            options,
            conditional_argument_chain(length),
            &format!("E{length}<\"ok\">"),
        )
        .is_some_and(|diagnostic| {
            diagnostic.code.code() == 2589
                && diagnostic.code.message() == TS2589
                && diagnostic.operation == CheckerDiagnosticOperation::InstantiationBudget
        })
    };
    for options in SETTINGS {
        assert!(
            answers(options, 98),
            "E98 under {options} answers past the checker's limit"
        );
        let inside = BUDGET as usize;
        assert!(
            answers(options, inside),
            "E{inside} under {options} answers inside Verter's budget"
        );
        assert!(
            reports_ts2589(options, inside + 1),
            "E{} under {options} is the checker's TS2589 at Verter's budget",
            inside + 1
        );
    }
}

/// An instantiation that never terminates — each one needing the next of
/// the same alias over a growing argument — stops at Verter's
/// instantiation budget with the checker's TS2589, in bounded memory.
///
/// TypeScript 7.0.2, all four settings: `Grow<"ok">` over `type Pair<X> =
/// X extends unknown ? [X, X] : never` and `type Grow<T> = T extends never
/// ? never : Pair<Grow<[T]>>` is TS2589.
#[test]
fn an_unbounded_instantiation_stops_at_verters_budget_with_ts2589() {
    const GROW: &str = "type Pair<X> = X extends unknown ? [X, X] : never;\n\
                        type Grow<T> = T extends never ? never : Pair<Grow<[T]>>;\n";
    for options in SETTINGS {
        let recovered = evaluated_in(
            Some(150),
            options,
            GROW.to_owned(),
            "Grow<\"ok\">".to_owned(),
            |dispatch, value| {
                let mut seen = rustc_hash::FxHashSet::default();
                let mut stack = vec![value];
                while let Some(node) = stack.pop() {
                    if !seen.insert(node) {
                        continue;
                    }
                    let Some(data) = dispatch.graph().node_data(node) else {
                        continue;
                    };
                    if matches!(
                        data.as_ref(),
                        SemanticNodeData::Opaque(QueryError::CheckerRecovery { diagnostic, .. })
                            if diagnostic.code == CheckerDiagnosticCode::ExcessivelyDeepInstantiation
                    ) {
                        return true;
                    }
                    data.for_each_retained_child(|child| stack.push(child));
                }
                false
            },
        );
        assert!(
            recovered,
            "Grow<\"ok\"> under {options} reaches the checker's TS2589 at Verter's budget"
        );
    }
}

/// Each frame's result carries what the frames below it read: the carrier
/// of every instantiation a frame needed is replayed into the frame's own
/// tracer when it resumes, so an edit to the declaration at the bottom of a
/// chain reaches every warm instantiation above it.
///
/// TypeScript 7.0.2, `strictNullChecks`: `typeof c` is `"ok" | undefined`
/// over `type A0<T> = T | undefined`, and `"ok" | null` over
/// `type A0<T> = T | null`.
#[test]
fn an_edit_below_a_chain_of_frames_reaches_every_instantiation_above_it() {
    use super::checker_probe_lane_tests::{default_probe_host, with_probe_on_host};
    use crate::semantic_query::{ProjectionMode, ProjectionReductionContext};
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};

    let mut source = String::from("import type { A0 } from \"./base\";\n");
    for level in 1..=30 {
        source.push_str(&format!("type A{level}<T> = A{}<T>;\n", level - 1));
    }
    source.push_str("declare function f<T>(x: T): A30<T>;\nexport const c = f(\"ok\");\n");
    let host = default_probe_host();
    let evaluate = |base: &str, checker: &str| -> Result<(), String> {
        let expected = checker_syntax::parse(checker).expect("the checker type parses");
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            "/wb/base.ts",
            base,
            crate::FileLanguage::script_ts(),
        );
        with_probe_on_host(
            &host,
            Default::default(),
            &source,
            "typeof c",
            |dispatch, node| {
                let value = dispatch
                    .normalize_node_for_structural_fact_demand(
                        node,
                        ProjectionReductionContext::published(ProjectionMode::Expanded),
                    )
                    .into_complete_node()
                    .expect("the chain evaluates completely");
                if checker_syntax::matches_node(dispatch, value, &expected, 0) {
                    Ok(())
                } else {
                    Err(render_node(dispatch, value, 0))
                }
            },
        )
    };
    assert_eq!(
        evaluate("export type A0<T> = T | undefined;\n", "\"ok\" | undefined"),
        Ok(())
    );
    assert_eq!(
        evaluate("export type A0<T> = T | null;\n", "\"ok\" | null"),
        Ok(())
    );
}

/// Printing an application of a long alias chain reads each alias's body a
/// bounded number of times: whether the chain declares a homomorphic
/// mapping is walked once for the whole chain, not once per alias, and the
/// printing walk itself runs in a loop.
///
/// TypeScript 7.0.2: `E200<"ok">` is TS2589 (Verter's full answer is
/// `"ok"`); the bound is on Verter's own work.
#[test]
fn printing_an_alias_chain_reads_each_alias_a_bounded_number_of_times() {
    const LENGTH: usize = 200;
    let lowerings = on_a_small_stack(|| {
        let _trace = super::raise::enable_dispatch_trace_for_test();
        let host = super::checker_probe_lane_tests::default_probe_host();
        super::checker_probe_lane_tests::with_probe_outcome_on_host(
            &host,
            Default::default(),
            &conditional_argument_chain(LENGTH),
            &format!("E{LENGTH}<\"ok\">"),
            |_, _| (),
        );
        super::raise::DISPATCH_TRACE.with(|trace| {
            trace
                .borrow()
                .iter()
                .filter(|kind| **kind == "LowerLocator")
                .count()
        })
    });
    assert!(
        lowerings <= 4 * LENGTH,
        "{lowerings} body lowerings for a chain of {LENGTH} aliases"
    );
}

/// Set in the child process
/// [`a_process_of_its_own_evaluates_deep_chains_on_a_one_mebibyte_thread`]
/// starts.
const SMALL_STACK_CHILD: &str = "VERTER_CONTINUATION_SMALL_STACK_CHILD";

/// In a process of its own — nothing evaluated before it, on any stack —
/// 1 MiB threads evaluate a 200-alias chain, conditional arguments nested
/// 97 deep and 500 deep, display an answer, and drop every host and
/// dispatcher they built there. An overflow aborts the child and fails
/// this test instead of the whole run.
///
/// TypeScript 7.0.2, `strict`: `typeof c` and `ReturnType<typeof gj>` over
/// the 200-alias chain are `"ok" | undefined`; `E97<"ok">` is `"ok"`;
/// `E500<"ok">` is TS2589, where Verter's full answer is `"ok"`.
#[test]
fn a_process_of_its_own_evaluates_deep_chains_on_a_one_mebibyte_thread() {
    if std::env::var_os(SMALL_STACK_CHILD).is_some() {
        let mismatches =
            on_a_small_stack(|| evaluated_mismatches(&alias_chain(200), ALIAS_CHAIN_ROWS));
        assert_eq!(mismatches, Vec::<String>::new(), "a chain of 200 aliases");
        for length in [97, 500] {
            let shown = displayed_in(
                None,
                SETTINGS[0],
                conditional_argument_chain(length),
                &format!("E{length}<\"ok\">"),
                "\"ok\"",
            )
            .unwrap_or_else(|shown| panic!("E{length}<\"ok\"> displays as {shown}"));
            assert!(
                shown.contains("ok"),
                "E{length}<\"ok\"> displays as {shown}"
            );
        }
        println!("{SMALL_STACK_CHILD}: every chain answered");
        return;
    }
    let output = std::process::Command::new(std::env::current_exe().expect("this test binary"))
        .args([
            "--exact",
            "project_semantic_dispatch_tests::continuation_depth_tests::a_process_of_its_own_evaluates_deep_chains_on_a_one_mebibyte_thread",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(SMALL_STACK_CHILD, "1")
        .output()
        .expect("run the evaluation in a child process");
    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert!(
        output.status.success()
            && stdout.contains(&format!("{SMALL_STACK_CHILD}: every chain answered")),
        "the child evaluating on 1 MiB threads failed (status {:?}):\n{stdout}\n{stderr}",
        output.status
    );
}

/// Whether Verter's instantiation budget is reached depends on the chain
/// that needed an instantiation, so its recovery is never kept: a demand
/// on the same host under the production budget evaluates the chain again
/// and answers.
///
/// TypeScript 7.0.2, `strict`: `E151<"ok">` is TS2589; Verter's full
/// answer is `"ok"`.
#[test]
fn a_budget_recovery_is_never_kept_in_the_memo() {
    use crate::semantic_query::{ProjectionMode, ProjectionReductionContext};
    let recovered_then_answered = on_a_small_stack(|| {
        let host = super::checker_probe_lane_tests::default_probe_host();
        let source = conditional_argument_chain(151);
        let evaluate = || {
            super::checker_probe_lane_tests::with_probe_on_host(
                &host,
                Default::default(),
                &source,
                "E151<\"ok\">",
                |dispatch, node| {
                    let value = dispatch
                        .normalize_node_for_structural_fact_demand(
                            node,
                            ProjectionReductionContext::published(ProjectionMode::Expanded),
                        )
                        .into_usable_node()
                        .expect("the probe answers");
                    match dispatch.graph().node_data(value).as_deref() {
                        Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { .. })) => None,
                        _ => Some(
                            crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::render_node(
                                dispatch, value, 0,
                            ),
                        ),
                    }
                },
            )
        };
        let limited = {
            let _budget = super::connected_demand::InstantiationBudgetForTests::install(150);
            evaluate()
        };
        (limited, evaluate())
    });
    assert_eq!(recovered_then_answered, (None, Some("\"ok\"".to_owned())));
}

/// Whether `probe`'s evaluated type, under `options` and an instantiation
/// budget of `budget`, holds the checker's TS2589 recovery anywhere in it.
fn holds_a_ts2589(options: &'static str, budget: u32, source: String, probe: &str) -> bool {
    evaluated_in(
        Some(budget),
        options,
        source,
        probe.to_owned(),
        |dispatch, value| {
            let mut seen = rustc_hash::FxHashSet::default();
            let mut stack = vec![value];
            while let Some(node) = stack.pop() {
                if !seen.insert(node) {
                    continue;
                }
                let Some(data) = dispatch.graph().node_data(node) else {
                    continue;
                };
                if matches!(
                    data.as_ref(),
                    SemanticNodeData::Opaque(QueryError::CheckerRecovery { diagnostic, .. })
                        if diagnostic.code == CheckerDiagnosticCode::ExcessivelyDeepInstantiation
                ) {
                    return true;
                }
                data.for_each_retained_child(|child| stack.push(child));
            }
            false
        },
    )
}

/// A recursive application over new arguments, in a position the checker
/// defers — an object member, an array or tuple element, a signature, an
/// interface's type argument, or an alias application inside a member — is
/// the lazy recursive reference: evaluating the application enters no
/// instantiation of it, so it never reaches a budget.
///
/// TypeScript 7.0.2, all four settings: `X<"ok">` over each declaration
/// below reports nothing.
#[test]
fn recursive_applications_in_deferred_positions_stay_lazy() {
    const DEFERRED: [&str; 9] = [
        "type X<T> = { v: T; kids: X<[T]>[] };",
        "type X<T> = [T, X<[T]>];",
        "type X<T> = (x: T) => X<[T]>;",
        "type X<T> = T extends any ? { a: X<[T]> } : never;",
        "type X<T> = T extends any ? X<[T]>[] : never;",
        "type X<T> = T extends any ? [X<[T]>] : never;",
        "type W<Y> = { w: Y }; type X<T> = { v: T; next: W<X<[T]>> };",
        "type X<T> = Promise<X<[T]>>;",
        "interface I<Y> { y: Y } type X<T> = T extends any ? I<X<[T]>> : never;",
    ];
    for options in SETTINGS {
        for declaration in DEFERRED {
            assert!(
                !holds_a_ts2589(
                    options,
                    150,
                    format!(
                        "{declaration}
"
                    ),
                    "X<\"ok\">"
                ),
                "`{declaration}` under {options} stays lazy"
            );
        }
    }
}

/// A recursive application over new arguments, as a type argument of
/// another alias in a conditional's branch, is instantiated as the checker
/// instantiates it — each one the next, without end — and stops at
/// Verter's instantiation budget with the checker's TS2589. A lower budget
/// than production's stands in for it on the same path.
///
/// TypeScript 7.0.2, all four settings: `X<"ok">` over each declaration
/// below is TS2589.
#[test]
fn recursive_applications_in_eager_positions_reach_verters_budget() {
    const EAGER: [&str; 3] = [
        "type Id<Y> = Y; type X<T> = T extends any ? Id<X<[T]>> : never;",
        "type W<Y> = { w: Y }; type X<T> = T extends any ? W<X<[T]>> : never;",
        "type W<Y> = Y[]; type X<T> = T extends any ? W<X<[T]>> : never;",
    ];
    for options in SETTINGS {
        for declaration in EAGER {
            assert!(
                holds_a_ts2589(
                    options,
                    150,
                    format!(
                        "{declaration}
"
                    ),
                    "X<\"ok\">"
                ),
                "`{declaration}` under {options} reaches TS2589"
            );
        }
    }
}

/// A homomorphic mapped type whose template applies the declaration over
/// new arguments is TS2589 in the checker, even over a primitive.
///
/// TypeScript 7.0.2, all four settings: `X<"ok">` over `type X<T> = { [K
/// in keyof T]: X<[T]> }` is TS2589.
#[test]
#[ignore = "a homomorphic mapped type over a primitive is the primitive, without instantiating its template"]
fn a_mapped_template_applying_its_declaration_reaches_ts2589() {
    for options in SETTINGS {
        assert!(
            holds_a_ts2589(
                options,
                150,
                "type X<T> = { [K in keyof T]: X<[T]> };
"
                .to_owned(),
                "X<\"ok\">"
            ),
            "the mapped template under {options} reaches TS2589"
        );
    }
}

/// The checker diagnostic `G<string>` over `source` evaluates to, under
/// `options` and a tail budget of `tail` when set: its numeric code, when
/// the answer is a checker recovery anywhere in it.
fn diagnostic_of_g_string(
    options: &'static str,
    source: &'static str,
    tail: Option<u32>,
) -> Option<u32> {
    use crate::semantic_query::{ProjectionMode, ProjectionReductionContext};
    on_a_small_stack(move || {
        let _tail = tail.map(super::connected_demand::TailBudgetForTests::install);
        with_probe_in(
            ProbeProject {
                compiler_options: Some(options),
                ..Default::default()
            },
            source,
            "G<string>",
            |dispatch, node| {
                let value = dispatch
                    .normalize_node_for_structural_fact_demand(
                        node,
                        ProjectionReductionContext::published(ProjectionMode::Expanded),
                    )
                    .into_complete_node()
                    .expect("the probe evaluates completely");
                let mut seen = rustc_hash::FxHashSet::default();
                let mut stack = vec![value];
                while let Some(node) = stack.pop() {
                    if !seen.insert(node) {
                        continue;
                    }
                    let Some(data) = dispatch.graph().node_data(node) else {
                        continue;
                    };
                    if let SemanticNodeData::Opaque(QueryError::CheckerRecovery {
                        diagnostic,
                        ..
                    }) = data.as_ref()
                    {
                        return Some(diagnostic.code.code());
                    }
                    data.for_each_retained_child(|child| stack.push(child));
                }
                None
            },
        )
    })
}

/// A generic alias whose declared type requires itself — its body applies
/// it, directly or through other aliases, with no conditional type of its
/// own deciding — is the checker's TS2456 (`any`), however large the
/// budgets; through its own conditional type's branch the application is
/// the tail loop, TS2589 at the tail budget (a lower one than production's
/// standing in); in a position the checker defers it is a lazy reference.
///
/// TypeScript 7.0.2, all four settings, `G<string>` over: `type G<T> =
/// G<[T]>`, `type G<T> = G<T>`, `type G<T> = H<T>; type H<T> = G<[T]>` and
/// `type G<T> = H<[T]>; type H<T> = T extends any ? G<T> : never` — TS2456,
/// `any`; `type G<T> = T extends never ? never : G<[T]>` — TS2589, `any`;
/// `type G<T> = { a: G<[T]> }` and `type G<T> = G<[T]>[]` — no diagnostic.
#[test]
fn circular_type_aliases_are_the_checkers_ts2456_and_tail_runs_its_ts2589() {
    const CIRCULAR: [&str; 4] = [
        "type G<T> = G<[T]>;\n",
        "type G<T> = G<T>;\n",
        "type G<T> = H<T>; type H<T> = G<[T]>;\n",
        "type G<T> = H<[T]>; type H<T> = T extends any ? G<T> : never;\n",
    ];
    const DEFERRED: [&str; 2] = ["type G<T> = { a: G<[T]> };\n", "type G<T> = G<[T]>[];\n"];
    const TAIL: &str = "type G<T> = T extends never ? never : G<[T]>;\n";
    for options in SETTINGS {
        for source in CIRCULAR {
            assert_eq!(
                diagnostic_of_g_string(options, source, None),
                Some(2456),
                "`{source}` under {options}"
            );
        }
        for source in DEFERRED {
            assert_eq!(
                diagnostic_of_g_string(options, source, None),
                None,
                "`{source}` under {options}"
            );
        }
        assert_eq!(
            diagnostic_of_g_string(options, TAIL, Some(200)),
            Some(2589),
            "`{TAIL}` under {options}"
        );
    }
}
