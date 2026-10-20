//! How a conditional type is decided: deferred where the checker defers
//! it, decided by the permissive and restrictive tests where its operands
//! hold a type parameter the checker does not defer on, and related as a
//! deferred type where it stays one.
//!
//! Every expected answer is TypeScript 7.0.2's, measured with `tsc
//! --ignoreConfig --noEmit --strict --noErrorTruncation` under all four
//! `strictNullChecks` x `noImplicitAny` settings (alike in every setting
//! here): the type a generic function's body holds is read off TS2322 for
//! `const s: never = null! as <probe>` inside that function, and a call's
//! answer off `const s: never = f()`, with no diagnostic in the calling
//! function. Rows over installed packages are measured the same way with
//! `--moduleResolution bundler --module esnext --target es2020
//! --skipLibCheck`, in a directory whose `node_modules` is the one the
//! matrix links.

use super::differential_harness_tests::{Matrix, Read, Setting, ALL};

#[test]
fn an_infer_variable_in_a_nested_conditional_check_stays_deferred() {
    let source = r#"
interface TextFactory { new (): { boxed: true }; (): string }
type PC<T> = { new (...args: any[]): T & {} } | { (): T };
type PCM<T> = PC<T> | (T extends (...args: any[]) => any
    ? { new (): any; (): T; prototype: any } : never);
type Wrapped<T> = PC<T> | ([T] extends [((...args: any[]) => any) | undefined]
    ? { new (): any; (): T; prototype: any } : never);
export function base() { return null! as (TextFactory extends PC<infer V> ? V : never); }
export function direct() { return null! as (TextFactory extends PCM<infer V> ? V : never); }
export function tuple() { return null! as (TextFactory extends Wrapped<infer V> ? V : never); }
"#;
    let failures = Matrix::new(source).settings(&ALL).returns(&[
        ("base", "string"),
        ("direct", "string"),
        ("tuple", "string"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Equivalent call-result demands over the pinned Vue definitions. The
/// expected signatures and prop types were measured with TypeScript 7.0.2's
/// native compiler with `--lib es2020` in all four strictness settings.
fn vue_call_result_fixture(emits: bool, strict: bool) {
    let mut events = String::new();
    let mut strict_emits = Vec::new();
    let mut loose_emits = Vec::new();
    for i in 0..8 {
        let (option, strict, loose) = if i % 3 == 2 {
            (
                "null".to_string(),
                format!("((event: \"e{i}\", ...args: any[]) => void)"),
                format!("((event: \"e{i}\", ...args: unknown[]) => void)"),
            )
        } else {
            let ty = if i % 3 == 0 { "number" } else { "string" };
            (
                format!("(v: {ty}, flag?: boolean) => true"),
                format!("((event: \"e{i}\", v: {ty}, flag?: boolean | undefined) => void)"),
                format!("((event: \"e{i}\", v: {ty}, flag?: boolean) => void)"),
            )
        };
        events.push_str(&format!("e{i}: {option};\n"));
        strict_emits.push(strict);
        loose_emits.push(loose);
    }
    let mut options = String::new();
    let mut required = String::new();
    let mut strict_optional = String::new();
    let mut loose_optional = String::new();
    for i in 0..16 {
        let (option, value) = match i % 8 {
            0 => ("StringConstructor".to_string(), "string".to_string()),
            1 => (
                "{ type: NumberConstructor; required: true }".to_string(),
                "number".to_string(),
            ),
            2 => (
                "{ type: PropType<string[]>; required: true }".to_string(),
                "string[]".to_string(),
            ),
            3 => ("BooleanConstructor".to_string(), "boolean".to_string()),
            4 => (
                "[NumberConstructor, StringConstructor]".to_string(),
                "string | number".to_string(),
            ),
            5 => (
                format!("{{ type: PropType<{{ x{i}: number }}>; required: true }}"),
                format!("{{ x{i}: number; }}"),
            ),
            6 => (
                "{ type: StringConstructor; default: \"fallback\" }".to_string(),
                "string".to_string(),
            ),
            _ => (
                "{ type: DateConstructor; required: true }".to_string(),
                "Date".to_string(),
            ),
        };
        options.push_str(&format!("p{i}: {option};\n"));
        if matches!(i % 8, 0 | 4) {
            strict_optional.push_str(&format!("readonly p{i}?: {value} | undefined; "));
            loose_optional.push_str(&format!("readonly p{i}?: {value}; "));
        } else {
            required.push_str(&format!("readonly p{i}: {value}; "));
        }
    }
    let source = format!(
        "import type {{ EmitFn, ExtractPropTypes, PropType }} from 'vue';\n\
         type UR<T> = T extends string ? \"s\" : T;\n\
         declare function f<T>(): {{ value: UR<T> }};\n\
         const a = f<number>();\n\
         type Events = {{ {events} }};\n\
         declare function emits<T>(): EmitFn<T>;\n\
         const e = emits<Events>();\n\
         type Options = {{ {options} }};\n\
         declare function props<T>(): ExtractPropTypes<T>;\n\
         const p = props<Options>();\n"
    );
    let strict_emits = strict_emits.join(" & ");
    let loose_emits = loose_emits.join(" & ");
    let strict_props = format!("{{ {required} {strict_optional} }}");
    let loose_props = format!("{{ {required} {loose_optional} }}");
    let settings = if strict {
        [ALL[0], ALL[2]]
    } else {
        [ALL[1], ALL[3]]
    };
    let (read, expected) = if emits {
        (
            "typeof e",
            if strict { &strict_emits } else { &loose_emits },
        )
    } else {
        (
            "{ readonly [K in keyof typeof p]: (typeof p)[K] }",
            if strict { &strict_props } else { &loose_props },
        )
    };
    let failures: Vec<String> = settings
        .iter()
        .flat_map(|setting| {
            let settings = [*setting];
            Matrix::new(&source)
                .installed_packages()
                .installed_typescript_libs(&["es2020"])
                .settings(&settings)
                .types(&[
                    (read, expected),
                    ("typeof a.value", "number"),
                    ("StringConstructor extends BooleanConstructor ? 1 : 2", "2"),
                    (
                        "[NumberConstructor, StringConstructor] extends BooleanConstructor ? 1 : 2",
                        "2",
                    ),
                    (
                        "[NumberConstructor, StringConstructor] extends { required: true } ? 1 : 2",
                        "2",
                    ),
                    (
                        "typeof Symbol.iterator extends typeof Symbol.match ? 1 : 2",
                        "2",
                    ),
                ])
        })
        .collect();
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn vue_callable_and_prop_aliases_in_call_results_emits_strict() {
    vue_call_result_fixture(true, true);
}
#[test]
fn vue_callable_and_prop_aliases_in_call_results_emits_loose() {
    vue_call_result_fixture(true, false);
}
#[test]
fn vue_callable_and_prop_aliases_in_call_results_props_strict() {
    vue_call_result_fixture(false, true);
}
#[test]
fn vue_callable_and_prop_aliases_in_call_results_props_loose() {
    vue_call_result_fixture(false, false);
}

#[test]
fn generic_signature_conditional_identity_decides_closed_arguments() {
    let source = r#"
type Exclude<T, U> = T extends U ? never : T;
type Extract<T, U> = T extends U ? T : never;
type Eq<X, Y> = (<T>() => T extends X ? 1 : 2) extends (<T>() => T extends Y ? 1 : 2) ? true : false;
type Renamed<X, Y> = (<S>() => S extends X ? 1 : 2) extends (<T>() => T extends Y ? 1 : 2) ? true : false;
type Left = { a: 1 };
type Right = { a: 1 };
type __BenchInit = 0;
export function open<X, Y>() { return null! as Eq<X, Y>; }
export function reflexive<X>() { return null! as Eq<X, X>; }
"#;
    let matrix = Matrix::new(source).settings(&ALL);
    let mut failures = matrix.types(&[
        ("Eq<{ a: 1 }, { a: 1 }>", "true"),
        ("Eq<{ a: 1 }, { a: 2 }>", "false"),
        ("Eq<{ a: 1 }, { a: number }>", "false"),
        ("Eq<any, unknown>", "false"),
        ("Eq<unknown, any>", "false"),
        ("Eq<Left, Right>", "true"),
        ("Renamed<{ a: 1 }, { a: 1 }>", "true"),
        ("Renamed<{ a: 1 }, { a: 2 }>", "false"),
    ]);
    failures.extend(matrix.returns(&[
        (
            "open",
            "(<T>() => T extends X ? 1 : 2) extends (<T>() => T extends Y ? 1 : 2) ? true : false",
        ),
        ("reflexive", "true"),
    ]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn generic_signature_identity_ignores_operand_construction_details() {
    let source = r#"
type Eq<X, Y> = (<T>() => T extends X ? 1 : 2) extends (<T>() => T extends Y ? 1 : 2) ? true : false;
type Left = { a: 1; b: 2 };
type Right = { b: 2; a: 1 };
interface Base { a: 1 }
interface Derived extends Base { b: 2 }
declare const key: unique symbol;
declare const otherKey: unique symbol;
export function reordered() { return null! as Eq<Left, Right>; }
export function nested() { return null! as Eq<{ v: Left }, { v: Right }>; }
export function union() { return null! as Eq<1 | 2, 2 | 1>; }
export function tuple() { return null! as Eq<[x: 1], [y: 1]>; }
export function signature() { return null! as Eq<(x: 1) => 2, (y: 1) => 2>; }
export function readonly() { return null! as Eq<{ readonly a: 1 }, { a: 1 }>; }
export function optional() { return null! as Eq<{ a?: 1 }, { a: 1 }>; }
export function tupleReadonly() { return null! as Eq<readonly [1], [1]>; }
export function signatureOptional() { return null! as Eq<(x?: 1) => 2, (y: 1) => 2>; }
export function generic() { return null! as Eq<<T>(x: T) => T, <U>(y: U) => U>; }
export function keySpelling() { return null! as Eq<{ 1: 2 }, { "1": 2 }>; }
export function nominal() { return null! as Eq<typeof key, typeof key>; }
export function nominalBroad() { return null! as Eq<typeof key, symbol>; }
export function nominalDifferent() { return null! as Eq<typeof key, typeof otherKey>; }
export function optionalUndefined() { return null! as Eq<{ a?: 1 }, { a?: 1 | undefined }>; }
export function inherited() { return null! as Eq<Derived, { a: 1; b: 2 }>; }
export function authoredIntersection() { return null! as Eq<{ a: 1 } & { b: 2 }, { a: 1; b: 2 }>; }
export function conditionalSignature() { return null! as Eq<<T>() => T extends 1 ? 1 : 2, <U>() => U extends 1 ? 1 : 2>; }
export function differentConditionalSignature() { return null! as Eq<<T>() => T extends 1 ? 1 : 2, <U>() => U extends 2 ? 1 : 2>; }
"#;
    let failures = Matrix::new(source).settings(&ALL).returns(&[
        ("reordered", "true"),
        ("nested", "true"),
        ("union", "true"),
        ("tuple", "true"),
        ("signature", "true"),
        ("readonly", "false"),
        ("optional", "false"),
        ("tupleReadonly", "false"),
        ("signatureOptional", "false"),
        ("generic", "true"),
        ("keySpelling", "true"),
        ("nominal", "true"),
        ("nominalBroad", "false"),
        ("nominalDifferent", "false"),
        ("optionalUndefined", "true"),
        ("inherited", "true"),
        ("authoredIntersection", "false"),
        ("conditionalSignature", "true"),
        ("differentConditionalSignature", "false"),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn named_generic_signature_identity_is_stable_when_warm() {
    use crate::{FileLanguage, HostConfig, UpsertRequest, VerterHost};
    use std::sync::Arc;
    use verter_type_expr::{LiteralValue, TypeExpr};

    for (strict_null_checks, no_implicit_any, exact_optional) in [
        (true, true, false),
        (true, false, false),
        (false, true, false),
        (false, false, false),
        (true, true, true),
    ] {
        let config = format!(
            r#"{{ "compilerOptions": {{ "strictNullChecks": {strict_null_checks}, "noImplicitAny": {no_implicit_any}, "exactOptionalPropertyTypes": {exact_optional} }} }}"#
        );
        let host = VerterHost::new_standalone_with_tsconfig_projects(
            HostConfig::default(),
            &[("/identity", &config)],
        );
        for (file, source) in [
            ("left.ts", "export type Left = { a: 1; b: 2 };"),
            ("right.ts", "export type Right = { b: 2; a: 1 };"),
            (
                "probe.ts",
                r#"
import type { Left } from './left';
import type { Right } from './right';
type Eq<X, Y> = (<T>() => T extends X ? 1 : 2) extends (<T>() => T extends Y ? 1 : 2) ? true : false;
export type ObjectOrder = Eq<{ a: 1; b: 2 }, { b: 2; a: 1 }>;
export type UnionOrder = Eq<1 | 2, 2 | 1>;
export type TupleLabels = Eq<[x: 1], [y: 1]>;
export type ParameterNames = Eq<(x: 1) => 2, (y: 1) => 2>;
export type ImportedAliases = Eq<{ v: Left }, { v: Right }>;
export type Unequal = Eq<{ a: 1 }, { a: 2 }>;
export type OptionalUndefined = Eq<{ a?: 1 }, { a?: 1 | undefined }>;
interface Base { a: 1 }
interface Derived extends Base { b: 2 }
export type Inherited = Eq<Derived, { a: 1; b: 2 }>;
export type ConditionalSignature = Eq<<T>() => T extends 1 ? 1 : 2, <U>() => U extends 1 ? 1 : 2>;
"#,
            ),
        ] {
            let canonical = format!("/identity/{file}");
            let _ = host
                .upsert(UpsertRequest {
                    canonical_id: Some(canonical.clone()),
                    input_id: canonical,
                    source: Arc::from(source),
                    file_language: FileLanguage::script_ts(),
                    aliases: vec![],
                })
                .expect("fixture admitted");
        }
        let read = |symbol: &str| -> TypeExpr {
            let node = host
                .resolve_named_symbol_with_audit("/identity/probe.ts", symbol, None)
                .into_parts()
                .0
                .expect("the named demand completes")
                .expect("the symbol exists");
            let bytes = host
                .project_node_to_type_expr_json_bytes(&node)
                .expect("the answer projects");
            serde_json::from_slice(&bytes).expect("a wire answer")
        };
        for (symbol, expected) in [
            ("ObjectOrder", true),
            ("UnionOrder", true),
            ("TupleLabels", true),
            ("ParameterNames", true),
            ("ImportedAliases", true),
            ("Unequal", false),
            ("OptionalUndefined", !exact_optional),
            ("Inherited", true),
            ("ConditionalSignature", true),
        ] {
            for _ in 0..2 {
                let answer = read(symbol);
                assert!(
                    matches!(answer, TypeExpr::Literal(LiteralValue::Boolean(value)) if value == expected),
                    "{symbol}: {answer:?}"
                );
            }
        }
        let _ = host
            .upsert(UpsertRequest {
                canonical_id: Some("/identity/right.ts".into()),
                input_id: "/identity/right.ts".into(),
                source: Arc::from("export type Right = { b: 2; a: 2 };"),
                file_language: FileLanguage::script_ts(),
                aliases: vec![],
            })
            .expect("changed dependency admitted");
        assert!(matches!(
            read("ImportedAliases"),
            TypeExpr::Literal(LiteralValue::Boolean(false))
        ));
    }
}

#[test]
fn any_infer_tuple_uses_unknown_and_preserves_open_checks() {
    let source = r#"
export function open<T>() { return null! as (T extends [infer A] ? A : 0); }
"#;
    let matrix = Matrix::new(source).settings(&ALL);
    let mut failures = matrix.types(&[
        ("any extends [infer A] ? A : 0", "unknown"),
        ("any extends [infer A] ? [A] : 0", "[unknown] | 0"),
        ("any extends [infer A extends string] ? A : 0", "string | 0"),
        ("any extends infer A ? A : 0", "any"),
        ("[1] extends [infer A] ? A : 0", "1"),
        ("unknown extends [infer A] ? A : 0", "0"),
    ]);
    failures.extend(matrix.returns(&[("open", "T extends [infer A] ? A : 0")]));
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn generic_signature_context_infers_composite_arguments() {
    let failures = Matrix::new("").settings(&ALL).types(&[
        (
            "(<T>(x: T) => void) extends (<U>(x: U[]) => void) ? 1 : 0",
            "1",
        ),
        ("(<T>(x: T) => T) extends (<U>(x: U[]) => U[]) ? 1 : 0", "1"),
        ("(<T>() => T) extends (<U>() => U[]) ? 1 : 0", "1"),
        (
            "(<T>(x: T, y: T) => void) extends (<U>(x: U, y: string) => void) ? 1 : 0",
            "0",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn any_infer_branch_union_uses_project_nullability() {
    let failures = Matrix::new("").settings(&ALL).nullness(&[
        (
            Read::Type("any extends [infer A] ? [A] : undefined"),
            "[unknown] | undefined",
            "[unknown]",
        ),
        (
            Read::Type("any extends [infer A] ? [A] : null"),
            "[unknown] | null",
            "[unknown]",
        ),
        (
            Read::Type("keyof (any extends [infer A] ? { v: A } : undefined)"),
            "never",
            "\"v\"",
        ),
        (
            Read::Type("keyof (any extends [infer A] ? { v: A } : null)"),
            "never",
            "\"v\"",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Generic functions holding each conditional; overloads choosing between a
/// parameter a deferred conditional argument may relate to and `unknown`;
/// and method overloads choosing between a deferred conditional parameter
/// and `unknown`.
const SOURCE: &str = r##"
declare function pickNum(n: number): "hit"; declare function pickNum(n: unknown): "other";
declare function pick12(n: 1 | 2): "hit"; declare function pick12(n: unknown): "other";
declare function pickStr(s: string): "hit"; declare function pickStr(s: unknown): "other";
declare function pick1(n: 1): "hit"; declare function pick1(n: unknown): "other";
export function a1<T>() { return null! as (any extends T ? 1 : 2); }
export function a2<T>() { return null! as (T extends any ? 1 : 2); }
export function a3<T>() { return null! as (any extends [T] ? 1 : 2); }
export function b1<T extends string>() { return null! as ({ v: T } extends { v: string } ? 1 : 2); }
export function b2<T extends string>() { return null! as ([T] extends [string] ? 1 : 2); }
export function b3<T>() { return null! as ({ v: T } extends { v: unknown } ? 1 : 2); }
export function b4<T>() { return null! as ({ v: T } extends { v: number } ? 1 : 2); }
export function b5<T extends string>() { return null! as ({ v: T } extends { v: number } ? 1 : 2); }
export function b6<T extends string>() { return null! as ({ v: T } extends { v: T } ? 1 : 2); }
export function b7<T>() { return null! as ({ v: T } extends {} ? 1 : 2); }
export function b9<T>() { return null! as ({ v: T[] } extends { v: string[] } ? 1 : 2); }
export function b10<T>() { return null! as ({ v: [T] } extends { v: [] } ? 1 : 2); }
export function b12<T>() { return null! as ({ v: T } extends { v: never } ? 1 : 2); }
export function i1<T extends { a: 1 }>() { return null! as ({ a: T["a"] } extends { a: number } ? 1 : 2); }
export function i2<T extends { a: string }>() { return null! as ({ a: T["a"] } extends { a: string } ? 1 : 2); }
export function i3<T extends { a: 1 }>() { return null! as (T["a"] extends number ? 1 : 2); }
export function i4<T extends { a: 1 }>() { return null! as ({ a: T["a"] } extends { a: unknown } ? 1 : 2); }
export function i5<T extends { a: 1 }>() { return null! as ({ a: T["a"] } extends { a: T["a"] } ? 1 : 2); }
export function i6<T extends { a: 1 }>() { return null! as ({ a: T["a"] } extends { a: never } ? 1 : 2); }
export function i7<T extends { a: 1 }>() { return null! as ({ a: [T["a"]] } extends { a: [] } ? 1 : 2); }
export function i8<T>() { return null! as ({ a: T[keyof T] } extends { a: string } ? 1 : 2); }
export function k1<T extends { a: 1 }>() { return null! as ({ a: T["a"] } extends { a: {} } ? 1 : 2); }
export function k2<T extends { a: { b: 1 } }>() { return null! as ({ a: T["a"]["b"] } extends { a: {} } ? 1 : 2); }
export function k3<T extends { a: 1 | 2 }>() { return null! as ({ a: T["a"] } extends { a: {} } ? 1 : 2); }
export function n1<T>() { return null! as ({ v: T; f: <U>(x: U) => T } extends { v: string; f: unknown } ? 1 : 2); }
export function n2<T>() { return null! as ({ v: 1; f: <U>(x: U) => U } extends { v: number; f: unknown } ? 1 : 2); }
export function n3<T>() { return null! as ({ v: T; f: <T>(x: T) => T } extends { v: string; f: unknown } ? 1 : 2); }
export function n4<T>() { return null! as ({ f: <T>(x: T) => T } extends { f: (x: string) => string } ? 1 : 2); }
export function n5<T>() { return null! as ({ f: <U>(x: U) => T } extends { f: (x: string) => string } ? 1 : 2); }
export function n6<T>() { return null! as ({ f: <T>(x: T) => T; v: T } extends { f: (x: string) => string; v: unknown } ? 1 : 2); }
export function n7<T>() { return null! as ({ f: <U>(x: U) => U } extends { f: (x: string) => number } ? 1 : 2); }
export function n8<T>() { return null! as ({ v: T; f: <U>(x: U) => U } extends { v: unknown; f: (x: string) => number } ? 1 : 2); }
export function c1<T>(x: T extends string ? 1 : 2) { return pickNum(x); }
export function c2<T>(x: T extends string ? 1 : 2) { return pick12(x); }
export function c3<T>(x: T extends string ? 1 : 2) { return pickStr(x); }
export function c4<T>(x: T extends string ? 1 : 2) { return pick1(x); }
export function c5<T>(x: T extends string ? "a" : "b") { return pickStr(x); }
export function c6<T extends string>(x: T extends string ? 1 : 2) { return pick1(x); }
export function c7<T extends number>(x: T extends string ? 1 : 2) { return pick1(x); }
export function c8<T extends string>(x: [T] extends [string] ? 1 : 2) { return pick1(x); }
export function c9<T extends number>(x: T extends string ? 1 : never) { return pickStr(x); }
export function e1<T>(n: 1, o: { pick(x: T extends string ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e2<T>(n: 1, o: { pick(x: T extends string ? number : number): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e3<T extends string>(n: 1, o: { pick(x: T extends string ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e4<T>(x: T extends string ? 1 : 2, o: { pick(x: T extends string ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(x); }
export function e5<T>(x: T extends string ? 1 : 2, o: { pick(x: T extends string ? number : number): "hit"; pick(x: unknown): "other" }) { return o.pick(x); }
export function e6<T>(n: 1, o: { pick(x: T extends string ? 1 : T): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e7<T>(n: never, o: { pick(x: T extends string ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e8<T>(n: 1, o: { pick(x: { v: T } extends { v: number } ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e9<T>(n: 1, o: { pick(x: { v: T } extends { v: unknown } ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e10<T>(n: 1, o: { pick(x: T extends unknown ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e11<T>(n: 2, o: { pick(x: [T] extends [T] ? 2 : 1): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e12<T extends string>(n: 2, o: { pick(x: { v: T } extends { v: string } ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e13<T>(n: 1, o: { pick(x: T extends infer U ? 1 : 1): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e14<T>(n: 2, o: { pick(x: T extends unknown ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e15<T>(n: 1, o: { pick(x: T extends T ? 1 : 2): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e16<T extends string>(n: 1, o: { pick(x: [T] extends [string] ? 1 : 1): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function e17<T>(n: 1, o: { pick(x: { v: T } extends { v: number } ? number : number): "hit"; pick(x: unknown): "other" }) { return o.pick(n); }
export function h1<T>(n: 1) { const o = null! as { pick(x: T extends unknown ? 1 : 2): "hit"; pick(x: unknown): "other" }; return o.pick(n); }
export function h2<T>(n: 1) { const o = null! as { pick(x: T extends string ? number : number): "hit"; pick(x: unknown): "other" }; return o.pick(n); }
export function h3<T>(n: 1) { const o = null! as { pick(x: [T] extends [string] ? 1 : 1): "hit"; pick(x: unknown): "other" }; return o.pick(n); }
"##;

/// Each generic function's measured answer.
const RETURNS: &[(&str, &str)] = &[
    ("a1", "any extends T ? 1 : 2"),
    ("a2", "T extends any ? 1 : 2"),
    ("a3", "1 | 2"),
    ("b1", "{ v: T; } extends { v: string; } ? 1 : 2"),
    ("b2", "[T] extends [string] ? 1 : 2"),
    ("b3", "1"),
    ("b4", "{ v: T; } extends { v: number; } ? 1 : 2"),
    ("b5", "{ v: T; } extends { v: number; } ? 1 : 2"),
    ("b6", "1"),
    ("b7", "1"),
    ("b9", "{ v: T[]; } extends { v: string[]; } ? 1 : 2"),
    ("b10", "2"),
    ("b12", "{ v: T; } extends { v: never; } ? 1 : 2"),
    ("i1", "{ a: T[\"a\"]; } extends { a: number; } ? 1 : 2"),
    ("i2", "{ a: T[\"a\"]; } extends { a: string; } ? 1 : 2"),
    ("i3", "T[\"a\"] extends number ? 1 : 2"),
    ("i4", "1"),
    ("i5", "1"),
    ("i6", "{ a: T[\"a\"]; } extends { a: never; } ? 1 : 2"),
    ("i7", "2"),
    ("i8", "{ a: T[keyof T]; } extends { a: string; } ? 1 : 2"),
    (
        "n1",
        "{ v: T; f: <U>(x: U) => T; } extends { v: string; f: unknown; } ? 1 : 2",
    ),
    ("n2", "1"),
    (
        "n3",
        "{ v: T; f: <T>(x: T) => T; } extends { v: string; f: unknown; } ? 1 : 2",
    ),
    ("n4", "1"),
    (
        "n5",
        "{ f: <U>(x: U) => T; } extends { f: (x: string) => string; } ? 1 : 2",
    ),
    ("n6", "1"),
    ("n7", "2"),
    ("n8", "2"),
];

/// `any extends T ? 1 : 2` stays deferred (the `any` check is read only
/// once neither operand is generic, and `[T]` is not generic, so `any
/// extends [T]` is `1 | 2`); `{ v: T } extends { v: string }` with `T
/// extends string` stays deferred (the restrictive instantiation drops the
/// constraint, so the true branch is not proven, and the permissive one
/// holds, so the false branch is not either), while `{ v: T } extends { v:
/// unknown }` and `{ v: T } extends { v: T }` hold restrictively (the
/// wildcard of the permissive test relating to itself). The wildcard relates
/// to `never` as well, where `any` does not, so `{ v: T } extends { v: never
/// }` stays deferred; `{ v: [T] } extends { v: [] }` fails even
/// permissively. A type parameter a generic signature declares is bound
/// only inside that signature: the function's `T` written in a signature
/// that declares `U` (or that declares a `T` of its own beside it) stays
/// free there and defers the conditional, while a signature's own
/// parameters never do: `{ f: <U>(x: U) => U }` against `{ f: (x: string)
/// => number }` is plainly false.
#[test]
fn conditionals_are_deferred_and_decided_as_the_checker_decides_them() {
    let failures = Matrix::new(SOURCE).settings(&ALL).returns(RETURNS);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Each call's measured answer: `"hit"` when the argument relates to the
/// first overload's parameter.
const CALLS: &[(&str, &str)] = &[
    ("c1", "\"hit\""),
    ("c2", "\"hit\""),
    ("c3", "\"other\""),
    ("c4", "\"other\""),
    ("c5", "\"hit\""),
    ("c6", "\"hit\""),
    ("c7", "\"other\""),
    ("c8", "\"other\""),
    ("c9", "\"other\""),
    ("e1", "\"other\""),
    ("e3", "\"other\""),
    ("e4", "\"hit\""),
    ("e5", "\"hit\""),
    ("e6", "\"other\""),
    ("e7", "\"hit\""),
    ("e8", "\"other\""),
    ("e9", "\"hit\""),
    ("e11", "\"hit\""),
    ("e12", "\"other\""),
    ("e13", "\"other\""),
    ("e14", "\"other\""),
    ("e15", "\"other\""),
    ("e16", "\"hit\""),
    ("e17", "\"hit\""),
    ("h3", "\"hit\""),
];

/// A deferred conditional source relates through its branches (`T extends
/// string ? 1 : 2` to `number` and to `1 | 2`, not to `string` or `1`),
/// through the conditional at its check type's constraint (`1` under `T
/// extends string`, `2` under `T extends number`), never through a
/// constraint that is `never`, and branch to branch against a conditional
/// of the same `extends` type. A deferred conditional target takes a source
/// relating to each branch it may still take (`[T] extends [string] ? 1 :
/// 1`), the false branch skipped where the restrictive instantiation holds;
/// never when it declares an `infer` or its branches or `extends` type read
/// its distributed check type (`T extends string ? 1 : T`, `T extends T ? 1
/// : 2`).
#[test]
fn deferred_conditionals_relate_as_the_checker_relates_them() {
    let failures = Matrix::new(SOURCE).settings(&ALL).returns(CALLS);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// An indexed access of a type parameter is generic, and relates as its
/// constraint: the same access of the parameter's constraint, and `unknown`
/// for the unconstrained parameter the restrictive test reads. Without
/// `strictNullChecks` `unknown` relates to `{}`, so the restrictive test
/// holds and the access's constraint (`1` for `T["a"]` under `T extends {
/// a: 1 }`, each member of a union constraint, and through an access of an
/// access too) decides the true branch; with it the conditional stays
/// deferred.
#[test]
fn indexed_accesses_of_a_type_parameter_relate_through_its_constraint() {
    let failures = Matrix::new(SOURCE).settings(&ALL).nullness(&[
        (
            Read::Return("k1"),
            "{ a: T[\"a\"]; } extends { a: {}; } ? 1 : 2",
            "1",
        ),
        (
            Read::Return("k2"),
            "{ a: T[\"a\"][\"b\"]; } extends { a: {}; } ? 1 : 2",
            "1",
        ),
        (
            Read::Return("k3"),
            "{ a: T[\"a\"]; } extends { a: {}; } ? 1 : 2",
            "1",
        ),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Whether a distributive conditional target reads its check type is read
/// by the checker off where the conditional is written (a block between the
/// type parameter's declaration and the conditional counts as a reading),
/// which the conditional's type does not carry. A source that relates to
/// each branch of such a target (`1` to `T extends unknown ? 1 : 2`, which
/// the checker takes in a parameter list, `"hit"`, and refuses in a
/// function body, `"other"`) is therefore undecided, never answered; a
/// target that is not distributive is related in a body too (`h3` above).
#[test]
fn a_distributive_target_whose_reading_is_unknown_is_not_answered() {
    let rows = [
        ("e2", "\"hit\""),
        ("e10", "\"hit\""),
        ("h1", "\"other\""),
        ("h2", "\"other\""),
    ];
    let reads: Vec<(Read<'_>, Vec<&str>)> = rows
        .iter()
        .map(|(function, answer)| (Read::Return(function), vec![*answer; ALL.len()]))
        .collect();
    let verdicts = Matrix::new(SOURCE).settings(&ALL).verdicts(&reads);
    for ((read, _), row) in reads.iter().zip(&verdicts) {
        for verdict in row {
            assert!(
                !verdict.matched && verdict.class == "GAP",
                "`{}` is undecided, not {} ({})",
                read.text(),
                verdict.class,
                verdict.lane
            );
        }
    }
}

/// Instantiated conditionals of each lattice row and distribution shape,
/// constrained generic operands, and `keyof T` arguments to overloads.
const CLOSED_SOURCE: &str = r##"
declare function pickStr(s: string): "hit"; declare function pickStr(s: unknown): "other";
type D<T> = T extends string ? "y" : "n";
type W<T> = [T] extends [string] ? "y" : "n";
type Bx<T> = T extends any ? { v: T } : never;
type N<T> = [T] extends [never] ? "never" : "other";
type IsAny<T> = 0 extends 1 & T ? true : false;
type K<T> = keyof T extends never ? "empty" : "keys";
export function d1() { return null! as D<string | number>; }
export function d2() { return null! as W<string | number>; }
export function d3() { return null! as D<never>; }
export function d4() { return null! as W<never>; }
export function d5() { return null! as D<any>; }
export function d6() { return null! as W<any>; }
export function d7() { return null! as Bx<1 | 2>; }
export function d8() { return null! as N<never>; }
export function d9() { return null! as IsAny<any>; }
export function d10() { return null! as IsAny<unknown>; }
export function d11() { return null! as D<boolean>; }
export function d12() { return null! as K<{}>; }
export function d13() { return null! as K<{ a: 1 }>; }
export function d14() { return null! as D<unknown>; }
export function d18<T extends string>() { return null! as (T extends string ? 1 : 2); }
declare const RefSymbol: unique symbol;
declare const RawSymbol: unique symbol;
declare const Marker: unique symbol;
interface Acc<T = any, S = T> { get value(): T; set value(_: S); [RefSymbol]: true; }
type ShallowAcc<T = any, S = T> = Acc<T, S> & { [Marker]?: true };
export function a1() { return null! as (Acc<number> extends Acc<infer V, unknown> ? V : 0); }
export function a2() { return null! as (Acc<number> extends ShallowAcc<infer V, unknown> ? V : 0); }
export function a3() { return null! as (Acc<number> extends Acc<number, unknown> ? 1 : 2); }
export function a4() { return null! as (Acc<number> extends { value: string } ? 1 : 2); }
export function a5() { return null! as Acc<number>["value"]; }
export function w1() { return null! as ({ a: 1 } extends { [RawSymbol]?: true } ? 1 : 2); }
export function w2() { return null! as ({ a: 1 } extends { a?: 1; b?: 2 } ? 1 : 2); }
export function w3() { return null! as ({} extends { [RawSymbol]?: true } ? 1 : 2); }
export function k5<T>(x: T extends string ? "a" : "b") { return pickStr(x); }
export function k6<T>(x: T extends string ? "a" : 1) { return pickStr(x); }
"##;

/// A union check distributes and a wrapped one does not (`D<string |
/// number>` is `"y" | "n"`, `W<string | number>` `"n"`); `never` is the
/// empty union to a distributive check and an ordinary type to a wrapped
/// one; `any` takes both branches of a distributive check and relates to a
/// wrapped one. A check holding a type parameter its constraint would
/// decide (`T extends string`) stays deferred. An interface's `get` / `set`
/// pair is one property of its getter's type, so a reference to it relates
/// and infers through that property whatever its setter takes. A weak
/// target (every property optional) takes no source that has properties
/// and shares none of them. A deferred conditional argument relates to a
/// parameter both its branches relate to, never to one only its true
/// branch does.
#[test]
fn instantiated_and_constrained_conditionals_decide_as_the_checker_decides_them() {
    let failures = Matrix::new(CLOSED_SOURCE).settings(&ALL).returns(&[
        ("d1", "\"n\" | \"y\""),
        ("d2", "\"n\""),
        ("d3", "never"),
        ("d4", "\"y\""),
        ("d5", "\"n\" | \"y\""),
        ("d6", "\"y\""),
        ("d7", "{ v: 1; } | { v: 2; }"),
        ("d8", "\"never\""),
        ("d9", "true"),
        ("d10", "false"),
        ("d11", "\"n\""),
        ("d12", "\"empty\""),
        ("d13", "\"keys\""),
        ("d14", "\"n\""),
        ("d18", "T extends string ? 1 : 2"),
        ("a1", "number"),
        ("a2", "number"),
        ("a3", "1"),
        ("a4", "2"),
        ("a5", "number"),
        ("w1", "2"),
        ("w2", "1"),
        ("w3", "1"),
        ("k5", "\"hit\""),
        ("k6", "\"other\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[test]
fn distributive_conditionals_split_boolean_and_boolean_union_members() {
    let source = r#"
type __BenchInit = 0;
type Exclude<T, U> = T extends U ? never : T;
type Extract<T, U> = T extends U ? T : never;
type F<T> = T extends true ? "t" : "f";
type Box<T> = T extends unknown ? { value: T } : never;
type Wrapped<T> = [T] extends [true] ? "t" : "f";
type Bool = boolean;
type BoolUnion = boolean | 0;
type Nested<T> = T extends infer U ? U extends true ? U : never : never;
"#;
    let failures = Matrix::new(source).settings(&ALL).types(&[
        ("F<boolean>", "\"f\" | \"t\""),
        ("Exclude<boolean | 0, false>", "0 | true"),
        ("Exclude<boolean | 0, true>", "0 | false"),
        ("Extract<boolean | 0, true>", "true"),
        ("F<Bool>", "\"f\" | \"t\""),
        ("Exclude<BoolUnion, false>", "0 | true"),
        ("Box<boolean>", "{ value: false; } | { value: true; }"),
        ("Nested<boolean>", "true"),
        ("Wrapped<boolean>", "\"f\""),
        ("[boolean] extends [true] ? \"t\" : \"f\"", "\"f\""),
        ("boolean extends true ? \"t\" : \"f\"", "\"f\""),
        ("F<true>", "\"t\""),
        ("F<false>", "\"f\""),
    ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Vue's ref unwrapping read through the real `vue` package, whose
/// `UnwrapRef` and `Ref` reach the probe through the `vue` → runtime-dom →
/// runtime-core → reactivity re-export chain. `Ref` declares its value as a
/// `get value()` / `set value()` accessor pair.
const VUE_SOURCE: &str = r#"
import { ref, proxyRefs, type UnwrapRef, type ShallowUnwrapRef, type Ref, type ShallowRef } from 'vue'
declare function r<T>(value: T): UnwrapRef<T>
const count = ref(0)
export function unwrapNumber() { return null! as UnwrapRef<number>; }
export function unwrapString() { return null! as UnwrapRef<string>; }
export function refZero() { return ref(0); }
export function unwrapInferred() { return r(0); }
export function unwrapRefNumber() { return null! as UnwrapRef<Ref<number>>; }
export function refAsShallow() { return null! as (Ref<number> extends ShallowRef<infer V, unknown> ? V : 0); }
export function exposedRefs() { return null! as ShallowUnwrapRef<{ count: typeof count }>; }
export function returnedRefs() { return null! as ShallowUnwrapRef<{ a: Ref<number>; b: string }>; }
export function proxied() { return proxyRefs({ count }); }
"#;

/// Each answer is the checker's, alike in all four settings: a primitive
/// and a ref decide their unwrapping, `ref(0)` is a `Ref<number, number>`,
/// and the refs a component exposes or returns unwrap to their values — a
/// `Ref` relates to `ShallowRef<infer V>` and `Ref<infer V>` through its
/// accessor's read type.
fn vue_ref_unwrapping_in(settings: &[Setting]) -> Vec<String> {
    Matrix::new(VUE_SOURCE)
        .settings(settings)
        .installed_packages()
        .returns(&[
            ("unwrapNumber", "number"),
            ("unwrapString", "string"),
            ("refZero", "Ref<number, number>"),
            ("unwrapInferred", "number"),
            ("unwrapRefNumber", "number"),
            ("refAsShallow", "number"),
            ("exposedRefs", "{ count: number; }"),
            ("returnedRefs", "{ a: number; b: string; }"),
            ("proxied", "{ count: number; }"),
        ])
}

/// [`vue_ref_unwrapping_in`] the two settings with `noImplicitAny`; each
/// project loads the real packages, so the other two are a test of their
/// own.
#[test]
fn vue_ref_unwrapping_decides_through_the_real_re_export_chain() {
    let failures = vue_ref_unwrapping_in(&ALL[..2]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// [`vue_ref_unwrapping_in`] the two settings without `noImplicitAny`.
#[test]
fn vue_ref_unwrapping_decides_without_no_implicit_any() {
    let failures = vue_ref_unwrapping_in(&ALL[2..]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Vue's `RefUnwrapBailTypes`, declared empty by reactivity and augmented
/// by `declare module "@vue/reactivity"` blocks in runtime-core and
/// runtime-dom, read from the dependencies `vue`'s runtime-dom installs.
const BAIL_SOURCE: &str = r#"
import type { RefUnwrapBailTypes, UnwrapRef } from '@vue/reactivity'
import type { VNode, ComponentInternalInstance } from '@vue/runtime-core'
import type {} from '@vue/runtime-dom'
type Bail = RefUnwrapBailTypes[keyof RefUnwrapBailTypes];
type CoreBail = RefUnwrapBailTypes["runtimeCoreBailTypes"];
export function coreKey() { return null! as ("runtimeCoreBailTypes" extends keyof RefUnwrapBailTypes ? 1 : 2); }
export function domKey() { return null! as ("runtimeDOMBailTypes" extends keyof RefUnwrapBailTypes ? 1 : 2); }
export function vnodeCore() { return null! as (VNode extends CoreBail ? 1 : 2); }
export function holderCore() { return null! as ({ $: ComponentInternalInstance } extends CoreBail ? 1 : 2); }
export function plainCore() { return null! as ({ a: 1 } extends CoreBail ? 1 : 2); }
export function vnode() { return null! as (VNode extends Bail ? 1 : 2); }
export function unwrapVNode() { return null! as (UnwrapRef<VNode> extends VNode ? 1 : 2); }
"#;

/// A package's `declare module "<package>"` block augments the module file
/// the specifier resolves to from the augmenting file, so the bail-type
/// interface holds both the runtime-core and the runtime-dom member, the
/// runtime-core member reads as the checker reads it, and a `VNode` bails
/// out of ref unwrapping through the written bail arm it relates to.
#[test]
fn package_augmentations_merge_into_the_module_they_resolve_to() {
    let failures = Matrix::new(BAIL_SOURCE)
        .settings(&ALL)
        .installed_packages_of(&["vue", "@vue/runtime-dom"])
        .returns(&[
            ("coreKey", "1"),
            ("domKey", "1"),
            ("vnodeCore", "1"),
            ("holderCore", "1"),
            ("plainCore", "2"),
            ("unwrapVNode", "1"),
        ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The runtime-dom member is `DomType<Node | Window>`, which reads
/// `typeof globalThis`: with no standard library registered the lane cannot
/// read it, so a conditional whose `extends` is the whole bail union, not a
/// union written with an arm the check relates to, stays unavailable —
/// never a branch the missing member could overturn (the checker answers
/// `1`).
#[test]
fn a_bail_union_with_an_unreadable_member_is_not_answered() {
    let reads: Vec<(Read<'_>, Vec<&str>)> = ["vnode"]
        .into_iter()
        .map(|function| (Read::Return(function), vec!["1"; ALL.len()]))
        .collect();
    let verdicts = Matrix::new(BAIL_SOURCE)
        .settings(&ALL)
        .installed_packages_of(&["vue", "@vue/runtime-dom"])
        .verdicts(&reads);
    for ((read, _), row) in reads.iter().zip(&verdicts) {
        assert_eq!(
            row.len(),
            ALL.len(),
            "`{}` is read in every setting",
            read.text()
        );
        for verdict in row {
            assert!(
                !verdict.matched && verdict.class == "GAP",
                "`{}` is unavailable, not {} ({})",
                read.text(),
                verdict.class,
                verdict.lane
            );
        }
    }
}

/// An interface a sibling file augments through `declare module "./base"`,
/// read by an indexed access as a conditional's `extends` operand.
const AUGMENTED_FILES: &[(&str, &str)] = &[
    ("base.ts", "export interface Bag {}\n"),
    (
        "aug.ts",
        "import type {} from './base'\ninterface Local { req: string }\ndeclare module './base' { interface Bag { core: Local | { $: number } } }\n",
    ),
];

const AUGMENTED_SOURCE: &str = r#"
import type { Bag } from './base'
import type {} from './aug'
type Core = Bag["core"];
export function m1() { return null! as ({ a: 1 } extends Bag["core"] ? 1 : 2); }
export function m2() { return null! as ({ $: 1 } extends Bag["core"] ? 1 : 2); }
export function m3() { return null! as ({ a: 1 } extends Bag[keyof Bag] ? 1 : 2); }
export function m4() { return null! as ({ a: 1 } extends Core ? 1 : 2); }
export function m5() { return null! as ({ req: "x" } extends Core ? 1 : 2); }
"#;

/// A relation reads an indexed access operand as the member's TYPE: a union
/// member read through an augmented interface (`Local | { $: number }`),
/// directly or through an alias, relates arm by arm — never as the empty
/// one-level surface its arms share, which every object would satisfy.
#[test]
fn indexed_members_of_an_augmented_interface_relate_as_their_type() {
    let failures = Matrix::new(AUGMENTED_SOURCE)
        .settings(&ALL)
        .files(AUGMENTED_FILES)
        .returns(&[
            ("m1", "2"),
            ("m2", "1"),
            ("m3", "2"),
            ("m4", "2"),
            ("m5", "1"),
        ]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Conditionals whose `extends` union holds an arm the lane cannot read:
/// `Dom<1>` reads `typeof globalThis`, which no standard library supplies.
const UNREADABLE_ARM_SOURCE: &str = r#"
type Dom<T> = typeof globalThis extends { window: unknown } ? T : never;
interface Bag { a: Dom<1>; b: boolean }
type F2<T> = T extends string | Bag[keyof Bag] ? { v: T } : 0;
type Two<X> = X | "z";
type An = any;
type AB = "a" | "b";
type Id<T> = T;
export function u1() { return null! as ("a" extends string | Dom<1> ? 1 : 2); }
export function u2() { return null! as F2<"a">; }
export function u3() { return null! as F2<Two<"a">>; }
export function u4() { return null! as (An extends string | Dom<1> ? 1 : 2); }
export function u5() { return null! as F2<never>; }
export function u6() { return null! as F2<AB>; }
export function u7() { return null! as F2<An>; }
export function u8() { return null! as F2<keyof { a: 1; b: 2 }>; }
export function u9() { return null! as (Id<any> extends string | Dom<1> ? 1 : 2); }
export function u10() { return null! as ((1 & Id<any>) extends string | Dom<1> ? 1 : 2); }
export function u11() { return null! as F2<Id<never>>; }
"#;

/// A check related to one readable arm of a union `extends` is related to
/// the union whatever its unreadable arms are, so a plain check takes the
/// true branch. A check that distributes or absorbs — a union named
/// through an alias or an instantiation, `any`, `never` and a closed
/// `keyof` — is decided per member or by its lattice row, never by
/// relating the whole check to one arm, whether it names that type
/// directly or through an instantiation (`Id<any>`, `1 & Id<any>`,
/// `Id<never>`): those stay unavailable until every member is decided.
#[test]
fn a_written_arm_proves_only_a_check_that_neither_distributes_nor_absorbs() {
    let failures = Matrix::new(UNREADABLE_ARM_SOURCE)
        .settings(&ALL)
        .returns(&[("u1", "1"), ("u2", "{ v: \"a\"; }")]);
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    let rows = [
        ("u3", "{ v: \"a\"; } | { v: \"z\"; }"),
        ("u4", "1 | 2"),
        ("u5", "never"),
        ("u6", "{ v: \"a\"; } | { v: \"b\"; }"),
        ("u7", "0 | { v: any; }"),
        ("u8", "{ v: \"a\"; } | { v: \"b\"; }"),
        ("u9", "1 | 2"),
        ("u10", "1 | 2"),
        ("u11", "never"),
    ];
    let reads: Vec<(Read<'_>, Vec<&str>)> = rows
        .iter()
        .map(|(function, answer)| (Read::Return(function), vec![*answer; ALL.len()]))
        .collect();
    let verdicts = Matrix::new(UNREADABLE_ARM_SOURCE)
        .settings(&ALL)
        .verdicts(&reads);
    for ((read, _), row) in reads.iter().zip(&verdicts) {
        assert_eq!(
            row.len(),
            ALL.len(),
            "`{}` is read in every setting",
            read.text()
        );
        for verdict in row {
            assert!(
                verdict.matched || verdict.class == "GAP",
                "`{}` is the checker's answer or unavailable, not {} ({})",
                read.text(),
                verdict.class,
                verdict.lane
            );
        }
    }
}

/// Named type queries must not publish a whole-boolean branch when an
/// unreadable extends arm prevents per-member decisions. Return-flow reads
/// already retain a gap for this input, so exercise the public named demand.
#[test]
fn a_written_arm_cannot_select_a_whole_distributive_boolean() {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::checker_syntax;
    use std::sync::Arc;
    use verter_type_engine::semantic_query::{QueryError, SemanticNodeData};

    let source = format!(
        "{UNREADABLE_ARM_SOURCE}\n\
         type Bool = boolean;\n\
         type FBool<T> = T extends boolean | Dom<1> ? {{ v: T }} : 0;\n\
         type Direct = FBool<boolean>;\n\
         type Aliased = FBool<Bool>;\n\
         type Whole = boolean extends boolean | Dom<1> ? 1 : 2;\n"
    );
    let host = Matrix::new(&source).settings(&ALL).host(&[]);
    for setting in ALL {
        let canonical = format!("{}/probe.ts", setting.root);
        for (name, answer) in [
            ("Whole", "1"),
            ("Direct", "{ v: false; } | { v: true; }"),
            ("Aliased", "{ v: false; } | { v: true; }"),
        ] {
            let result = host
                .resolve_named_symbol_with_audit(&canonical, name, None)
                .into_result()
                .expect("the named demand does not fault");
            let Some(node) = result else {
                assert_ne!(name, "Whole", "the non-distributive control resolves");
                continue;
            };
            let store_view = host.resolver_store_view_read().into_owned_view();
            let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
            let host_ctx =
                crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
            let dispatch = super::ProjectSemanticDispatch::new(&host_ctx);
            let expected = checker_syntax::parse(answer).expect("valid checker type");
            let data = dispatch.graph().node_data(node.id());
            let gap = name != "Whole"
                && matches!(
                    data.as_deref(),
                    Some(
                        SemanticNodeData::Conditional { .. }
                            | SemanticNodeData::Opaque(
                                QueryError::IncompleteSemanticOperand { .. }
                            )
                    )
                );
            assert!(
                gap || checker_syntax::matches_node(&dispatch, node.id(), &expected, 0),
                "{name} [{}] must be {answer} or an unresolved conditional, got {data:?}",
                setting.label
            );
        }
    }
}
