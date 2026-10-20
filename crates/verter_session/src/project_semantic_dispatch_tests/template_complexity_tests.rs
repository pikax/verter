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
/// 100,000 does not. The rows are read from one host: the 99,999
/// concatenations are built once, and the relation reads them again.
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
    let failures = mismatches_in_one_host(
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
    let _builder = dispatch.enter_construction_scope();
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
        quasis: verter_type_expr::OwnedSlice::<verter_type_expr::OwnedStr>::from(
            vec![
                verter_type_expr::OwnedStr::from(""),
                verter_type_expr::OwnedStr::from("-"),
                verter_type_expr::OwnedStr::from(""),
            ]
            .into_boxed_slice(),
        ),
        expressions: verter_type_engine::semantic_query::PayloadArray::from(std::sync::Arc::from(
            vec![union("a"), union("b")].into_boxed_slice(),
        )),
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

/// `W` is the 1,000 three-digit strings.
const THREE_DIGITS: &str = "type W = `${D}${D}${D}`;\n";

/// A `never` span empties the product wherever it sits: the template is
/// `never` whatever the other spans are — open, or a product far past the
/// checker's limit — and nothing is concatenated.
///
/// Measured: each of the first four probes is `never`, with no diagnostic
/// (`W × W` alone is 1,000,000 concatenations, TS2590). Past them, seven
/// `W` spans before or after `never` overflow any machine count before the
/// empty span is reached; the checker distributes such a template span by
/// span and does not finish, while the product is empty, so `never` answers
/// beyond it.
#[test]
fn an_empty_span_empties_the_product_wherever_it_sits() {
    let source = format!("{DIGITS}{THREE_DIGITS}");
    let failures = mismatches(
        &source,
        &[
            (
                "ReturnType<<T extends string>() => `${T}${never}`>",
                "never",
            ),
            (
                "ReturnType<<T extends string>() => `x${never}${T}`>",
                "never",
            ),
            ("`${W}${W}${never}`", "never"),
            ("`${never}${W}${W}`", "never"),
            ("`${W}${W}${W}${W}${W}${W}${W}${never}`", "never"),
            ("`${never}${W}${W}${W}${W}${W}${W}${W}`", "never"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The wire answer of `type __Probe = <probe>` resolved by name, as the
/// semantic benchmark demands it, with the connected work it charged.
fn named_template_probe(
    source: &str,
    probe: &str,
) -> (
    verter_type_expr::TypeExpr,
    verter_type_engine::project_semantic_dispatch::connected_demand::DemandUsage,
) {
    const SCENARIO: &str = "/template/scenario.ts";
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[("/template", r#"{ "compilerOptions": { "strict": true } }"#)],
    );
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(SCENARIO.into()),
            input_id: SCENARIO.into(),
            source: std::sync::Arc::from(format!(
                "{source}type __Probe = {probe};\nexport {{}};\n"
            )),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .expect("fixture admitted");
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
    let bytes = host
        .project_node_to_type_expr_json_bytes(&node)
        .expect("the answer projects");
    (
        serde_json::from_slice(&bytes).expect("a wire answer"),
        usage,
    )
}

/// Resolved by name, as the benchmark asks for it, a template at the
/// checker's limit answers its TS2590 recovery — never the probe
/// expression unevaluated — and one under the limit its union.
#[test]
fn a_named_template_at_the_limit_answers_its_recovery() {
    use verter_type_expr::TypeExpr;
    let spans = |count: usize| "${D}".repeat(count);
    let (under, _) = named_template_probe(DIGITS, &format!("`{}`", spans(4)));
    assert!(
        matches!(&under, TypeExpr::Union(arms) if arms.len() == 10_000),
        "four digit spans are 10,000 strings"
    );
    let two_spans = format!(
        "{}{}",
        string_union("C", "c", 400),
        string_union("F", "f", 250)
    );
    for (source, probe) in [
        (DIGITS.to_owned(), format!("`{}`", spans(5))),
        (two_spans, "`${C}-${F}`".to_owned()),
    ] {
        let (answer, _) = named_template_probe(&source, &probe);
        let rendered = format!("{answer:?}");
        assert!(
            !matches!(answer, TypeExpr::TemplateLiteral { .. }),
            "`{probe}` is the checker's recovery, not the probe unevaluated: {rendered:.300}"
        );
        assert!(
            matches!(
                answer,
                TypeExpr::Primitive(verter_type_expr::PrimitiveName::Any)
            ),
            "`{probe}` answers the TS2590 recovery, the error type `any`: {rendered:.300}"
        );
    }
}

/// The work and the construction bytes a template charges grow with the
/// concatenations it builds: four times the concatenations charge at most
/// six times the work and the bytes (a quadratic term would charge
/// sixteen), and the work is never below one unit per concatenation.
#[test]
fn a_template_charges_in_the_linear_class() {
    let measure = |a: usize, b: usize| {
        let source = format!("{}{}", string_union("A", "a", a), string_union("B", "b", b));
        let (answer, usage) = named_template_probe(&source, "`${A}-${B}`");
        assert!(
            matches!(&answer, verter_type_expr::TypeExpr::Union(arms) if arms.len() == a * b),
            "{a} × {b} concatenations"
        );
        assert!(
            usage.work >= a * b,
            "one unit per concatenation at {a} × {b}"
        );
        assert!(usage.bytes > 0, "bytes charged at {a} × {b}");
        (usage.work as u64, usage.bytes)
    };
    let (small_work, small_bytes) = measure(50, 40);
    let (large_work, large_bytes) = measure(100, 80);
    println!("work {small_work} -> {large_work}, bytes {small_bytes} -> {large_bytes}");
    assert!(
        large_work <= 6 * small_work,
        "work grew from {small_work} to {large_work}"
    );
    assert!(
        large_bytes <= 6 * small_bytes,
        "bytes grew from {small_bytes} to {large_bytes}"
    );
}
