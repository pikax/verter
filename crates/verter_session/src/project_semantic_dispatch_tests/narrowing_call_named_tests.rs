//! Narrowing through calls, answered by the named demand the semantic
//! benchmark makes: `__Probe` resolved by name in a project over the
//! benchmark's own `noLib` library, projected to its wire answer.
//!
//! - `filter` over an array literal with a type-predicate callback,
//!   declared or inferred, selects the predicate overload and narrows the
//!   element type; any method of a literal receiver is read off the
//!   literal's widened type.
//! - A targetless assertion function called with a condition narrows the
//!   code after the call by that condition.
//! - `typeof x === "function"` narrows a union to its callable arm, and a
//!   call of the narrowed reference calls that arm.
//!
//! Every expected answer is TypeScript 7.0.2's print of the same program,
//! alike under the four `strictNullChecks` × `noImplicitAny` settings,
//! spelled as the wire answer displays it (`number[]` displays as
//! `Array<number>`, `{ a: 1; }` as `{ a: 1 }`).

use std::sync::Arc;

const SCENARIO: &str = "/narrow/scenario.ts";

/// The display of `__Probe`'s wire answer in a fresh project holding
/// `source` beside the benchmark library, or why there is none.
fn named_answer(source: &str) -> String {
    let config = r#"{ "compilerOptions": { "strict": true, "noLib": true }, "files": ["lib.bench.d.ts", "scenario.ts"] }"#;
    let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
        crate::HostConfig::default(),
        &[("/narrow", config)],
    );
    let library = "/narrow/lib.bench.d.ts";
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(library.into()),
            input_id: library.into(),
            source: Arc::from(include_str!(
                "../../../../scripts/benchmark/semantic-perf/lib/bench-globals.d.ts"
            )),
            file_language: crate::LanguageRegistry::global()
                .classify_static(library)
                .static_resolution(),
            aliases: vec![],
        })
        .expect("benchmark library admitted");
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some(SCENARIO.into()),
            input_id: SCENARIO.into(),
            source: Arc::from(format!("type __BenchInit = 0;\n{source}\nexport {{}};\n")),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .expect("fixture admitted");
    let (outcome, _) = host
        .resolve_named_symbol_with_audit(SCENARIO, "__Probe", None)
        .into_parts();
    let node = match outcome {
        Ok(Some(node)) => node,
        other => return format!("no answer: {other:?}"),
    };
    let Some(wire) = host.project_node_to_type_expr_json_bytes(&node) else {
        return "no wire answer".into();
    };
    let value: serde_json::Value = serde_json::from_slice(&wire).expect("the wire answer is JSON");
    let answer = verter_type_expr::type_expr_from_json(&value).expect("a wire TypeExpr");
    verter_type_expr::render_type_expr_display(&answer)
        .expect("the answer renders")
        .text
        .to_string()
}

/// Each `(program, answer)` row whose named answer differs.
fn misses(rows: &[(&str, &str)]) -> Vec<String> {
    rows.iter()
        .filter_map(|(program, expected)| {
            let answer = named_answer(program);
            (answer != *expected)
                .then(|| format!("{program}\n  expected `{expected}`, got `{answer}`"))
        })
        .collect()
}

/// The benchmark's `core-filter-type-predicate` program: `isNum` declares
/// no predicate, the checker infers `x is number` from its body, and
/// `filter<S extends T>(predicate: (value: T, …) => value is S): S[]` over
/// the literal's `(string | number)[]` narrows the elements to `number`.
const FILTER_INFERRED_PREDICATE: &str = r#"type __Flat<T> = [T] extends [infer U] ? U : never;
type __D<T> = T extends unknown ? T : never;
type __Obj<T> = { [K in keyof T]: T[K] };
const isNum = (x: unknown) => typeof x === "number";
const arr = [1, "a", 2].filter(isNum);
type __Probe = typeof arr;"#;

#[test]
fn filter_over_an_array_literal_narrows_by_the_inferred_predicate() {
    let failures = misses(&[(FILTER_INFERRED_PREDICATE, "Array<number>")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A declared predicate selects the same overload, inline or named; a
/// callback with no predicate takes the plain overload, which keeps the
/// literal's widened element type; other methods of literal receivers
/// read off the literal's type as well.
#[test]
fn methods_of_literal_receivers_read_off_the_widened_literal() {
    let failures = misses(&[
        (
            "declare function isNum(x: unknown): x is number;\nconst arr = [1, \"a\", 2].filter(isNum);\ntype __Probe = typeof arr;",
            "Array<number>",
        ),
        (
            "const arr = [1, \"a\", 2].filter((x): x is number => typeof x === \"number\");\ntype __Probe = typeof arr;",
            "Array<number>",
        ),
        (
            "function g() { return [1, \"a\", 2].filter((x): x is number => typeof x === \"number\"); }\ntype __Probe = ReturnType<typeof g>;",
            "Array<number>",
        ),
        (
            "const arr = [1, \"a\", 2].filter((x) => true);\ntype __Probe = typeof arr;",
            "Array<number | string>",
        ),
        (
            "const r = [1, \"a\", 2].indexOf(1);\ntype __Probe = typeof r;",
            "number",
        ),
        (
            "const r = \"ab\".concat(\"c\");\ntype __Probe = typeof r;",
            "string",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The benchmark's `core-assertion-functions` program: `assert(y)` narrows
/// the reference `y` by truthiness, and `assert(typeof z === 'string')`
/// narrows `z` by the condition it passes.
const ASSERTED_CONDITION: &str = r#"type __Flat<T> = [T] extends [infer U] ? U : never;
type __D<T> = T extends unknown ? T : never;
type __Obj<T> = { [K in keyof T]: T[K] };
declare function assert(c: unknown): asserts c;
function g(y: { a: 1 } | undefined, z: string | number) { assert(y); assert(typeof z === 'string'); return [y, z] as const; }
type __Probe = ReturnType<typeof g>;"#;

#[test]
fn an_asserted_condition_narrows_the_code_after_the_call() {
    let failures = misses(&[(ASSERTED_CONDITION, "readonly [{ a: 1 }, string]")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The condition narrows as an `if` over it would: through a predicate
/// call, a negation and a disjunction; a condition naming no reference
/// narrows nothing; an assertion inside one branch narrows that branch
/// only, so the join keeps the other's type.
#[test]
fn an_asserted_condition_narrows_as_its_test_would() {
    let assertion = "declare function assert(c: unknown): asserts c;\ndeclare function isStr(c: unknown): c is string;\n";
    let rows = [
        (
            "function g(z: string | number) { assert(isStr(z)); return z; }",
            "string",
        ),
        (
            "function g(z: string | number) { assert(!(typeof z === \"string\")); return z; }",
            "number",
        ),
        (
            "function g(z: string | number | boolean) { assert(typeof z === \"string\" || typeof z === \"number\"); return z; }",
            "number | string",
        ),
        (
            "function g(z: string | number) { assert(1 > 0); return z; }",
            "number | string",
        ),
        (
            "function g(z: string | number) { if (z) { assert(typeof z === \"string\"); } return z; }",
            "number | string",
        ),
    ];
    let programs: Vec<(String, &str)> = rows
        .iter()
        .map(|(body, expected)| {
            (
                format!("{assertion}{body}\ntype __Probe = ReturnType<typeof g>;"),
                *expected,
            )
        })
        .collect();
    let rows: Vec<(&str, &str)> = programs
        .iter()
        .map(|(program, expected)| (program.as_str(), *expected))
        .collect();
    let failures = misses(&rows);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The benchmark's `core-typeof-function-narrow` program: the guard
/// narrows `x` to `() => 1`, whose call returns `1`, and the fall-through
/// keeps `string`.
const TYPEOF_FUNCTION_CALL: &str = r#"type __Flat<T> = [T] extends [infer U] ? U : never;
type __D<T> = T extends unknown ? T : never;
type __Obj<T> = { [K in keyof T]: T[K] };
function f(x: string | (() => 1)) { if (typeof x === "function") return x(); return x; }
type __Probe = ReturnType<typeof f>;"#;

#[test]
fn a_call_of_a_typeof_function_narrowed_union_calls_its_callable_arm() {
    let failures = misses(&[
        (TYPEOF_FUNCTION_CALL, "string | 1"),
        (
            "function f(x: 0 | (() => 1)) { if (typeof x !== \"function\") return x; return x(); }\ntype __Probe = ReturnType<typeof f>;",
            "0 | 1",
        ),
        (
            "function f(x: (() => 1) | (() => \"a\") | 2) { if (typeof x === \"function\") return x(); return 0; }\ntype __Probe = ReturnType<typeof f>;",
            "'a' | 0 | 1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
