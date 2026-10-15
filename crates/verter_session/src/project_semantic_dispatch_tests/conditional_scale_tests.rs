//! Conditional types over large literal collections answer at the sizes the
//! checker answers them, at a cost that grows with the input:
//!
//! - a recursive conditional that infers through an `as const` route tree
//!   (`Names` / `Paths` below) distributes over each tree level once, and
//!   the conditional nested in a distributed branch sees that branch's
//!   member, never the whole union the outer `infer` bound;
//! - a pattern that repeats an `infer` distributes over a 1,000-member union
//!   of distinct object aliases;
//! - a tuple of 1,000 distinct aliases of one generic infers through each
//!   alias once.
//!
//! Each demand is the named demand the semantic benchmark makes (`__Probe`
//! resolved by name and projected to its wire answer). Every expected answer
//! is TypeScript 7.0.2's: the union of the literals the construction
//! collects, measured with the benchmark's reference method
//! (`scripts/benchmark/semantic-perf/measure-expected.mjs`) at the listed
//! sizes and in all four `strictNullChecks` × `noImplicitAny` settings,
//! which agree, with no diagnostic.
//!
//! No size of these shapes may be refused below the checker's own limits.
//! The checker refuses with TS2589 once one statement has instantiated
//! 5,000,000 types; the request's operation fuse defaults to that same
//! allowance. So each demand here runs under a fuse of exactly the
//! instantiations the checker makes for the same program (measured with
//! `tsc --extendedDiagnostics`): answering within it at every size shows
//! the fuse never stops one of these shapes before the checker would.

use std::collections::BTreeSet;
use std::sync::Arc;

use verter_type_engine::project_semantic_dispatch::raise::{
    enable_dispatch_trace_for_test, DISPATCH_TRACE,
};
use verter_type_expr::{LiteralValue, TypeExpr};

const SCENARIO: &str = "/scale/scenario.ts";

/// The route tree of `parents` parents with five children each, and the two
/// recursive conditionals that collect its names and joined paths.
fn route_tree(parents: usize) -> String {
    let mut source = String::from("const routes = [\n");
    for i in 0..parents {
        let children = (0..5)
            .map(|j| format!("{{ name: \"r{i}c{j}\", path: \"c{j}\" }}"))
            .collect::<Vec<_>>()
            .join(", ");
        source.push_str(&format!(
            "  {{ name: \"r{i}\", path: \"/r{i}\", children: [{children}] }},\n"
        ));
    }
    source.push_str(
        "] as const;\n\
         type Names<R> = R extends readonly (infer E)[]\n\
         \x20 ? E extends { name: infer N } ? N | (E extends { children: infer C } ? Names<C> : never) : never\n\
         \x20 : never;\n\
         type Paths<R, P extends string = \"\"> = R extends readonly (infer E)[]\n\
         \x20 ? E extends { path: infer S extends string }\n\
         \x20   ? `${P}${S}` | (E extends { children: infer C } ? Paths<C, `${P}${S}/`> : never)\n\
         \x20   : never\n\
         \x20 : never;\n",
    );
    source
}

fn route_names(parents: usize) -> BTreeSet<Literal> {
    (0..parents)
        .flat_map(|i| {
            std::iter::once(format!("r{i}")).chain((0..5).map(move |j| format!("r{i}c{j}")))
        })
        .map(Literal::String)
        .collect()
}

fn route_paths(parents: usize) -> BTreeSet<Literal> {
    (0..parents)
        .flat_map(|i| {
            std::iter::once(format!("/r{i}")).chain((0..5).map(move |j| format!("/r{i}/c{j}")))
        })
        .map(Literal::String)
        .collect()
}

/// `members` object aliases whose two positions repeat one `infer`, and the
/// conditional over their union.
fn repeated_infer(members: usize) -> (String, String) {
    let mut source = String::new();
    for i in 0..members {
        source.push_str(&format!("type M{i} = {{ a: {i}; b: (x: {i}) => void }};\n"));
    }
    source.push_str("type Pat<T> = T extends { a: infer U; b: (x: infer U) => void } ? U : 0;\n");
    let union = (0..members)
        .map(|i| format!("M{i}"))
        .collect::<Vec<_>>()
        .join(" | ");
    (source, format!("Pat<{union}>"))
}

/// `members` distinct aliases of one generic interface, and the inference
/// through a tuple of one application of each.
fn distinct_aliases(members: usize) -> (String, String) {
    let mut source = String::from("interface Box<T> { v: T }\n");
    for i in 0..members {
        source.push_str(&format!("type B{i}<T> = Box<T>;\n"));
    }
    let tuple = (0..members)
        .map(|i| format!("B{i}<{i}>"))
        .collect::<Vec<_>>()
        .join(", ");
    (
        source,
        format!("[{tuple}] extends Box<infer P>[] ? P : never"),
    )
}

fn numbers(members: usize) -> BTreeSet<Literal> {
    (0..members).map(|i| Literal::Number(i as u64)).collect()
}

/// The types TypeScript 7.0.2 instantiates checking each program, in its
/// `--extendedDiagnostics` count (measured at 0, 10, 100 and 1,000, and
/// exactly linear): `Names` 219 per parent, `Paths` 234, the repeated
/// `infer` 7 per member and the distinct aliases 5.
fn checker_instantiations_names(parents: usize) -> usize {
    219 * parents + 40
}

fn checker_instantiations_paths(parents: usize) -> usize {
    234 * parents + 41
}

fn checker_instantiations_repeated_infer(members: usize) -> usize {
    7 * members + 4
}

fn checker_instantiations_aliases(members: usize) -> usize {
    5 * members + 20
}

/// A literal member of an answer.
#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord)]
enum Literal {
    String(String),
    Number(u64),
}

/// The work a demand did, by the counters a growth test compares.
type Work = [(&'static str, u64); 6];

/// Resolve `__Probe = probe` in a fresh project holding `source`, as the
/// benchmark's probe does, and return its literal members and its work. An
/// answer that is not a union of literals (an unevaluated conditional, the
/// probe expression itself, a template not built into its string) fails.
///
/// The request's operation fuse is `operations`.
fn named_probe(source: &str, probe: &str, operations: usize) -> (BTreeSet<Literal>, Work) {
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig {
            projection_op_budget: operations,
            ..crate::HostConfig::default()
        },
        &[("/scale", r#"{ "compilerOptions": { "strict": true } }"#)],
    );
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(SCENARIO.into()),
            input_id: SCENARIO.into(),
            source: Arc::from(format!("{source}type __Probe = {probe};\nexport {{}};\n")),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .expect("fixture admitted");
    let _trace = enable_dispatch_trace_for_test();
    let ((outcome, _), usage) =
        verter_type_engine::project_semantic_dispatch::connected_demand::with_connected_work_capture_for_tests(
            || {
                host.resolve_named_symbol_with_audit(SCENARIO, "__Probe", None)
                    .into_parts()
            },
        );
    let node = outcome
        .expect("the named demand completes")
        .expect("the probe exists");
    let stats = host.project_type_store().semantic_graph().stats_snapshot();
    let dispatches = DISPATCH_TRACE.with(|trace| trace.borrow().len()) as u64;
    let bytes = host
        .project_node_to_type_expr_json_bytes(node)
        .expect("the answer projects");
    let answer: TypeExpr = serde_json::from_slice(&bytes).expect("a wire answer");
    let TypeExpr::Union(arms) = &answer else {
        panic!("`{}` is not a union of literals", short(&answer));
    };
    let literals = arms
        .iter()
        .map(|arm| match arm {
            TypeExpr::Literal(LiteralValue::String(value)) => Literal::String(value.to_string()),
            TypeExpr::Literal(LiteralValue::Number(value)) if value.fract() == 0.0 => {
                Literal::Number(*value as u64)
            }
            other => panic!("the answer holds `{}`, not a literal", short(other)),
        })
        .collect();
    let work = [
        ("connected work", usage.work as u64),
        ("query reads", dispatches),
        ("cold builds", stats.misses),
        ("decided conditionals", stats.conditional_decided_count),
        ("relation checks", stats.relation_check_count),
        ("substitutions", stats.substitute_memo_misses),
    ];
    (literals, work)
}

fn short(expr: &TypeExpr) -> String {
    format!("{expr:?}").chars().take(300).collect()
}

fn assert_answer(label: &str, answer: &BTreeSet<Literal>, expected: &BTreeSet<Literal>) {
    let missing: Vec<_> = expected.difference(answer).take(5).collect();
    let extra: Vec<_> = answer.difference(expected).take(5).collect();
    assert!(
        missing.is_empty() && extra.is_empty(),
        "{label}: {} members, missing {missing:?}, extra {extra:?}",
        answer.len()
    );
}

/// Between a size and a larger one each work counter may grow at most 1.5
/// times faster than the input: a quadratic term grows 9 times over the
/// route tree's threefold step and 100 times over the unions' tenfold one.
fn assert_linear_growth(
    label: &str,
    (small, before): (usize, Work),
    (large, after): (usize, Work),
) {
    for ((what, before), (_, after)) in before.into_iter().zip(after) {
        println!("{label}: {what} {before} at {small}, {after} at {large}");
        assert!(
            2 * after * small as u64 <= 3 * before.max(1) * large as u64,
            "{label}: {what} grew from {before} at {small} to {after} at {large}"
        );
    }
}

/// The route tree answers every name at each size the checker answers,
/// with work linear from the middle size to the largest. Measured: 120, 600
/// and 1,800 string literals.
#[test]
fn route_tree_names_answer_at_every_size_with_linear_work() {
    let work = [20, 100, 300].map(|parents| {
        let (names, work) = named_probe(
            &route_tree(parents),
            "Names<typeof routes>",
            checker_instantiations_names(parents),
        );
        assert_answer(
            &format!("Names over {parents}"),
            &names,
            &route_names(parents),
        );
        (parents, work)
    });
    assert_linear_growth("Names", work[1], work[2]);
}

/// The route tree answers every joined path, each a string the template
/// builds, at each size the checker answers, with work linear from the
/// middle size to the largest. Measured: 120, 600 and 1,800 string literals.
#[test]
fn route_tree_paths_answer_at_every_size_with_linear_work() {
    let work = [20, 100, 300].map(|parents| {
        let (paths, work) = named_probe(
            &route_tree(parents),
            "Paths<typeof routes>",
            checker_instantiations_paths(parents),
        );
        assert_answer(
            &format!("Paths over {parents}"),
            &paths,
            &route_paths(parents),
        );
        (parents, work)
    });
    assert_linear_growth("Paths", work[1], work[2]);
}

/// A conditional nested in a distributed branch over an `infer` binding sees
/// the branch's member: each parent joins only its own children. Measured:
/// `"/a" | "/a/x" | "/b" | "/b/y"`.
#[test]
fn a_nested_conditional_over_a_distributed_infer_sees_one_member() {
    let source = "const routes = [\n\
                  \x20 { name: \"a\", path: \"/a\", children: [{ name: \"x\", path: \"x\" }] },\n\
                  \x20 { name: \"b\", path: \"/b\", children: [{ name: \"y\", path: \"y\" }] },\n\
                  ] as const;\n"
        .to_string()
        + route_tree(0)
            .split_once("] as const;\n")
            .expect("the aliases")
            .1;
    let (paths, _) = named_probe(
        &source,
        "Paths<typeof routes>",
        checker_instantiations_paths(2),
    );
    let expected = ["/a", "/a/x", "/b", "/b/y"]
        .map(|path| Literal::String(path.into()))
        .into();
    assert_answer("Paths over two parents", &paths, &expected);
}

/// The repeated-infer pattern answers at each size the checker answers,
/// with work linear from the middle size to the largest. Measured: the
/// numbers `0` to `n - 1`.
#[test]
fn repeated_infer_answers_at_every_size_with_linear_work() {
    let work = [10, 100, 1000].map(|members| {
        let (source, probe) = repeated_infer(members);
        let (answer, work) = named_probe(
            &source,
            &probe,
            checker_instantiations_repeated_infer(members),
        );
        assert_answer(&format!("Pat over {members}"), &answer, &numbers(members));
        (members, work)
    });
    assert_linear_growth("Pat", work[1], work[2]);
}

/// The distinct aliases answer at each size the checker answers, with work
/// linear from the smaller size to the larger. Measured: the numbers `0` to
/// `n - 1`.
#[test]
fn distinct_aliases_answer_at_every_size_with_linear_work() {
    let work = [100, 1000].map(|members| {
        let (source, probe) = distinct_aliases(members);
        let (answer, work) = named_probe(&source, &probe, checker_instantiations_aliases(members));
        assert_answer(
            &format!("aliases over {members}"),
            &answer,
            &numbers(members),
        );
        (members, work)
    });
    assert_linear_growth("aliases", work[0], work[1]);
}

/// The first level of `Names` over 2,000 parents answers every parent's
/// name. Its `infer E` binds the union of the tree's 2,000 elements, each a
/// distinct object holding a tuple of five more (28,000 nodes); building
/// that union keys every element by its structure, however many nodes the
/// elements hold, as the checker reads every constituent of a union it
/// builds. Measured on TypeScript 7.0.2: `"r0" | … | "r1999"`, with no
/// diagnostic.
#[test]
fn a_conditional_over_a_two_thousand_parent_tree_answers_every_parent() {
    let parents = 2000;
    let (names, _) = named_probe(
        &route_tree(parents),
        "typeof routes extends readonly (infer E)[] ? E extends { name: infer N } ? N : never : never",
        verter_type_engine::request_budget::DEFAULT_PROJECTION_OP_BUDGET,
    );
    let expected = (0..parents)
        .map(|i| Literal::String(format!("r{i}")))
        .collect();
    assert_answer("the first level over 2,000 parents", &names, &expected);
}

/// A template whose holes an `infer` binds to `null`, `undefined` or a
/// union of literals is the strings it builds, as the checker's template
/// construction folds those holes into its text. Measured on TypeScript
/// 7.0.2: `"xtrue" | "xnull" | "xundefined"` for the nullish holes, and
/// `"p/r0" | "q/r0" | "p/r0/c0" | … | "q/r0/c4"` for a union prefix.
#[test]
fn templates_built_at_substitution_fold_nullish_and_union_holes() {
    let source = "const items = [{ n: true }, { n: null }, { n: undefined }] as const;\n\
                  type X<R> = R extends readonly (infer E)[]\n\
                  \x20 ? E extends { n: infer N extends boolean | null | undefined } ? `x${N}` : never\n\
                  \x20 : never;\n";
    let (strings, _) = named_probe(
        source,
        "X<typeof items>",
        verter_type_engine::request_budget::DEFAULT_PROJECTION_OP_BUDGET,
    );
    let expected = ["xtrue", "xnull", "xundefined"]
        .map(|text| Literal::String(text.into()))
        .into();
    assert_answer("nullish holes", &strings, &expected);

    let (paths, _) = named_probe(
        &route_tree(1),
        "Paths<typeof routes, \"p\" | \"q\">",
        verter_type_engine::request_budget::DEFAULT_PROJECTION_OP_BUDGET,
    );
    let expected = ["p", "q"]
        .into_iter()
        .flat_map(|prefix| {
            std::iter::once(format!("{prefix}/r0"))
                .chain((0..5).map(move |j| format!("{prefix}/r0/c{j}")))
        })
        .map(Literal::String)
        .collect();
    assert_answer("a union prefix", &paths, &expected);
}
