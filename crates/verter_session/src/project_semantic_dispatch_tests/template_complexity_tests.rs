//! A template literal type over unions whose cross product reaches the
//! checker's limit is the checker's TS2590 recovery (`checkCrossProductUnion`
//! in `getTemplateLiteralType`): the product of the spans' constituent
//! counts is checked before a concatenation is built, and the answer reads
//! and relates as `any` — a resource partial, never kept. Under the limit
//! the template distributes.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured with `tsc
//! --declaration --emitDeclarationOnly` on the fixture each test builds,
//! each probe read off a TS2322 against `never`. The four `strictNullChecks`
//! × `noImplicitAny` settings agree on every probe.

use super::checker_probe_lane_tests::{mismatches, mismatches_in_one_host, with_recovered_probe};
use verter_type_engine::semantic_query::{
    CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation, QueryError,
    SemanticNodeData,
};

const IS_ANY: &str = "type IsAny<T> = 0 extends 1 & T ? \"any\" : \"not-any\";\n";

const DIGITS: &str = "type D = 0 | 1 | 2 | 3 | 4 | 5 | 6 | 7 | 8 | 9;\n";

/// `type <name> = "<prefix>0" | … | "<prefix><count - 1>";`
fn string_union(name: &str, prefix: &str, count: usize) -> String {
    let arms: Vec<String> = (0..count).map(|i| format!("\"{prefix}{i}\"")).collect();
    format!("type {name} = {};\n", arms.join(" | "))
}

/// The TS2590 recovery of a template literal type.
const TS2590: CheckerDiagnostic = CheckerDiagnostic {
    code: CheckerDiagnosticCode::UnionTooComplex,
    operation: CheckerDiagnosticOperation::TemplateLiteral,
};

/// Four digit spans are 10,000 concatenations; five are 100,000, the
/// checker's limit.
///
/// Measured: `IsAny<`${D}${D}${D}${D}`>` is `"not-any"`, `"0000" extends
/// `${D}${D}${D}${D}` ? 1 : 2` is `1` and `"x" extends …` is `2`;
/// `IsAny<`${D}${D}${D}${D}${D}`>` and `"x" extends `${D}${D}${D}${D}${D}` ?
/// 1 : 2` are `any` under TS2590. (A conditional extending the recovery is
/// the recovery; `IsAny` alone would also meet the limit in `1 & T`.)
#[test]
fn four_digit_spans_distribute_and_five_reach_the_limit() {
    let source = format!("{IS_ANY}{DIGITS}");
    let failures = mismatches(
        &source,
        &[
            ("IsAny<`${D}${D}${D}${D}`>", "\"not-any\""),
            ("\"0000\" extends `${D}${D}${D}${D}` ? 1 : 2", "1"),
            ("\"x\" extends `${D}${D}${D}${D}` ? 1 : 2", "2"),
            ("IsAny<`${D}${D}${D}${D}${D}`>", "any"),
            ("\"x\" extends `${D}${D}${D}${D}${D}` ? 1 : 2", "any"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Two spans at the limit: 369 × 271 = 99,999 distributes and 400 × 250 =
/// 100,000 does not.
///
/// Measured: `IsAny<`${A}-${B}`>` is `"not-any"` and `"a0-b0" extends
/// `${A}-${B}` ? 1 : 2` is `1` at 369 × 271; `IsAny<`${C}-${F}`>` and `"x"
/// extends `${C}-${F}` ? 1 : 2` are `any` under TS2590 at 400 × 250.
#[test]
fn two_spans_meet_the_limit_at_one_hundred_thousand() {
    let source = format!(
        "{IS_ANY}{}{}{}{}",
        string_union("A", "a", 369),
        string_union("B", "b", 271),
        string_union("C", "c", 400),
        string_union("F", "f", 250),
    );
    let failures = mismatches(
        &source,
        &[
            ("IsAny<`${A}-${B}`>", "\"not-any\""),
            ("\"a0-b0\" extends `${A}-${B}` ? 1 : 2", "1"),
            ("IsAny<`${C}-${F}`>", "any"),
            ("\"x\" extends `${C}-${F}` ? 1 : 2", "any"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A span whose union holds a template counts that template's
/// concatenations: `"x" | `${D}${D}${D}`` is 1,001 constituents, and two
/// more digit spans make 100,100; one more makes 10,010.
///
/// Measured: `IsAny<Nest>` and `"x" extends Nest ? 1 : 2` are `any` under
/// TS2590; `"x" extends Nest2 ? 1 : 2` is `2` and `"x0" extends Nest2 ? 1 :
/// 2` is `1`.
#[test]
fn a_template_inside_a_span_counts_its_concatenations() {
    let source = format!(
        "{IS_ANY}{DIGITS}type Nest = `${{\"x\" | `${{D}}${{D}}${{D}}`}}${{D}}${{D}}`;\n\
         type Nest2 = `${{\"x\" | `${{D}}${{D}}${{D}}`}}${{D}}`;\n"
    );
    let failures = mismatches(
        &source,
        &[
            ("IsAny<Nest>", "any"),
            ("\"x\" extends Nest ? 1 : 2", "any"),
            ("\"x\" extends Nest2 ? 1 : 2", "2"),
            ("\"x0\" extends Nest2 ? 1 : 2", "1"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A span counts its union's constituents, not their texts: `Y = S | D` is
/// twenty constituents spelling ten strings, so four `Y` spans are 160,000;
/// `` Z = S2 | `a${D}` `` is ten, the template forming the string literals
/// `S2` already holds, so four `Z` spans are 10,000.
///
/// Measured: `` IsAny<`${Y}${Y}${Y}${Y}`> `` and
/// `` "x" extends `${Y}${Y}${Y}${Y}` ? 1 : 2 `` are `any` under TS2590,
/// `` IsAny<`${Y}${Y}${Y}`> `` and `` IsAny<`${Z}${Z}${Z}${Z}`> `` are
/// `"not-any"`, `` "a0a1" extends `${Z}${Z}` ? 1 : 2 `` is `1` and
/// `` "0000" extends `${Z}${Z}` ? 1 : 2 `` is `2`.
#[test]
fn a_span_counts_constituents_not_texts() {
    let source = format!(
        "{IS_ANY}{DIGITS}{}{}type Y = S | D;\ntype Z = S2 | `a${{D}}`;\n",
        string_union("S", "", 10),
        string_union("S2", "a", 10),
    );
    let failures = mismatches(
        &source,
        &[
            ("IsAny<`${Y}${Y}${Y}${Y}`>", "any"),
            ("\"x\" extends `${Y}${Y}${Y}${Y}` ? 1 : 2", "any"),
            ("IsAny<`${Y}${Y}${Y}`>", "\"not-any\""),
            ("IsAny<`${Z}${Z}${Z}${Z}`>", "\"not-any\""),
            ("\"a0a1\" extends `${Z}${Z}` ? 1 : 2", "1"),
            ("\"0000\" extends `${Z}${Z}` ? 1 : 2", "2"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `boolean` is the two constituents `false | true`.
///
/// Measured: `` `${boolean}${1 | 2}` `` is `"false1" | "false2" | "true1" |
/// "true2"`, and `IsAny<`${boolean}${D}`>` is `"not-any"`.
#[test]
fn boolean_spans_two_constituents() {
    let source = format!("{IS_ANY}{DIGITS}");
    let failures = mismatches(
        &source,
        &[
            (
                "`${boolean}${1 | 2}`",
                "\"false1\" | \"false2\" | \"true1\" | \"true2\"",
            ),
            ("IsAny<`${boolean}${D}`>", "\"not-any\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Past the limit the template is the TS2590 recovery, a resource partial
/// holding the authored template as its origin: nothing is concatenated.
#[test]
fn the_ts2590_recovery_is_a_partial_holding_the_authored_template() {
    let source = format!("{IS_ANY}{DIGITS}");
    with_recovered_probe(&source, "`${D}${D}${D}${D}${D}`", |dispatch, node| {
        let data = dispatch.graph().node_data(node);
        let Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery {
            diagnostic,
            basis: verter_type_engine::semantic_query::RecoveryBasis::Budget,
            origin: Some(origin),
        })) = data.as_deref()
        else {
            panic!("the template must be the TS2590 recovery, measured {data:?}");
        };
        assert_eq!(*diagnostic, TS2590);
        assert!(
            matches!(
                dispatch.graph().node_data(*origin).as_deref(),
                Some(SemanticNodeData::TemplateLiteral { expressions, .. })
                    if expressions.len() == 5
            ),
            "its origin is the authored five-span template"
        );
    });
}

/// The answer at the limit is the same read cold or warm, and in either
/// order.
#[test]
fn the_limit_answers_the_same_cold_warm_and_reordered() {
    let source = format!("{IS_ANY}{DIGITS}");
    let over = ("\"x\" extends `${D}${D}${D}${D}${D}` ? 1 : 2", "any");
    let under = ("IsAny<`${D}${D}${D}${D}`>", "\"not-any\"");
    let under_relation = ("\"0000\" extends `${D}${D}${D}${D}` ? 1 : 2", "1");
    let mut failures = mismatches_in_one_host(&source, &[over, under, under_relation, over, under]);
    failures.extend(mismatches_in_one_host(
        &source,
        &[under_relation, under, over, under_relation, over],
    ));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A union absorbs the string literals a template literal type in it
/// already matches (`removeStringLiteralsMatchedByTemplateLiterals`), so
/// `A = "a0" | … | "a399" | `a${number}`` is the one constituent
/// `` `a${number}` `` and `` `${A}-${B}` `` over 250 `B` literals is 250
/// concatenations; without the pattern the same span is 400 × 250, the
/// checker's limit.
///
/// Measured: `A` is `` `a${number}` ``, `` IsAny<`${A}-${B}`> `` is
/// `"not-any"`, `` "x" extends `${A}-${B}` ? 1 : 2 `` is `2`, and `` "x"
/// extends `${A2}-${B}` ? 1 : 2 `` is `any` under TS2590.
#[test]
fn a_union_absorbs_the_literals_its_template_matches() {
    let source = format!(
        "{IS_ANY}type A = {} | `a${{number}}`;\n{}{}",
        (0..400)
            .map(|i| format!("\"a{i}\""))
            .collect::<Vec<_>>()
            .join(" | "),
        string_union("A2", "a", 400),
        string_union("B", "b", 250),
    );
    let failures = mismatches(
        &source,
        &[
            ("IsAny<`${A}-${B}`>", "\"not-any\""),
            ("\"x\" extends `${A}-${B}` ? 1 : 2", "2"),
            ("\"x\" extends `${A2}-${B}` ? 1 : 2", "any"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The union's literal reduction (`removeRedundantLiteralTypes`, then
/// `removeStringLiteralsMatchedByTemplateLiterals`): `string` absorbs a
/// template literal type and a string mapping, and a pattern literal type
/// absorbs every string literal (a string enum member included) whose
/// slices fit its placeholders, each text matched leftmost. A written union
/// is that reduced union too: `type U = "a" | string` is `string`.
///
/// Measured on TypeScript 7.0.2 (all four settings agree), with `enum E { A
/// = "a7", B = "zz" }`, `type U = "a" | string`, `type P = "a1" |
/// `a${number}`` and `type R = "ab" | `${string}${string}``: each probe below
/// prints as its row's answer.
#[test]
fn a_union_drops_the_literals_its_patterns_match() {
    let source = "enum E { A = \"a7\", B = \"zz\" }\n\
                  type U = \"a\" | string;\n\
                  type P = \"a1\" | `a${number}`;\n\
                  type R = \"ab\" | `${string}${string}`;\n";
    let failures = mismatches(
        source,
        &[
            ("\"a1\" | `a${number}`", "`a${number}`"),
            ("\"ab\" | `a${number}`", "\"ab\" | `a${number}`"),
            ("\"A\" | Uppercase<string>", "Uppercase<string>"),
            ("\"a\" | Uppercase<string>", "\"a\" | Uppercase<string>"),
            ("string | `a${number}`", "string"),
            ("string | Uppercase<string>", "string"),
            (
                "\"1e3\" | \" 1\" | \"x\" | `${number}`",
                "\"x\" | `${number}`",
            ),
            (
                "\"0x10\" | \"-0\" | \"1.5\" | `${bigint}`",
                "\"1.5\" | `${bigint}`",
            ),
            (
                "\"a-b\" | \"ab\" | `${string}-${string}`",
                "\"ab\" | `${string}-${string}`",
            ),
            ("E.A | E.B | `a${number}`", "E.B | `a${number}`"),
            (
                "\"a1b\" | `a${number}b` | \"a1c\"",
                "\"a1c\" | `a${number}b`",
            ),
            ("\"ab\" | `${string}${string}`", "string"),
            ("U", "string"),
            ("P", "`a${number}`"),
            ("R", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Every concatenation's bytes are reserved before it is built: under a
/// 4 KiB construction allowance, 40 × 40 concatenations stop on the memory
/// rail as a typed partial, never a truncated union.
#[test]
fn a_template_past_its_byte_allowance_is_a_memory_partial() {
    use verter_type_engine::semantic_query::{
        PartialReasonSet, ProjectionMode, ProjectionReductionContext, ResultCompleteness,
    };
    let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
    let dispatch = super::ProjectSemanticDispatch::new(&host);
    let graph = dispatch.graph();
    let union = |prefix: &str| {
        let members: Vec<verter_type_engine::semantic_query::SemanticNodeId> = (0..40)
            .map(|i| {
                graph.intern_node(SemanticNodeData::Literal(
                    verter_type_engine::semantic_query::LiteralValue::String(format!(
                        "{prefix}{i}"
                    )),
                ))
            })
            .collect();
        graph.intern_node(SemanticNodeData::Union(
            verter_type_engine::semantic_query::composite::CompositeList::test_fixture(
                std::sync::Arc::from(members.into_boxed_slice()),
            ),
        ))
    };
    let template = graph.intern_node(SemanticNodeData::TemplateLiteral {
        quasis: std::sync::Arc::from(
            vec![
                std::sync::Arc::<str>::from(""),
                std::sync::Arc::from("-"),
                std::sync::Arc::from(""),
            ]
            .into_boxed_slice(),
        ),
        expressions: std::sync::Arc::from(vec![union("a"), union("b")].into_boxed_slice()),
    });
    dispatch.set_construction_byte_limit_for_tests(4 << 10);
    let (_, completeness) = dispatch.evaluate_deferred_semantic_node_with_context_for_tests(
        template,
        ProjectionReductionContext::published(ProjectionMode::Expanded),
    );
    match completeness {
        ResultCompleteness::Partial(reasons) => assert!(
            reasons.contains(PartialReasonSet::CONNECTED_MEMORY_LIMIT)
                && !reasons.contains(PartialReasonSet::OPERATION_BUDGET),
            "the partial is the memory rail's, never the template's TS2590, got {reasons:?}"
        ),
        ResultCompleteness::Complete => panic!("4 KiB cannot hold 1,600 concatenations"),
    }
}

/// A recovery is a resource partial and never kept: a second read of the
/// same template on the same host evaluates it again and is again the
/// recovered partial, never a complete answer served from the memo.
#[test]
fn a_ts2590_recovery_is_never_kept() {
    use super::evaluate::StructuralFactDemandOutcome;
    let source = format!("{IS_ANY}{DIGITS}");
    let host = super::checker_probe_lane_tests::default_probe_host();
    let read = || {
        super::checker_probe_lane_tests::with_probe_outcome_on_host(
            &host,
            Default::default(),
            &source,
            "`${D}${D}${D}${D}${D}`",
            |_, outcome| match outcome {
                StructuralFactDemandOutcome::Recovered { reasons, .. } => Some(reasons),
                _ => None,
            },
        )
    };
    let cold = read();
    let warm = read();
    assert_eq!(
        cold,
        Some(verter_type_engine::semantic_query::PartialReasonSet::OPERATION_BUDGET),
        "cold"
    );
    assert_eq!(
        warm,
        Some(verter_type_engine::semantic_query::PartialReasonSet::OPERATION_BUDGET),
        "warm"
    );
}

/// A relation refused inside a span is the relation's own refusal: the span
/// reads the relation's false recovery and the template concatenates it,
/// partial through the relation, and never becomes the template's TS2590.
/// Two hundred object arms against the same arms reversed need about 20,100
/// comparisons, past an allowance of 600 (see the relation work tests).
#[test]
fn a_relation_refused_inside_a_span_is_not_the_templates_ts2590() {
    let arms = |value: &dyn Fn(usize) -> String, reversed: bool| {
        let mut arms: Vec<String> = (0..200)
            .map(|i| format!("{{ p{i}: {} }}", value(i)))
            .collect();
        if reversed {
            arms.reverse();
        }
        arms.join(" | ")
    };
    let source = format!(
        "type S = {};\ntype T = {};\n",
        arms(&|i| i.to_string(), false),
        arms(&|_| "number".to_owned(), true)
    );
    let span = "`${[S] extends [T] ? \"a\" : \"b\"}-x`";
    verter_type_engine::semantic_query::checker_policy::with_relation_comparisons_for_tests(
        600,
        || {
            with_recovered_probe(&source, span, |dispatch, node| {
                let data = dispatch.graph().node_data(node);
                assert!(
                    matches!(
                        data.as_deref(),
                        Some(SemanticNodeData::Literal(verter_type_engine::semantic_query::LiteralValue::String(text)))
                            if text == "b-x"
                    ),
                    "the span reads the relation's false and the template is `\"b-x\"`, \
                 measured {data:?}"
                );
            });
        },
    );
}
