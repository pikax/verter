//! A mapped type declared over `keyof T` for a type parameter `T` — the
//! homomorphic utilities `Partial`, `Required` and `Readonly`, and any
//! alias of the same form — instantiates the way TypeScript's
//! `instantiateMappedType` does: a union maps each constituent, a primitive
//! passes through, and an array or tuple maps element-wise with the
//! modifiers applied to its elements. A mapping written over a concrete
//! type maps that type's own keys.
//!
//! Every expected answer below is TypeScript 7.0.2's, measured on this
//! exact fixture with `tsc --ignoreConfig --noEmit` (strict by default),
//! then with `--strictNullChecks false` and with
//! `--exactOptionalPropertyTypes`: `declare const v: <probe>; export const
//! s: null = v;` read off the TS2322 message.

use super::checker_probe_lane_tests::{
    evaluated_mismatches, mismatches, mismatches_in, ProbeProject,
};

const MAPPED: &str = "\
interface Face { a: 1 }
type Obj = { o: 1 };
type U = Face | Obj;
type Mp<T> = { [K in keyof T]: T[K] };
type MpS<T> = { [K in keyof T]: string };
type MpR<T> = { [K in keyof T as `x${K & string}`]: T[K] };
type P<T> = Partial<T>;
type Tup = [1, 'two', 3?];
type RoT = readonly [1, 2];
declare function g<T>(x: T): { [K in keyof T]: T[K] };
export function callG() { return g('s' as string); }
export function callGA() { return g([1] as number[]); }
declare function gp<T>(x: T): { [K in keyof T]?: T[K] };
export function callGP() { return gp([1] as number[]); }
function h<T>(x: T) { const y: { [K in keyof T]: T[K] } = null!; return y; }
export function callH() { return h('s' as string); }
";

fn mapped_tags_source(count: usize) -> String {
    let mut source = String::from("interface Big {\n");
    for i in 0..count {
        let value = match i % 3 {
            0 => i.to_string(),
            1 => format!("\"s{i}\""),
            _ => "boolean".to_string(),
        };
        source.push_str(&format!("k{i}: {value};\n"));
    }
    source.push_str("}\n");
    source
}

#[test]
fn large_mapped_index_reduces_each_selected_value() {
    // Exercise the member-index path and discriminate quadratic key scans
    // without repeating the benchmark's full source-to-answer populations.
    // The production benchmark retains the 1,000/3,000/10,000-key cases.
    for count in [64, 128, 256] {
        let source = mapped_tags_source(count);
        let before = verter_type_engine::semantic_query::key_scan_comparisons_for_tests();
        let failures = mismatches(
            &source,
            &[(
                "{ [K in keyof Big]: Big[K] extends number ? 'n' : Big[K] extends string ? 's' : 'b' }[keyof Big]",
                "\"b\" | \"n\" | \"s\"",
            )],
        );
        assert!(failures.is_empty(), "{count} keys: {}", failures.join("\n"));
        let comparisons =
            verter_type_engine::semantic_query::key_scan_comparisons_for_tests() - before;
        println!("{count} keys: {comparisons} member comparisons");
        assert!(
            comparisons <= 64 * count,
            "{count} keys required {comparisons} member comparisons"
        );
    }
}

#[test]
fn wide_source_index_preserves_accessors_overloads_and_key_kinds() {
    let mut source = String::from("declare const marker: unique symbol; declare class Wide {\n");
    for i in 0..20 {
        source.push_str(&format!("padding{i}: {i};\n"));
    }
    source.push_str(
        "1: 'numeric'; [marker]: 'symbol'; optional?: 'present';\n\
         get value(): 'getter'; set value(v: string);\n\
         method(x: 'a'): 'first'; method(x: 'b'): 'last'; }\n",
    );
    let failures = mismatches(
        &source,
        &[
            ("Wide[1]", "\"numeric\""),
            ("Wide['1']", "\"numeric\""),
            ("Wide[typeof marker]", "\"symbol\""),
            ("Wide['optional']", "\"present\" | undefined"),
            ("Wide['value']", "\"getter\""),
            ("ReturnType<Wide['method']>", "\"last\""),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn mapped_index_reads_values_in_the_selected_key_domain() {
    let source = "interface Src { readonly a: 1; b?: 'two'; c: boolean }\n\
        type Values<T> = { [K in keyof T]: T[K] }[keyof T];\n\
        type Tags<T> = { [K in keyof T]: T[K] extends number ? 'n' : 's' }[keyof T];\n\
        declare const marker: unique symbol;\n\
        interface Mixed { text: 1; 0: 2; [marker]: 3 }";
    let failures = mismatches(
        source,
        &[
            (
                "{ [K in keyof Src]: K }[keyof Src]",
                "\"a\" | \"b\" | \"c\" | undefined",
            ),
            ("{ [K in keyof Src]: K }['a' | 'c']", "\"a\" | \"c\""),
            (
                "{ [K in keyof Src & string]: K }[keyof Src & string]",
                "\"a\" | \"b\" | \"c\"",
            ),
            ("Values<Src>", "1 | \"two\" | boolean | undefined"),
            ("Tags<{ a: 1; b: 'two' }>", "\"n\" | \"s\""),
            (
                "{ [K in keyof Mixed & string]: Mixed[K] }[keyof Mixed & string]",
                "1",
            ),
            (
                "{ [K in keyof Mixed & number]: Mixed[K] }[keyof Mixed & number]",
                "2",
            ),
            (
                "{ [K in keyof Mixed & symbol]: Mixed[K] }[keyof Mixed & symbol]",
                "3",
            ),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn removing_object_optionality_preserves_authored_undefined_in_exact_mode() {
    let exact = ProbeProject {
        compiler_options: Some(r#"{ "strict": true, "exactOptionalPropertyTypes": true }"#),
        ..ProbeProject::default()
    };
    const SOURCE: &str = "type Req<T> = { [K in keyof T]-?: T[K] };";
    for (project, optional_value) in [(ProbeProject::default(), "1"), (exact, "1 | undefined")] {
        let failures = mismatches_in(
            project,
            SOURCE,
            &[
                ("Required<{ a?: 1 | undefined }>['a']", optional_value),
                ("Req<{ a?: 1 | undefined }>['a']", optional_value),
                ("Required<{ a: 1 | undefined }>['a']", "1 | undefined"),
                ("Req<{ a: 1 | undefined }>['a']", "1 | undefined"),
                ("Required<{ a?: 1 }>['a']", "1"),
            ],
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

/// A primitive passes through, with or without an `as` clause, and `never`
/// is `never`.
///
/// Measured on TypeScript 7.0.2: `Partial<string>`, `Mp<string>`,
/// `P<string>` and `MpR<string>` are `string`; `Partial<undefined>` is
/// `undefined`, `Partial<symbol>` `symbol`, `Partial<bigint>` `bigint`,
/// `Partial<object>` `object` and `Partial<1>` `1`; `Partial<null>`,
/// `Partial<never>` and `Mp<never>` are assignable to `null` (they are
/// `null` and `never`); `Partial<boolean>` prints as the application and
/// is mutually assignable with `boolean`.
#[test]
fn a_primitive_passes_through() {
    let failures = mismatches(
        MAPPED,
        &[
            ("Partial<string>", "string"),
            ("Mp<string>", "string"),
            ("P<string>", "string"),
            ("MpR<string>", "string"),
            ("Partial<undefined>", "undefined"),
            ("Partial<null>", "null"),
            ("Partial<symbol>", "symbol"),
            ("Partial<bigint>", "bigint"),
            ("Partial<object>", "object"),
            ("Partial<1>", "1"),
            ("Partial<boolean>", "boolean"),
            ("Partial<never>", "never"),
            ("Mp<never>", "never"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A mapping written inline in a generic signature, or in the body of a
/// generic function, instantiates the same way at a call.
///
/// Measured on TypeScript 7.0.2 over `declare function g<T>(x: T): { [K in
/// keyof T]: T[K] }` and its `?` twin `gp`: `g('s' as string)` returns
/// `string`, `g([1] as number[])` `number[]`, and `gp([1] as number[])`
/// `(number | undefined)[]`; over `function h<T>(x: T) { const y: { [K in
/// keyof T]: T[K] } = null!; return y; }`, `h('s' as string)` returns
/// `string`.
#[test]
fn a_mapping_in_a_generic_signature_instantiates_at_a_call() {
    let failures = mismatches(
        MAPPED,
        &[
            ("ReturnType<typeof callG>", "string"),
            ("ReturnType<typeof callGA>", "number[]"),
            ("ReturnType<typeof callGP>", "(number | undefined)[]"),
            ("ReturnType<typeof callH>", "string"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// A union maps each constituent: `Partial<Face | Obj>` is `Partial<Face> |
/// Partial<Obj>`, not the mapping of the constituents' common keys. The
/// checker prints the application by name, so the comparison reads the
/// value the application evaluates to, against a type the checker holds
/// mutually assignable with it; `unknown` maps to `{}`.
///
/// Measured on TypeScript 7.0.2: each `X extends Y` and `Y extends X` is
/// `"y"` for `Partial<Face | Obj>`, `Partial<U>` and `P<Face | Obj>`
/// against `{ a?: 1 } | { o?: 1 }`, `Mp<Face | Obj>` against `{ a: 1 } | {
/// o: 1 }`, `MpS<Face | Obj>` against `{ a: string } | { o: string }`,
/// `Partial<Face | string>` against `{ a?: 1 } | string`, `Required<{ a?: 1
/// } | { b?: 2 }>` against `{ a: 1 } | { b: 2 }`; `Readonly<Face | Obj>
/// extends { readonly a: 1 } | { readonly o: 1 }` is `"y"`; `keyof
/// Partial<Face | Obj>` and `keyof Partial<unknown>` are `never`.
#[test]
fn a_union_maps_each_constituent() {
    let failures = evaluated_mismatches(
        MAPPED,
        &[
            ("Partial<Face | Obj>", "{ a?: 1; } | { o?: 1; }"),
            ("Partial<U>", "{ a?: 1; } | { o?: 1; }"),
            ("P<Face | Obj>", "{ a?: 1; } | { o?: 1; }"),
            ("Mp<Face | Obj>", "{ a: 1; } | { o: 1; }"),
            ("MpS<Face | Obj>", "{ a: string; } | { o: string; }"),
            ("Partial<Face | string>", "{ a?: 1; } | string"),
            ("Required<{ a?: 1 } | { b?: 2 }>", "{ a: 1; } | { b: 2; }"),
            (
                "Readonly<Face | Obj>",
                "{ readonly a: 1; } | { readonly o: 1; }",
            ),
            ("Partial<unknown>", "{}"),
        ],
    );
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An array maps its element and a tuple each element, the modifiers
/// applying to the elements: `+?` makes every element optional and adds
/// `undefined` (unless `exactOptionalPropertyTypes` carries it on the
/// optional flag), `-?` removes both, `readonly` makes the array or tuple
/// readonly. With `strictNullChecks` off no `undefined` is added.
///
/// Measured on TypeScript 7.0.2, strict / `--strictNullChecks false` /
/// `--exactOptionalPropertyTypes`: `Partial<number[]>[number]` is `number |
/// undefined` / `number` / `number | undefined`;
/// `Required<(number | undefined)[]>[number]` is `number` throughout;
/// `MpS<number[]>[number]` is `string`; `Partial<Tup>[0]` is `1 |
/// undefined` / `1` / `1 | undefined` and `[1]` `"two" | undefined` /
/// `"two"` / `"two" | undefined`; `Required<Tup>[2]` is `3`;
/// `Readonly<Tup>[2]` and `Mp<Tup>[2]` are `3 | undefined` / `3` / `3 |
/// undefined`; `MpS<Tup>[0]` is `string` and `MpS<Tup>[2]` `string |
/// undefined` / `string` / `string | undefined`; throughout,
/// `Readonly<number[]> extends number[]` is `"n"` and the converse `"y"`,
/// `Partial<number[]>` and `(number | undefined)[]` are mutually
/// assignable, `Readonly<Tup> extends Tup` is `"n"` and the converse
/// `"y"`, `Partial<RoT> extends readonly [1?, 2?]` is `"y"` while
/// `Partial<RoT> extends [1?, 2?]` is `"n"`, `[] extends Partial<Tup>` is
/// `"y"` while `[] extends Tup` is `"n"`, and `Required<[1?]> extends [1]`
/// is `"y"`.
#[test]
fn an_array_or_tuple_maps_element_wise() {
    let strict = ProbeProject::default();
    let loose = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "strictNullChecks": false }"#),
        ambient_lib: None,
    };
    let exact = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "exactOptionalPropertyTypes": true }"#),
        ambient_lib: None,
    };
    for (project, strict_null_checks) in [(strict, true), (loose, false), (exact, true)] {
        let read = |on: &'static str, off: &'static str| if strict_null_checks { on } else { off };
        let failures = mismatches_in(
            project,
            MAPPED,
            &[
                (
                    "Partial<number[]>[number]",
                    read("number | undefined", "number"),
                ),
                ("Required<(number | undefined)[]>[number]", "number"),
                ("MpS<number[]>[number]", "string"),
                ("Partial<Tup>[0]", read("1 | undefined", "1")),
                ("Partial<Tup>[1]", read("\"two\" | undefined", "\"two\"")),
                ("Required<Tup>[2]", "3"),
                ("Readonly<Tup>[2]", read("3 | undefined", "3")),
                ("Mp<Tup>[2]", read("3 | undefined", "3")),
                ("MpS<Tup>[0]", "string"),
                ("MpS<Tup>[2]", read("string | undefined", "string")),
                ("Readonly<number[]> extends number[] ? 'y' : 'n'", "\"n\""),
                ("number[] extends Readonly<number[]> ? 'y' : 'n'", "\"y\""),
                (
                    "Partial<number[]> extends (number | undefined)[] ? 'y' : 'n'",
                    "\"y\"",
                ),
                (
                    "(number | undefined)[] extends Partial<number[]> ? 'y' : 'n'",
                    "\"y\"",
                ),
                ("Readonly<Tup> extends Tup ? 'y' : 'n'", "\"n\""),
                ("Tup extends Readonly<Tup> ? 'y' : 'n'", "\"y\""),
                (
                    "Partial<RoT> extends readonly [1?, 2?] ? 'y' : 'n'",
                    "\"y\"",
                ),
                ("Partial<RoT> extends [1?, 2?] ? 'y' : 'n'", "\"n\""),
                ("[] extends Partial<Tup> ? 'y' : 'n'", "\"y\""),
                ("[] extends Tup ? 'y' : 'n'", "\"n\""),
                ("Required<[1?]> extends [1] ? 'y' : 'n'", "\"y\""),
            ],
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

/// An OPTIONAL tuple element's type is its written type plus `undefined`
/// under `strictNullChecks`, so the relation compares elements that way;
/// under `exactOptionalPropertyTypes` the implied `undefined` is the
/// missing type, which an explicit `undefined` does not satisfy.
///
/// Measured on TypeScript 7.0.2, strict / `--strictNullChecks false` /
/// `--exactOptionalPropertyTypes`: `[(1 | undefined)?] extends [1?]` is
/// `"y"` / `"y"` / `"n"`; `[1?] extends [(1 | undefined)?]` is `"y"`
/// throughout; `[1 | undefined] extends [1?]` and `[undefined] extends
/// [1?]` are `"y"` / `"y"` / `"n"`; `[1?] extends [1]` is `"n"` throughout.
#[test]
fn an_optional_tuple_element_relates_with_its_implied_undefined() {
    let strict = ProbeProject::default();
    let loose = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "strictNullChecks": false }"#),
        ambient_lib: None,
    };
    let exact = ProbeProject {
        files: &[],
        compiler_options: Some(r#"{ "strict": true, "exactOptionalPropertyTypes": true }"#),
        ambient_lib: None,
    };
    for (project, exact_optional) in [(strict, false), (loose, false), (exact, true)] {
        let read =
            |plain: &'static str, exact: &'static str| if exact_optional { exact } else { plain };
        let failures = mismatches_in(
            project,
            MAPPED,
            &[
                (
                    "[(1 | undefined)?] extends [1?] ? 'y' : 'n'",
                    read("\"y\"", "\"n\""),
                ),
                ("[1?] extends [(1 | undefined)?] ? 'y' : 'n'", "\"y\""),
                (
                    "[1 | undefined] extends [1?] ? 'y' : 'n'",
                    read("\"y\"", "\"n\""),
                ),
                (
                    "[undefined] extends [1?] ? 'y' : 'n'",
                    read("\"y\"", "\"n\""),
                ),
                ("[1?] extends [1] ? 'y' : 'n'", "\"n\""),
            ],
        );
        assert!(failures.is_empty(), "{}", failures.join("\n"));
    }
}

#[test]
fn named_mapped_index_completes_with_and_without_audit_capture() {
    use std::collections::BTreeSet;
    use std::sync::Arc;
    use verter_type_engine::project_semantic_dispatch::raise::{
        enable_dispatch_trace_for_test, DISPATCH_TRACE,
    };
    use verter_type_expr::{LiteralValue, TypeExpr};
    for audit_enabled in [false, true] {
        let mut previous = None;
        for count in [64, 128, 256] {
            let host = crate::VerterHost::new_standalone_with_tsconfig_projects(
                crate::HostConfig {
                    audit_enabled,
                    ..crate::HostConfig::default()
                },
                &[("/mapped", r#"{ "compilerOptions": { "strict": true } }"#)],
            );
            let source = mapped_tags_source(count) + "type __Probe = { [K in keyof Big]: Big[K] extends number ? 'n' : Big[K] extends string ? 's' : 'b' }[keyof Big]; export {};";
            let _ = host
                .upsert(crate::UpsertRequest {
                    canonical_id: Some("/mapped/scenario.ts".into()),
                    input_id: "/mapped/scenario.ts".into(),
                    source: Arc::from(source),
                    file_language: crate::FileLanguage::script_ts(),
                    aliases: vec![],
                })
                .expect("fixture admitted");
            let _trace = enable_dispatch_trace_for_test();
            let ((outcome, _), usage) = verter_type_engine::project_semantic_dispatch::connected_demand::with_connected_work_capture_for_tests(|| {
                host.resolve_named_symbol_with_audit("/mapped/scenario.ts", "__Probe", None).into_parts()
            });
            let node = outcome
                .expect("named demand must complete")
                .expect("probe exists");
            let bytes = host
                .project_node_to_type_expr_json_bytes(&node)
                .expect("answer projects");
            let answer: TypeExpr = serde_json::from_slice(&bytes).expect("valid answer");
            let TypeExpr::Union(arms) = &answer else {
                panic!("expected three-literal union, got {answer:?}")
            };
            let values = arms
                .iter()
                .map(|arm| match arm {
                    TypeExpr::Literal(LiteralValue::String(value)) => value.as_str(),
                    other => panic!("unexpected union arm {other:?}"),
                })
                .collect::<BTreeSet<_>>();
            assert_eq!(values, BTreeSet::from(["b", "n", "s"]));
            let stats = host.project_type_store().semantic_graph().stats_snapshot();
            let dispatches = DISPATCH_TRACE.with(|trace| trace.borrow().len()) as u64;
            let work = usage.work as u64
                + stats.evaluate_deferred_memo_misses
                + stats.substitute_memo_misses
                + dispatches;
            println!("audit={audit_enabled} {count} keys: evaluation={} substitution={} dispatch={dispatches} connected={}", stats.evaluate_deferred_memo_misses, stats.substitute_memo_misses, usage.work);
            if let Some(previous) = previous {
                assert!(
                    work <= previous * 3,
                    "doubling keys must not quadruple total evaluation/substitution/dispatch work"
                );
            }
            previous = Some(work);
        }
    }
}

#[test]
fn selected_mapped_class_members_keep_the_source_receiver() {
    assert_navigate_mapped_value(
        "class Base { me(): this { return this; } } class Sub extends Base { value = 1; }",
        "{ [K in keyof Sub]: Sub[K] extends (...args: any[]) => infer R ? R : never }[keyof Sub]",
        "Sub",
    );
}

#[test]
fn selected_mapped_values_keep_recursive_generic_carriers() {
    assert_navigate_mapped_value(
        "interface A<T> { peer: B<T> } interface B<T> { peer: A<T> }",
        "{ [K in 'x' | 'y']: A<K> }['x' | 'y']",
        "A<\"x\"> | A<\"y\">",
    );
}

fn assert_navigate_mapped_value(source: &str, probe: &str, expected: &str) {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::checker_syntax;
    use std::sync::Arc;
    use verter_type_engine::semantic_query::ProjectionMode;
    let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some("/mapped-value.ts".into()),
            input_id: "/mapped-value.ts".into(),
            source: Arc::from(format!("{source}\ntype Probe = {probe}; export {{}};")),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .expect("fixture admitted");
    // Navigate promises a selected value with named declarations intact.
    // Expanded publication deliberately materializes the terminal surface.
    let (answer, _) = host
        .resolve_named_symbol_with_audit(
            "/mapped-value.ts",
            "Probe",
            Some(ProjectionMode::Navigate),
        )
        .into_parts();
    let node = answer
        .expect("selected demand completes")
        .expect("probe exists");
    let dispatch = super::ProjectSemanticDispatch::new(&host);
    let expected = checker_syntax::parse(expected).expect("checker answer parses");
    assert!(
        checker_syntax::matches_node(&dispatch, node.id(), &expected, 0),
        "unexpected selected value: {:?}",
        host.project_type_store()
            .semantic_graph()
            .node_data(node.id())
    );
}

/// Resolve `Probe` through the public named-symbol request: `None` when the
/// demand does not complete, else whether the answer matches `expected`.
fn public_probe(source: &str, probe: &str, expected: &str) -> Option<bool> {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::checker_syntax;
    use std::sync::Arc;
    let host = crate::VerterHost::new_standalone(crate::HostConfig::default());
    let _ = host
        .upsert(crate::UpsertRequest {
            canonical_id: Some("/getters.ts".into()),
            input_id: "/getters.ts".into(),
            source: Arc::from(format!("{source}\ntype Probe = {probe}; export {{}};")),
            file_language: crate::FileLanguage::script_ts(),
            aliases: vec![],
        })
        .expect("fixture admitted");
    let (answer, _) = host
        .resolve_named_symbol_with_audit("/getters.ts", "Probe", None)
        .into_parts();
    let node = answer.ok()?.expect("probe exists");
    let dispatch = super::ProjectSemanticDispatch::new(&host);
    let expected = checker_syntax::parse(expected).expect("checker answer parses");
    Some(checker_syntax::matches_node(
        &dispatch,
        node.id(),
        &expected,
        0,
    ))
}

/// A getter whose return inference this engine refuses (a closure assigns
/// the returned local), so any read of `S['a']` is observable by its outcome.
const REFUSED_GETTER: &str =
    "class S { get a() { let x = 0; (() => { x = 1; })(); return x; } b = 1 as const; }";

#[test]
fn selected_mapped_conditional_does_not_read_a_dormant_getter() {
    assert_eq!(
        public_probe(REFUSED_GETTER, "S['a']", "number"),
        None,
        "fixture precondition: a direct getter read does not complete"
    );
    // Control: the branch `K = 'a'` selects reads the getter, with the
    // same outcome as the direct read.
    assert_eq!(
        public_probe(
            REFUSED_GETTER,
            "{ [K in keyof S]: K extends 'b' ? 'ok' : S[K] }[keyof S]",
            "\"ok\" | number",
        ),
        None,
        "a selected getter read reaches the getter"
    );
    // TypeScript 7.0.2 answers `"ok" | 1`: the getter is read only by the
    // branch `K = 'a'` does not select, so it cannot taint the answer.
    assert_eq!(
        public_probe(
            REFUSED_GETTER,
            "{ [K in keyof S]: K extends 'a' ? 'ok' : S[K] }[keyof S]",
            "\"ok\" | 1",
        ),
        Some(true),
        "the winning literal branch never reads the getter"
    );
}

#[test]
fn branded_key_space_never_enumerates_its_brand_members() {
    // TypeScript 7.0.2 keeps `string & { __brand: "U" }` as the key type:
    // the object arm brands the string, it never contributes `__brand`.
    let source = "type B = string & { __brand: 'U' };";
    for (probe, forbidden) in [
        ("{ [K in B]: K }[B]", "\"__brand\""),
        ("{ [K in 'x' | B]: K }['x' | B]", "\"x\" | \"__brand\""),
    ] {
        assert_ne!(
            public_probe(source, probe, forbidden),
            Some(true),
            "{probe} must not read the brand's member names as keys"
        );
    }
}
