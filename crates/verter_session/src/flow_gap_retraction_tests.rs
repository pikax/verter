use std::sync::Arc;

use super::u6_flow_expect_tests::{
    make_audit_host,
    matrix::{iife_position_program, COVERED_POSITIONS, IIFE_EFFECT_REFUSAL},
};
use super::upsert;
use crate::host_flow_return_audit::FlowReturnError;
use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    FlowGap, FlowInputContext, FlowReturnDegradation, FlowReturnFailure, FlowReturnKey,
    ReturnProjectionDemand, SemanticQueryKey,
};
use crate::{FileLanguage, VerterHost};
use verter_type_expr::facts::{FlowFunctionReturnIdentity, FunctionPartIdentity};
use verter_type_expr::locators::{AuthoredAnchor, LocatorSymbolSpace};

#[derive(Debug)]
struct Sample {
    from_cache: bool,
    cold_computes: u32,
    degradation: Option<FlowReturnDegradation>,
    error: Option<FlowReturnError>,
    json: Option<String>,
    projected: Option<String>,
    candidates: usize,
}

#[derive(Debug)]
struct Trace {
    first: Sample,
    second: Sample,
}

fn identity(canonical: &str, symbol: &str) -> FlowFunctionReturnIdentity {
    FlowFunctionReturnIdentity {
        anchor: AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: LocatorSymbolSpace::Value,
        },
        function_part: FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    }
}

fn candidate_count(host: &Arc<VerterHost>, canonical: &str, function: &str) -> usize {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let key = FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(canonical),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(function),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(canonical),
        demand: ReturnProjectionDemand::whole_return(),
        input: FlowInputContext::empty(),
        result_contract:
            crate::project_semantic_dispatch::flow_solve::flow_return_result_contract_id(),
    };
    dispatch
        .graph()
        .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)))
}

fn call_once(host: &Arc<VerterHost>, canonical: &str, function: &str) -> Sample {
    let carrier = host.get_flow_return_type_with_audit(
        &identity(canonical, function),
        ReturnProjectionDemand::whole_return(),
    );
    let audit = carrier.audit();
    let payload = audit
        .flow_return_inference_payload()
        .expect("flow-return audit payload");
    let (degradation, error, json, projected) = match carrier.as_result() {
        Ok(result) => {
            let node = result.return_type();
            (
                result.degradation(),
                None,
                host.project_node_to_type_expr_json_bytes(node)
                    .map(|bytes| String::from_utf8(bytes).expect("UTF-8 TypeExpr JSON")),
                host.project_node_to_type_expr_for_test(node)
                    .map(|ty| format!("{ty:?}")),
            )
        }
        Err(error) => (None, Some(*error), None, None),
    };
    Sample {
        from_cache: audit.from_cache,
        cold_computes: payload.cold_computes,
        degradation,
        error,
        json,
        projected,
        candidates: candidate_count(host, canonical, function),
    }
}

fn run_on(host: &Arc<VerterHost>, canonical: &str, function: &str) -> Trace {
    Trace {
        first: call_once(host, canonical, function),
        second: call_once(host, canonical, function),
    }
}

fn run(id: &str, script: &str, function: &str) -> Trace {
    let host = make_audit_host();
    let canonical = format!("/flow-gap-retraction/{id}.ts");
    upsert(
        &host,
        &canonical,
        &super::module_script(script),
        FileLanguage::script_ts(),
    );
    run_on(&host, &canonical, function)
}

fn record_trace(id: &str, trace: &Trace) {
    eprintln!("FLOW_GAP_CASE {id}: {trace:#?}");
}

fn assert_partial(trace: &Trace, gap: FlowGap) {
    let expected = Some(FlowReturnDegradation::FlowGap(gap));
    for sample in [&trace.first, &trace.second] {
        assert_eq!(
            sample.error, None,
            "partial result remains usable: {trace:#?}"
        );
        assert_eq!(sample.degradation, expected, "exact typed gap: {trace:#?}");
        assert!(
            !sample.from_cache,
            "partial result must stay cold: {trace:#?}"
        );
        assert!(
            sample.cold_computes >= 1,
            "cold work must be nonzero: {trace:#?}"
        );
        assert_eq!(
            sample.candidates, 0,
            "partial result admits no candidate: {trace:#?}"
        );
        assert!(
            sample.json.is_some(),
            "partial result must project: {trace:#?}"
        );
        assert!(
            !sample
                .projected
                .as_deref()
                .unwrap_or_default()
                .contains("Primitive(Any)"),
            "a gap must not fabricate any: {trace:#?}"
        );
    }
    assert_eq!(
        trace.first.json, trace.second.json,
        "cold replay must be stable"
    );
}

fn assert_complete_warm(trace: &Trace, expected_json: Option<&str>) {
    assert_eq!(
        trace.first.error, None,
        "first call must return a value: {trace:#?}"
    );
    assert_eq!(
        trace.second.error, None,
        "second call must return a value: {trace:#?}"
    );
    assert_eq!(
        trace.first.degradation, None,
        "first call must be complete: {trace:#?}"
    );
    assert_eq!(
        trace.second.degradation, None,
        "second call must be complete: {trace:#?}"
    );
    assert!(
        !trace.first.from_cache,
        "first call must be cold: {trace:#?}"
    );
    assert!(
        trace.first.cold_computes >= 1,
        "first call must do work: {trace:#?}"
    );
    assert!(
        trace.second.from_cache,
        "second call must be warm: {trace:#?}"
    );
    assert_eq!(
        trace.second.cold_computes, 0,
        "warm call must do no cold work: {trace:#?}"
    );
    assert_eq!(
        trace.first.candidates, 1,
        "complete result admits one candidate: {trace:#?}"
    );
    assert_eq!(
        trace.second.candidates, 1,
        "warm candidate remains present: {trace:#?}"
    );
    assert_eq!(
        trace.first.json, trace.second.json,
        "warm replay must be byte-stable"
    );
    if let Some(expected) = expected_json {
        assert_eq!(trace.first.json.as_deref(), Some(expected));
    }
}

#[test]
fn flow_gap_known_gap_results_are_typed_partial_and_never_warm() {
    let fixtures = [
        (
            "g3",
            "declare const A_KIND: unique symbol; declare const B_KIND: unique symbol;\ntype A = { kind: typeof A_KIND; a: number }; type B = { kind: typeof B_KIND; b: number };\nfunction isA(x: A | B): x is A { return x.kind === A_KIND }\nfunction isB(x: A | B): x is B { return x.kind === B_KIND }\nfunction makeProps(x: A | B) { return { v: isA(x) ? (isB(x) ? x : \"ok\") : \"no\" } }",
            FlowGap::NominalRelation,
        ),
        (
            "g6",
            "function makeProps() { let x: \"a\" | \"b\" = \"a\"; const f = () => x; x = \"b\"; return f }",
            FlowGap::ClosureCapture,
        ),
        (
            "g7_sibling",
            "function makeProps() { let x: \"a\" | \"b\" = \"a\"; const w = () => { x = \"b\" }; void w; return () => x }",
            FlowGap::ClosureCapture,
        ),
        (
            "g7_deeper",
            "function makeProps() { let x: \"a\" | \"b\" = \"a\"; const w = () => () => { x = \"b\" }; void w; return () => x }",
            FlowGap::ClosureCapture,
        ),
        (
            "g9",
            "function makeProps(v: string | number) { if (typeof v === \"string\") { return () => v } return () => \"z\" as const }",
            FlowGap::ClosureCapture,
        ),
        (
            "g11_sequence",
            "function makeProps() { return (0, () => \"a\" as const) }",
            FlowGap::UnmodeledExpression,
        ),
        (
            "g11_this",
            "function makeProps(this: { value: \"a\" }) { return () => this.value }",
            FlowGap::UnmodeledExpression,
        ),
    ];

    for (id, script, gap) in fixtures {
        let trace = run(id, script, "makeProps");
        record_trace(id, &trace);
        assert_partial(&trace, gap);
    }
}

#[test]
fn flow_gap_invoked_closure_effect_is_position_independent_no_value() {
    for position in COVERED_POSITIONS {
        let trace = run(
            &format!("iife_{}", position.id()),
            &iife_position_program(*position),
            "makeProps",
        );
        record_trace(&format!("iife_{}", position.id()), &trace);
        for sample in [&trace.first, &trace.second] {
            assert_eq!(
                sample.error,
                Some(IIFE_EFFECT_REFUSAL),
                "{position:?}: {trace:#?}"
            );
            assert!(
                !sample.from_cache,
                "refusal must stay cold: {position:?}: {trace:#?}"
            );
            assert!(
                sample.cold_computes >= 1,
                "refusal must recompute: {position:?}: {trace:#?}"
            );
            assert_eq!(
                sample.candidates, 0,
                "refusal admits no candidate: {position:?}"
            );
        }
    }
}

#[test]
fn flow_gap_authored_any_remains_complete_and_warm() {
    let script = "function explicit(x: any) { return x }\nfunction implicit(x) { return x }\nfunction rest(...x) { return x }\nfunction callAny(callable: any) { return callable() }\nfunction asserted(x: unknown) { return x as any }";
    for function in ["explicit", "implicit", "rest", "callAny", "asserted"] {
        let trace = run(&format!("authored_any_{function}"), script, function);
        record_trace(&format!("authored_any_{function}"), &trace);
        assert_eq!(
            trace.first.degradation, None,
            "authored-any fixture {function}: {trace:#?}"
        );
        assert_complete_warm(&trace, None);
        if function == "callAny" {
            assert_eq!(
                trace.first.projected.as_deref(),
                Some("Primitive(Any)"),
                "callable any must project to semantic any: {trace:#?}"
            );
            assert_eq!(
                trace.second.projected.as_deref(),
                Some("Primitive(Any)"),
                "warm callable any must preserve semantic any: {trace:#?}"
            );
            assert_eq!(
                trace.first.projected, trace.second.projected,
                "callable any projection must be stable"
            );
        }
    }
}

fn assert_refused(trace: &Trace) {
    for sample in [&trace.first, &trace.second] {
        assert!(!sample.from_cache, "refusal must stay cold: {trace:#?}");
        assert!(
            sample.cold_computes >= 1,
            "refusal must recompute: {trace:#?}"
        );
        assert_eq!(
            sample.candidates, 0,
            "refusal admits no candidate: {trace:#?}"
        );
        assert!(
            sample.error.is_some() || sample.degradation.is_some(),
            "refusal needs a typed reason: {trace:#?}",
        );
    }
}

#[test]
fn optional_any_requires_the_reaching_value_to_still_be_any() {
    let written = run(
        "optional_any_after_write",
        "function makeProps(x: any) { x = \"s\"; return x?.length }",
        "makeProps",
    );
    record_trace("optional_any_after_write", &written);
    assert_refused(&written);

    let narrowed = run(
        "optional_any_after_narrow",
        "type T = { length: number }; function isT(x: any): x is T { return true } function makeProps(x: any) { if (isT(x)) return x?.length; return 0 }",
        "makeProps",
    );
    record_trace("optional_any_after_narrow", &narrowed);
    assert_refused(&narrowed);

    let typeof_narrowed = run(
        "optional_any_after_typeof_narrow",
        "function makeProps(x: any) { if (typeof x === \"string\") return x?.length; return 0 }",
        "makeProps",
    );
    record_trace("optional_any_after_typeof_narrow", &typeof_narrowed);
    assert_refused(&typeof_narrowed);
}

#[test]
fn optional_any_admits_the_complete_pure_member_and_terminal_call_class() {
    let cases = [
        ("param_member", "function makeProps(a: any) { return a?.b }"),
        (
            "local_member",
            "function makeProps() { const a: any = {}; return a?.b }",
        ),
        (
            "optional_param",
            "function makeProps(a?: any) { return a?.b }",
        ),
        (
            "computed_member",
            "function makeProps(a: any, k: string) { return a?.[k] }",
        ),
        (
            "computed_unary",
            "function makeProps(a: any) { return a?.[-1] }",
        ),
        (
            "computed_member_expression",
            "function makeProps(a: any, k: { id: string }) { return a?.[k.id] }",
        ),
        (
            "member_chain",
            "function makeProps(a: any) { return a?.b.c }",
        ),
        (
            "terminal_call",
            "function makeProps(a: any) { return a?.() }",
        ),
        (
            "member_terminal_call",
            "function makeProps(a: any) { return a.b?.() }",
        ),
        (
            "terminal_call_binary_argument",
            "function makeProps(a: any) { return a?.b(1 + 2) }",
        ),
        (
            "terminal_call_object_argument",
            "function makeProps(a: any) { return a?.b({}) }",
        ),
        (
            "terminal_call_asserted_argument",
            "type T = string; function makeProps(a: any, x: unknown) { return a?.b(x as T) }",
        ),
        (
            "terminal_call_template_argument",
            "function makeProps(a: any, x: string) { return a?.b(`${x}`) }",
        ),
    ];
    for (id, script) in cases {
        let trace = run(id, script, "makeProps");
        record_trace(id, &trace);
        assert_complete_warm(&trace, Some(r#"{"kind":"primitive","name":"any"}"#));
    }
}

#[test]
fn conditional_impossible_assertion_preserves_enclosing_return_suffix() {
    let trace = run(
        "conditional_impossible_assertion_enclosing_suffix",
        "function assertNum(x: string): asserts x is number {}\nfunction makeProps(x: string, c: boolean) { if (c) { assertNum(x) } return \"live\" as const }",
        "makeProps",
    );
    record_trace("conditional_impossible_assertion_enclosing_suffix", &trace);
    assert_complete_warm(
        &trace,
        Some(r#"{"kind":"literal","literalKind":"string","value":"live"}"#),
    );
}

#[test]
fn impossible_assertion_in_nested_block_preserves_exact_subject_ancestor_return() {
    let trace = run(
        "nested_impossible_assertion_exact_subject_ancestor_suffix",
        "function assertNum(x: string): asserts x is number {}\nfunction makeProps(x: string, c: boolean) { if (c) return \"a\" as const; { assertNum(x) } return x }",
        "makeProps",
    );
    record_trace(
        "nested_impossible_assertion_exact_subject_ancestor_suffix",
        &trace,
    );
    assert_complete_warm(
        &trace,
        Some(r#"{"kind":"literal","literalKind":"string","value":"a"}"#),
    );
}

#[test]
fn optional_any_refuses_type_changing_or_effectful_interposed_nodes() {
    for (id, script) in [
        (
            "as_interposed",
            "type Foo = { b: number }; function makeProps(x: any) { return (x as Foo)?.b }",
        ),
        (
            "satisfies_interposed",
            "function makeProps(x: any) { return (x satisfies any)?.b }",
        ),
        (
            "non_null_interposed",
            "function makeProps(x: any) { return x!?.b }",
        ),
        (
            "instantiation_interposed",
            "function makeProps(x: any) { return x<string>?.b }",
        ),
        (
            "call_interposed",
            "function makeProps(x: any) { return x?.().b }",
        ),
        (
            "effectful_call_argument",
            "function makeProps(a: any, x: string | number) { return a?.b(x = \"s\") }",
        ),
        (
            "effectful_computed_key",
            "function makeProps(a: any, x: string | number) { return a?.[x = 2] }",
        ),
    ] {
        let trace = run(id, script, "makeProps");
        record_trace(id, &trace);
        assert_refused(&trace);
    }
}

#[test]
fn optional_any_refuses_effect_in_nested_template_interpolation() {
    let trace = run(
        "effect_in_nested_template_interpolation",
        "function makeProps(a: any, x: number) { return a?.b(`${x = 1}`) }",
        "makeProps",
    );
    record_trace("effect_in_nested_template_interpolation", &trace);
    assert_refused(&trace);
}

#[test]
fn optional_any_refuses_effect_in_nested_object_property() {
    let trace = run(
        "effect_in_nested_object_property",
        "function makeProps(a: any, x: number) { return a?.b({ k: x++ }) }",
        "makeProps",
    );
    record_trace("effect_in_nested_object_property", &trace);
    assert_refused(&trace);
}

#[test]
fn optional_any_refuses_effect_in_nested_array_element() {
    let trace = run(
        "effect_in_nested_array_element",
        "async function makeProps(a: any, p: Promise<unknown>) { return a?.b([await p]) }",
        "makeProps",
    );
    record_trace("effect_in_nested_array_element", &trace);
    assert_refused(&trace);
}

#[test]
fn optional_any_refuses_effect_in_nested_call_argument() {
    let trace = run(
        "effect_in_nested_call_argument",
        "function makeProps(a: any, x: number, g: (value: number) => number) { return a?.b(g(x = 1)) }",
        "makeProps",
    );
    record_trace("effect_in_nested_call_argument", &trace);
    assert_refused(&trace);
}

#[test]
fn angle_bracket_assertion_matches_as_assertion_lowering() {
    let trace = run(
        "angle_assertion",
        "type Foo = { b: number }; function makeProps(x: unknown) { return <Foo>x }",
        "makeProps",
    );
    record_trace("angle_assertion", &trace);
    assert_complete_warm(
        &trace,
        Some(r#"{"kind":"ref","name":"Foo","typeArguments":[]}"#),
    );
}

#[test]
fn checker_correct_unannotated_same_closure_write_remains_complete_and_warm() {
    let trace = run(
        "unannotated_same_closure_write",
        "function makeProps() { let x = \"a\"; return () => { x = \"b\"; return x } }",
        "makeProps",
    );
    record_trace("unannotated_same_closure_write", &trace);
    assert_complete_warm(
        &trace,
        Some(
            r#"{"kind":"function","parameters":[],"returnType":{"kind":"primitive","name":"string"}}"#,
        ),
    );
}

#[test]
fn flow_gap_default_parameter_budget_failure_is_no_value_and_cold() {
    let mut default = "0".to_owned();
    for depth in 0..65 {
        default = if depth % 2 == 0 {
            format!("[{default}]")
        } else {
            format!("{{ value: {default} }}")
        };
    }
    let trace = run(
        "default_budget",
        &format!("function makeProps(value = {default}) {{ return value }}"),
        "makeProps",
    );
    record_trace("default_budget", &trace);
    let expected = Some(FlowReturnError::Failure(FlowReturnFailure::Budget(
        verter_type_expr::facts::InferenceUnavailableReason::DepthBudgetExceeded,
    )));
    for sample in [&trace.first, &trace.second] {
        assert_eq!(sample.error, expected, "{trace:#?}");
        assert!(
            !sample.from_cache,
            "budget failure must stay cold: {trace:#?}"
        );
        assert!(
            sample.cold_computes >= 1,
            "budget failure must recompute: {trace:#?}"
        );
        assert_eq!(sample.candidates, 0, "budget failure admits no candidate");
    }
}

#[test]
fn flow_gap_partial_propagates_through_consumer_and_scc_gates() {
    let root = run(
        "root_gate",
        "function makeProps() { return (0, () => \"a\" as const) }",
        "makeProps",
    );
    record_trace("root_gate", &root);
    assert_partial(&root, FlowGap::UnmodeledExpression);

    let outer = run(
        "consumer_gate",
        "function inner() { return (0, () => \"a\" as const) } function outer() { return inner() }",
        "outer",
    );
    record_trace("consumer_gate", &outer);
    assert_partial(&outer, FlowGap::UnmodeledExpression);

    let host = make_audit_host();
    let canonical = "/flow-gap-retraction/scc_gate.ts";
    upsert(
        &host,
        canonical,
        &super::module_script(
            "function left(flag: boolean) { if (flag) return right(flag); return (0, () => \"a\" as const) } function right(flag: boolean) { return left(flag) }",
        ),
        FileLanguage::script_ts(),
    );
    for function in ["left", "right"] {
        let trace = run_on(&host, canonical, function);
        record_trace(&format!("scc_gate_{function}"), &trace);
        assert_partial(&trace, FlowGap::UnmodeledExpression);
    }
}

#[test]
fn flow_gap_false_refusal_controls_remain_complete_and_warm() {
    let controls = [
        (
            "impossible_typeof_exact_subject_read",
            "function makeProps(x: string) { if (typeof x === \"number\") return x; return \"live\" as const }",
            Some(r#"{"kind":"literal","literalKind":"string","value":"live"}"#),
        ),
        // A guard edge no arm survives stays ALIVE with its subject read
        // as `never` — a contributor on that edge that reads a DIFFERENT
        // value keeps its own type (measured: the checker counts the
        // `"dead"` return of the uninhabited `typeof` edge, and the dead
        // predicate arm's object value, in the joined return type).
        (
            "impossible_typeof_non_subject_read",
            "function makeProps(x: string) { if (typeof x === \"number\") return \"dead\" as const; return \"live\" as const }",
            Some(r#"{"kind":"union","types":[{"kind":"literal","literalKind":"string","value":"dead"},{"kind":"literal","literalKind":"string","value":"live"}]}"#),
        ),
        (
            "impossible_predicate_non_subject_value",
            "type A = { kind: \"a\" }; type B = { kind: \"b\" }\nfunction isA(x: A | B): x is A { return x.kind === \"a\" }\nfunction isB(x: A | B): x is B { return x.kind === \"b\" }\nfunction makeProps(x: A | B) { return { v: isA(x) ? (isB(x) ? { dead: true } : \"ok\") : \"no\" } }",
            None,
        ),
        ("n23", "function makeProps(x: string | number | boolean) { if (!((typeof x === \"string\" && typeof x === \"number\") || typeof x === \"number\" || typeof x === \"boolean\")) throw 0; return { v: x } }", None),
        ("x70", "declare function sink(cb: () => void): void\nfunction makeProps() { let x: \"a\" | \"b\" = \"a\"; do { sink(() => { x = \"b\" }) } while (false); return x }", Some(r#"{"kind":"literal","literalKind":"string","value":"a"}"#)),
        ("x87", "function makeProps() { let x: \"a\" | \"b\" = \"a\"; return () => x }", Some(r#"{"kind":"function","parameters":[],"returnType":{"kind":"literal","literalKind":"string","value":"a"}}"#)),
        ("x68", "function makeProps() { L: try { break L } finally { return \"a\" as const } }", None),
        ("x80", "function makeProps() { OUT: INNER: { try { break OUT } finally { return \"a\" as const } } }", None),
        ("n24", "type A = { kind: \"a\"; a: number }; type B = { kind: \"b\"; b: number }\nfunction isA(x: A | B): x is A { return x.kind === \"a\" }\nfunction isB(x: A | B): x is B { return x.kind === \"b\" }\nfunction makeProps(x: A | B) { return { v: isA(x) ? (isB(x) ? x : \"ok\") : \"no\" } }", None),
        ("n26", "type A = { a: number }; type B = { b: number }\nfunction isA(x: A | B): x is A { return \"a\" in x }\nfunction isB(x: A | B): x is B { return \"b\" in x }\nfunction makeProps(x: A | B) { return { v: isA(x) ? (isB(x) ? x : \"ok\") : \"no\" } }", Some(r#"{"kind":"object","properties":[{"excessOrigin":"freshOwn","key":{"kind":"string","value":"v"},"memberKind":"property","optional":false,"readonly":false,"ty":{"kind":"union","types":[{"kind":"intersection","types":[{"kind":"ref","name":"A","typeArguments":[]},{"kind":"ref","name":"B","typeArguments":[]}]},{"kind":"primitive","name":"string"}]}}]}"#)),
        ("x85", "function makeProps() { let x: \"a\" | \"b\" = \"a\"; return () => { x = \"b\"; return x } }", Some(r#"{"kind":"function","parameters":[],"returnType":{"kind":"literal","literalKind":"string","value":"b"}}"#)),
        ("x88", "function makeProps() { OUT: INNER: { try { break INNER } finally { return \"a\" as const } } return \"b\" as const }", Some(r#"{"kind":"union","types":[{"kind":"literal","literalKind":"string","value":"a"},{"kind":"literal","literalKind":"string","value":"b"}]}"#)),
        (
            "optional_member_any",
            "function makeProps(a: any) { return a?.b }",
            Some(r#"{"kind":"primitive","name":"any"}"#),
        ),
    ];
    for (id, script, json) in controls {
        let trace = run(id, script, "makeProps");
        record_trace(id, &trace);
        assert_complete_warm(&trace, json);
    }
}
