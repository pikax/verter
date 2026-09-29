//! A chain of instantiations evaluates as a chain of continuation frames,
//! not of native calls: an alias whose body applies the next alias needs
//! that instantiation from the runtime, which parks the frame on the heap
//! while the next one builds. The answer no longer depends on how deep the
//! chain is — neither the stack the caller runs on nor the nested-query
//! depth the connected demand counts for native re-entry decides it.
//!
//! Every expected answer is TypeScript 7.0.2's, measured through the capped
//! `tsc` with `--ignoreConfig`.

use super::checker_probe_lane_tests::{evaluated_mismatches, with_probe};
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
#[ignore = "the request's projection-operation budget of 2,000 stops an alias chain the checker instantiates"]
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

/// Conditional arguments nested through a chain of aliases evaluate on a
/// 1 MiB thread up to the checker's instantiation depth.
///
/// TypeScript 7.0.2, all four `strictNullChecks` × `noImplicitAny`
/// settings: `E97<"ok">` is `"ok"`; `E98<"ok">` is TS2589.
#[test]
fn conditional_arguments_nested_97_deep_answer_on_a_one_mebibyte_stack() {
    let source = conditional_argument_chain(97);
    let mismatches =
        on_a_small_stack(move || evaluated_mismatches(&source, &[("E97<\"ok\">", "\"ok\"")]));
    assert_eq!(mismatches, Vec::<String>::new());
}

/// One level deeper, the checker's instantiation of the declared alias type
/// — a conditional nested once per alias — reaches its depth limit: TS2589,
/// whose recovery reads as `any`.
///
/// TypeScript 7.0.2, all four `strictNullChecks` × `noImplicitAny`
/// settings: `E98<"ok">`, `E200<"ok">` and `E500<"ok">` are TS2589.
#[test]
#[ignore = "the checker's instantiation depth over a declared alias type is not counted"]
fn conditional_arguments_nested_98_deep_are_the_checker_recovery() {
    for length in [98, 200, 500] {
        let source = conditional_argument_chain(length);
        let recovered = on_a_small_stack(move || {
            with_probe(&source, &format!("E{length}<\"ok\">"), |dispatch, node| {
                matches!(
                    dispatch.graph().node_data(node).as_deref(),
                    Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery { diagnostic, .. }))
                        if diagnostic.code == CheckerDiagnosticCode::ExcessivelyDeepInstantiation
                )
            })
        });
        assert!(
            recovered,
            "E{length}<\"ok\"> is the checker's TS2589 recovery"
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
