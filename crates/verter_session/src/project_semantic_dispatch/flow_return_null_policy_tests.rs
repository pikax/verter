//! Flow-return inference under the function's OWN `strictNullChecks` and
//! `noImplicitAny`.
//!
//! Every expected answer here was measured on the pinned TypeScript 7.0.2
//! checker (`tsc --noEmit --strict`, once as is and once with
//! `--strictNullChecks false`, the auto-typed table in each combination
//! with `--noImplicitAny false` as well), read from a checker message
//! quoting `ReturnType<typeof f>` (TS2741, or TS2322 against a `never`
//! probe); for the first table the emitted `.d.ts` agrees row for row. The checker's print of each row is quoted beside it; the
//! table itself spells the answer in [`answer_text`]'s form, which sorts
//! union members so the comparison is independent of union member order.

use std::sync::Arc;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    FlowReturnStep, LiteralValue, NullabilityPolicy, PrimitiveKind, QueryResult,
    ReturnProjectionDemand, SemanticNodeData, SemanticNodeId, SemanticQueryApi, SemanticQueryKey,
};
use crate::types::HostConfig;
use crate::VerterHost;

const STRICT_ROOT: &str = "/strict";
const LOOSE_ROOT: &str = "/loose";

/// One host carrying two tsconfig-backed projects that differ ONLY in
/// `strictNullChecks`.
fn two_policy_host() -> VerterHost {
    VerterHost::new_standalone_with_tsconfig_projects(
        HostConfig::default(),
        &[
            (STRICT_ROOT, r#"{ "compilerOptions": { "strict": true } }"#),
            (
                LOOSE_ROOT,
                r#"{ "compilerOptions": { "strict": true, "strictNullChecks": false } }"#,
            ),
        ],
    )
}

fn upsert(host: &VerterHost, canonical: &str, source: &str) {
    crate::u6_flow_shape_corpus_tests::upsert(
        host,
        canonical,
        source,
        crate::FileLanguage::script(verter_language::ScriptSourceType::Ts),
    );
}

fn identity(canonical: &str, symbol: &str) -> verter_type_expr::facts::FlowFunctionReturnIdentity {
    verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part: verter_type_expr::facts::FunctionPartIdentity::DeclarationBody,
        overload_ordinal: 0,
    }
}

/// A node as TypeScript-like text: primitives and literals as TS prints
/// them, union members SORTED and joined by ` | `, object members in
/// declaration order (`readonly ` / `?` as TS marks them), an array as
/// `T[]` (a union element parenthesised), a tuple as `[A, B]`, a
/// signature as `(name?: type) => return`, a generic application as
/// `Name<A, B>`.
/// The printed name of the parameter at `index` in an already-rendered
/// parameter list (`name: type` / `name?: type`).
fn params_names(params: &[String], index: usize) -> String {
    params
        .get(index)
        .and_then(|param| param.split([':', '?']).next())
        .unwrap_or("_")
        .to_string()
}

fn answer_text(host: &VerterHost, node: SemanticNodeId) -> String {
    let graph = host.project_type_store().semantic_graph();
    let Some(data) = graph.node_data(node) else {
        return "<evicted>".to_owned();
    };
    match data.as_ref() {
        SemanticNodeData::Primitive(kind) => format!("{kind:?}").to_ascii_lowercase(),
        SemanticNodeData::Literal(LiteralValue::String(value)) => format!("\"{value}\""),
        SemanticNodeData::Literal(LiteralValue::Number(value)) => format!("{value}"),
        SemanticNodeData::Literal(LiteralValue::Boolean(value)) => format!("{value}"),
        SemanticNodeData::Union(members) => {
            let mut parts: Vec<String> = members
                .iter()
                .map(|member| answer_text(host, *member))
                .collect();
            parts.sort();
            parts.join(" | ")
        }
        SemanticNodeData::Object(surface) => {
            let members: Vec<String> = surface
                .positive_members()
                .iter()
                .map(|member| {
                    format!(
                        "{}{}{}: {}",
                        if member.readonly { "readonly " } else { "" },
                        member.key.as_string().unwrap_or("<key>"),
                        if member.optional { "?" } else { "" },
                        answer_text(host, member.value)
                    )
                })
                .collect();
            format!("{{ {} }}", members.join("; "))
        }
        SemanticNodeData::Signature {
            params,
            return_type,
            predicate,
            ..
        } => {
            let params: Vec<String> = params
                .iter()
                .map(|param| {
                    format!(
                        "{}{}: {}",
                        param.name.as_deref().unwrap_or("_"),
                        if param.optional { "?" } else { "" },
                        answer_text(host, param.ty)
                    )
                })
                .collect();
            // A predicate prints in the return position, as the checker does.
            let result = match predicate {
                Some(predicate) => {
                    let subject = match predicate.subject {
                        crate::semantic_query::PredicateSubject::This => "this".to_string(),
                        crate::semantic_query::PredicateSubject::Parameter(index) => {
                            params_names(&params, index as usize)
                        }
                    };
                    let asserts = if predicate.asserts { "asserts " } else { "" };
                    match predicate.ty {
                        Some(target) => {
                            format!("{asserts}{subject} is {}", answer_text(host, target))
                        }
                        None => format!("{asserts}{subject}"),
                    }
                }
                None => answer_text(host, *return_type),
            };
            format!("({}) => {result}", params.join(", "))
        }
        SemanticNodeData::Array { element, readonly } => {
            format!(
                "{}{}[]",
                if *readonly { "readonly " } else { "" },
                parenthesized_operand(host, *element)
            )
        }
        SemanticNodeData::Tuple { elements, readonly } => {
            let elements: Vec<String> = elements
                .iter()
                .map(
                    |element| match (&element.label, element.rest, element.optional) {
                        (Some(label), true, _) => {
                            format!("...{label}: {}", answer_text(host, element.value))
                        }
                        (Some(label), false, true) => {
                            format!("{label}?: {}", answer_text(host, element.value))
                        }
                        (Some(label), false, false) => {
                            format!("{label}: {}", answer_text(host, element.value))
                        }
                        (None, true, _) => format!("...{}", answer_text(host, element.value)),
                        (None, false, true) => {
                            format!("{}?", parenthesized_operand(host, element.value))
                        }
                        (None, false, false) => answer_text(host, element.value),
                    },
                )
                .collect();
            format!(
                "{}[{}]",
                if *readonly { "readonly " } else { "" },
                elements.join(", ")
            )
        }
        SemanticNodeData::InstantiationRef { base, args } => {
            let args: Vec<String> = args.iter().map(|arg| answer_text(host, *arg)).collect();
            format!("{}<{}>", base.decl_name, args.join(", "))
        }
        SemanticNodeData::DeclRef { identity } => identity.decl_name.to_string(),
        other => format!("<unrendered {other:?}>"),
    }
}

/// A node printed as the operand of a postfix type operator (`T[]`, an
/// optional tuple element's `T?`): parenthesized when it is a union, a
/// function type or a `readonly` array or tuple, as the checker prints it.
fn parenthesized_operand(host: &VerterHost, node: SemanticNodeId) -> String {
    let text = answer_text(host, node);
    let graph = host.project_type_store().semantic_graph();
    let needs_parentheses = matches!(
        graph.node_data(node).as_deref(),
        Some(
            SemanticNodeData::Union(_)
                | SemanticNodeData::Signature { .. }
                | SemanticNodeData::Array { readonly: true, .. }
                | SemanticNodeData::Tuple { readonly: true, .. }
        )
    );
    if needs_parentheses {
        format!("({text})")
    } else {
        text
    }
}

/// The COMPLETE whole-return answer of `symbol` in `canonical`, through the
/// public audited flow-return boundary. A degraded or refused answer is a
/// failure of the row, never a comparable value.
fn observe(host: &VerterHost, canonical: &str, symbol: &str) -> String {
    let carrier = host.get_flow_return_type_with_audit(
        &identity(canonical, symbol),
        ReturnProjectionDemand::whole_return(),
    );
    match carrier.as_result() {
        Ok(result) if result.degradation().is_none() => answer_text(host, result.return_type()),
        Ok(result) => format!(
            "{} [degraded {:?}]",
            answer_text(host, result.return_type()),
            result.degradation()
        ),
        Err(error) => format!("<no value: {error:?}>"),
    }
}

/// The measured table's source, upserted verbatim into BOTH projects.
const SOURCE: &str = r#"
export function leaf(v?: string) { return v; }
export function nullable(v: string | null) { return v; }
export function twoArms(c: boolean) { if (c) return "a"; return null; }
export function fallthrough(c: boolean) { if (c) return 1; }
export function nested(v: string | null) { return { a: v }; }
export function loneNull() { return null; }
interface O { p?: string; q: string | null }
export function optRead(o: O) { return o.p; }
export function declRead(o: O) { return o.q; }
export function optChain(o?: O) { return o?.q; }
export function bare(c: boolean) { if (c) return; return 1; }
export function declNull(v: null) { return v; }
export function asNull() { return null as null; }
export function localCopy(v: string | null) { const x = v; return x; }
export function nestedSig() { return (v?: string) => v; }
export function closure(v?: string) { return () => v; }
export function ternary(c: boolean) { return c ? "a" : null; }
export function ternarySame(c: boolean) { return c ? "a" : "a"; }
export function objLit(c: boolean) { return { x: c ? 1 : null }; }
export function unionLitNull(c: number) { if (c === 1) return "a"; if (c === 2) return "b"; return null; }
export function unionMixed(c: boolean, s: string | null) { if (c) return s; return 1; }
export function nullDefault(v: string | null = null) { return v; }
declare function declaredCall(): string | null;
export function callDecl() { return declaredCall(); }
"#;

/// A same-project caller of `leaf`: its answer is the callee's answer,
/// inferred under the callee's own policy.
const PARENT: &str = r#"
import { leaf } from "./main";
export function parent(v?: string) { return leaf(v); }
"#;

/// `(symbol, strict answer, strictNullChecks-off answer)`, each measured on
/// TypeScript 7.0.2; the TS print follows every row.
const TABLE: &[(&str, &str, &str)] = &[
    // string | undefined / string
    ("leaf", "string | undefined", "string"),
    // string | null / string
    ("nullable", "null | string", "string"),
    // "a" | null / string
    ("twoArms", "\"a\" | null", "string"),
    // 1 | undefined / number
    ("fallthrough", "1 | undefined", "number"),
    // { a: string | null; } / { a: string; }
    ("nested", "{ a: null | string }", "{ a: string }"),
    // null / any (TS7010 under noImplicitAny)
    ("loneNull", "null", "any"),
    // string | undefined / string
    ("optRead", "string | undefined", "string"),
    // string | null / string
    ("declRead", "null | string", "string"),
    // string | null | undefined / string
    ("optChain", "null | string | undefined", "string"),
    // 1 | undefined / number
    ("bare", "1 | undefined", "number"),
    // null / null — a declared `null` is not the widening literal type
    ("declNull", "null", "null"),
    // null / null — an asserted `null` is not the widening literal type
    ("asNull", "null", "null"),
    // string | null / string
    ("localCopy", "null | string", "string"),
    // (v?: string) => string | undefined / (v?: string) => string — the
    // optional parameter's body type carries `undefined` only when strict
    (
        "nestedSig",
        "(v?: string | undefined) => string | undefined",
        "(v?: string) => string",
    ),
    // () => string | undefined / () => string
    ("closure", "() => string | undefined", "() => string"),
    // "a" | null / string
    ("ternary", "\"a\" | null", "string"),
    // string / string
    ("ternarySame", "string", "string"),
    // { x: number | null; } / { x: number; }
    ("objLit", "{ x: null | number }", "{ x: number }"),
    // "a" | "b" | null / "a" | "b"
    ("unionLitNull", "\"a\" | \"b\" | null", "\"a\" | \"b\""),
    // string | 1 | null / string | 1
    ("unionMixed", "1 | null | string", "1 | string"),
    // string | null / string
    ("nullDefault", "null | string", "string"),
    // string | null / string
    ("callDecl", "null | string", "string"),
];

fn measured_table_mismatches(host: &VerterHost) -> Vec<String> {
    let mut mismatches = Vec::new();
    let mut check = |canonical: &str, symbol: &str, expected: &str| {
        let observed = observe(host, canonical, symbol);
        if observed != expected {
            mismatches.push(format!(
                "{canonical} `{symbol}`: expected `{expected}`, observed `{observed}`"
            ));
        }
    };
    for (symbol, strict, loose) in TABLE {
        check("/strict/main.ts", symbol, strict);
        check("/loose/main.ts", symbol, loose);
    }
    // string | undefined / string
    check("/strict/parent.ts", "parent", "string | undefined");
    check("/loose/parent.ts", "parent", "string");
    mismatches
}

fn upsert_table_sources(host: &VerterHost) {
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        upsert(host, &format!("{root}/main.ts"), SOURCE);
        upsert(host, &format!("{root}/parent.ts"), PARENT);
    }
}

/// The same body in two projects that differ only in `strictNullChecks`
/// answers each project's own TypeScript 7.0.2 measurement: optional
/// parameters, optional member reads, optional chains, bare returns and
/// fall-throughs add `undefined` only when strict; with it off a union
/// keeps no `null` / `undefined` beside another member (so a declared
/// `string | null` read is `string`, nested object members included), the
/// lone fresh literal left behind widens (`"a"` → `string`, `1` →
/// `number`), and a return of bare `null` alone widens to `any`.
#[test]
fn flow_returns_follow_their_own_projects_strict_null_checks() {
    let host = two_policy_host();
    assert!(
        host.semantic_compiler_options_for("/strict/main.ts")
            .strict_null_checks
    );
    assert!(
        !host
            .semantic_compiler_options_for("/loose/main.ts")
            .strict_null_checks
    );
    upsert_table_sources(&host);
    let mismatches = measured_table_mismatches(&host);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 table:\n{}",
        mismatches.join("\n")
    );
}

/// Every `(symbol, strict answer, strictNullChecks-off answer)` row of
/// `table` for `file` upserted into both projects, as the mismatch list.
fn table_mismatches(
    host: &VerterHost,
    file: &str,
    source: &str,
    table: &[(&str, &str, &str)],
) -> Vec<String> {
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        upsert(host, &format!("{root}/{file}"), source);
    }
    let mut mismatches = Vec::new();
    for (symbol, strict, loose) in table {
        for (root, expected) in [(STRICT_ROOT, strict), (LOOSE_ROOT, loose)] {
            let canonical = format!("{root}/{file}");
            let observed = observe(host, &canonical, symbol);
            if observed != *expected {
                mismatches.push(format!(
                    "{canonical} `{symbol}`: expected `{expected}`, observed `{observed}`"
                ));
            }
        }
    }
    mismatches
}

/// Bare `null` / `undefined` / `void` values nested in object and array
/// literals, locals initialised from them, and declared nullable unions
/// nested in inline structures.
const NESTED_SOURCE: &str = r#"
export function objNull() { return { a: null }; }
export function objUndef() { return { a: undefined }; }
export function objVoid() { return { a: void 0 }; }
export function objNested() { return { a: { b: null } }; }
export function objTernary(c: boolean) { return { a: c ? null : undefined }; }
export function objAsConst() { return { a: null } as const; }
export function objSatisfies() { return { a: null } satisfies object; }
export function objDeclNull(v: null) { return { a: v }; }
export function objTernDecl(c: boolean, v: null) { return { a: c ? null : v }; }
export function arrNull() { return [null]; }
export function arrMixed() { return [null, 1]; }
export function arrUndefNull() { return [null, undefined]; }
export function arrObjNull() { return [{ a: null }]; }
export function arrArrNull() { return [[null]]; }
export function objArrUndef() { return { a: [undefined] }; }
export function tupleNull() { return [null] as const; }
export function letNull() { let x = null; return x; }
export function letUndef() { let y = undefined; return y; }
export function varNull() { var y = null; return y; }
export function constNull() { const y = null; return y; }
export function constUndef() { const y = undefined; return y; }
export function constTernary(c: boolean) { const y = c ? null : undefined; return y; }
export function constTernaryMixed(c: boolean) { const y = c ? null : 1; return y; }
export function letTernary(c: boolean) { let x = c ? null : undefined; return x; }
export function declLocalNull() { const y: null = null; return y; }
export function declLocalUndef() { let y: undefined = undefined; return y; }
export function letNullCond(c: boolean) { let x = null; if (c) x = "s"; return x; }
export function letUndefCond(c: boolean) { let x = undefined; if (c) x = 1; return x; }
export function letNullNull(c: boolean) { let x = null; if (c) x = null; return x; }
export function letNullDecl(c: boolean, v: null) { let x = null; if (c) x = v; return x; }
export function noInitNull() { let x; x = null; return x; }
export function declInitNullWrite(c: boolean, v: null) { let x = v; if (c) x = null; return x; }
export function constFromLet() { let x = null; const y = x; return y; }
export function returnTernaryLocal(c: boolean) { let x = null; return c ? x : undefined; }
export function letArr() { let a = [null]; return a; }
export function letObjNull() { let o = { a: null }; return o; }
export function constObjNull() { const o = { a: null }; return o; }
export function objFromConst() { const n = null; return { a: n }; }
export function objFromLet() { let n = null; return { a: n }; }
export function nestedDecl(o: { a: string | null }) { return o; }
export function arrDecl(v: (string | null)[]) { return v; }
export function tupleDecl(t: [string | null]) { return t; }
export function sigDecl(f: (x: string | null) => void) { return f; }
export function guardDecl(g: (x: unknown) => x is string | null) { return g; }
"#;

/// `(symbol, strict, off)` for [`NESTED_SOURCE`], each the checker's
/// answer on TypeScript 7.0.2 — read from the checker (a TS2741 message
/// quoting `ReturnType<typeof f>`), not from the emitted `.d.ts`, whose
/// printer re-derives a nested literal's type from its initializer
/// (`{ a: [undefined] }` prints `{ a: undefined[] }` there while the
/// checker holds `{ a: any[] }`). The checker's print follows each row.
const NESTED_TABLE: &[(&str, &str, &str)] = &[
    // { a: null; } / { a: any; }
    ("objNull", "{ a: null }", "{ a: any }"),
    // { a: undefined; } / { a: any; }
    ("objUndef", "{ a: undefined }", "{ a: any }"),
    // { a: undefined; } / { a: any; }
    ("objVoid", "{ a: undefined }", "{ a: any }"),
    // { a: { b: null; }; } / { a: { b: any; }; }
    ("objNested", "{ a: { b: null } }", "{ a: { b: any } }"),
    // { a: null | undefined; } / { a: any; }
    ("objTernary", "{ a: null | undefined }", "{ a: any }"),
    // { readonly a: null; } / { readonly a: any; }
    ("objAsConst", "{ readonly a: null }", "{ readonly a: any }"),
    // { a: null; } / { a: any; }
    ("objSatisfies", "{ a: null }", "{ a: any }"),
    // { a: null; } / { a: null; } — a declared `null` never widens
    ("objDeclNull", "{ a: null }", "{ a: null }"),
    // { a: null; } / { a: null; } — the declared arm is non-widening
    ("objTernDecl", "{ a: null }", "{ a: null }"),
    // null[] / any[]
    ("arrNull", "null[]", "any[]"),
    // (number | null)[] / number[]
    ("arrMixed", "(null | number)[]", "number[]"),
    // (null | undefined)[] / any[]
    ("arrUndefNull", "(null | undefined)[]", "any[]"),
    // { a: null; }[] / { a: any; }[]
    ("arrObjNull", "{ a: null }[]", "{ a: any }[]"),
    // null[][] / any[][]
    ("arrArrNull", "null[][]", "any[][]"),
    // { a: undefined[]; } / { a: any[]; }
    ("objArrUndef", "{ a: undefined[] }", "{ a: any[] }"),
    // readonly [null] / readonly [any]
    ("tupleNull", "readonly [null]", "readonly [any]"),
    // null / any — the auto-typed `let` reads the widening `null`
    ("letNull", "null", "any"),
    // undefined / any
    ("letUndef", "undefined", "any"),
    // null / any
    ("varNull", "null", "any"),
    // null / any — a `const` is declared as the widened type
    ("constNull", "null", "any"),
    // undefined / any
    ("constUndef", "undefined", "any"),
    // null | undefined / any
    ("constTernary", "null | undefined", "any"),
    // 1 | null / number
    ("constTernaryMixed", "1 | null", "number"),
    // null | undefined / any
    ("letTernary", "null | undefined", "any"),
    // null / null — a declared local never widens
    ("declLocalNull", "null", "null"),
    // undefined / undefined
    ("declLocalUndef", "undefined", "undefined"),
    // string | null / string
    ("letNullCond", "null | string", "string"),
    // number | undefined / number
    ("letUndefCond", "number | undefined", "number"),
    // null / any — a widening write keeps the local widening
    ("letNullNull", "null", "any"),
    // null / null — a declared-`null` write ends it
    ("letNullDecl", "null", "null"),
    // null / any — an initializer-less `let` is auto-typed too
    ("noInitNull", "null", "any"),
    // null / null — a `let` initialised from a declared `null` is not
    // auto-typed, so a later bare `null` write does not widen it
    ("declInitNullWrite", "null", "null"),
    // null / any
    ("constFromLet", "null", "any"),
    // null | undefined / any
    ("returnTernaryLocal", "null | undefined", "any"),
    // null[] / any[]
    ("letArr", "null[]", "any[]"),
    // { a: null; } / { a: any; }
    ("letObjNull", "{ a: null }", "{ a: any }"),
    // { a: null; } / { a: any; }
    ("constObjNull", "{ a: null }", "{ a: any }"),
    // { a: null; } / { a: any; }
    ("objFromConst", "{ a: null }", "{ a: any }"),
    // { a: null; } / { a: any; }
    ("objFromLet", "{ a: null }", "{ a: any }"),
    // { a: string | null; } / { a: string; }
    ("nestedDecl", "{ a: null | string }", "{ a: string }"),
    // (string | null)[] / string[]
    ("arrDecl", "(null | string)[]", "string[]"),
    // [string | null] / [string]
    ("tupleDecl", "[null | string]", "[string]"),
    // (x: string | null) => void / (x: string) => void
    (
        "sigDecl",
        "(x: null | string) => void",
        "(x: string) => void",
    ),
    // (x: unknown) => x is string | null / (x: unknown) => x is string
    (
        "guardDecl",
        "(x: unknown) => x is null | string",
        "(x: unknown) => x is string",
    ),
];

/// With `strictNullChecks` off a bare `null` / `undefined` / `void` value
/// is the checker's WIDENING nullable type: nested in an object or array
/// literal it widens to `any` with the literal (`{ a: null }` is
/// `{ a: any }`, `[null]` is `any[]`, `[null, 1]` is `number[]`), a
/// `const` initialised to one is declared `any`, and an auto-typed `let` /
/// `var` reads it until a later write retypes it. A declared `null` /
/// `undefined` — a parameter, an annotated local, an assertion — never
/// widens, and a declared union nested in an inline object, array, tuple
/// or function type loses its nullable members. Each row matches its own
/// project's TypeScript 7.0.2 answer.
#[test]
fn nested_and_local_nullish_values_widen_with_strict_null_checks_off() {
    let host = two_policy_host();
    let mismatches = table_mismatches(&host, "nested.ts", NESTED_SOURCE, NESTED_TABLE);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 table:\n{}",
        mismatches.join("\n")
    );
}

/// Generator yields and bare `undefined` / `void 0` returns. The lib
/// generator surfaces this standalone host has no `lib*.d.ts` for are
/// declared in the file: the wrap resolves `Generator` / `AsyncGenerator`
/// through the ordinary bare-reference resolver.
const YIELD_AND_UNDEFINED_SOURCE: &str = r#"
interface Generator<T, TReturn, TNext> {}
interface AsyncGenerator<T, TReturn, TNext> {}
export function* genNull() { yield null; }
export function* genUndef() { yield undefined; }
export function* genVoid() { yield void 0; }
export function* genBare() { yield; }
export function* genNullStr() { yield null; yield "s"; }
export function* genBareAndNum() { yield; yield 1; }
export function* genDeclNull(v: null) { yield v; }
export async function* agenNull() { yield null; }
export async function* agenUndef() { yield undefined; }
export function returnUndefined() { return undefined; }
export function returnVoid0() { return void 0; }
export function returnUndefinedAndNum(c: boolean) { if (c) return 1; return undefined; }
"#;

/// `(symbol, strict, off)` for [`YIELD_AND_UNDEFINED_SOURCE`], each the
/// checker's answer on TypeScript 7.0.2 (the checker's print follows each
/// row).
const YIELD_AND_UNDEFINED_TABLE: &[(&str, &str, &str)] = &[
    // Generator<null, void, unknown> / Generator<any, void, unknown>
    (
        "genNull",
        "Generator<null, void, unknown>",
        "Generator<any, void, unknown>",
    ),
    // Generator<undefined, void, unknown> / Generator<any, void, unknown>
    (
        "genUndef",
        "Generator<undefined, void, unknown>",
        "Generator<any, void, unknown>",
    ),
    // Generator<undefined, void, unknown> / Generator<any, void, unknown>
    (
        "genVoid",
        "Generator<undefined, void, unknown>",
        "Generator<any, void, unknown>",
    ),
    // Generator<undefined, void, unknown> / Generator<any, void, unknown>
    (
        "genBare",
        "Generator<undefined, void, unknown>",
        "Generator<any, void, unknown>",
    ),
    // Generator<"s" | null, void, unknown> / Generator<string, void, unknown>
    (
        "genNullStr",
        "Generator<\"s\" | null, void, unknown>",
        "Generator<string, void, unknown>",
    ),
    // Generator<1 | undefined, void, unknown> / Generator<number, void, unknown>
    (
        "genBareAndNum",
        "Generator<1 | undefined, void, unknown>",
        "Generator<number, void, unknown>",
    ),
    // Generator<null, void, unknown> / Generator<null, void, unknown>
    (
        "genDeclNull",
        "Generator<null, void, unknown>",
        "Generator<null, void, unknown>",
    ),
    // AsyncGenerator<null, void, unknown> / AsyncGenerator<any, void, unknown>
    (
        "agenNull",
        "AsyncGenerator<null, void, unknown>",
        "AsyncGenerator<any, void, unknown>",
    ),
    // AsyncGenerator<undefined, void, unknown> / AsyncGenerator<any, void, unknown>
    (
        "agenUndef",
        "AsyncGenerator<undefined, void, unknown>",
        "AsyncGenerator<any, void, unknown>",
    ),
    // undefined / any
    ("returnUndefined", "undefined", "any"),
    // undefined / any
    ("returnVoid0", "undefined", "any"),
    // 1 | undefined / number
    ("returnUndefinedAndNum", "1 | undefined", "number"),
];

/// A generator's yield type is widened exactly as a return type is: with
/// `strictNullChecks` off a yield of only bare `null` / `undefined` /
/// `void` values is `any` (sync and async), and a nullable yield beside
/// another vanishes before the lone-fresh-literal rule. A bare
/// `undefined` or `void 0` return is `undefined` when strict and `any`
/// when off.
#[test]
fn yields_and_bare_undefined_returns_follow_strict_null_checks() {
    let host = two_policy_host();
    let mismatches = table_mismatches(
        &host,
        "yields.ts",
        YIELD_AND_UNDEFINED_SOURCE,
        YIELD_AND_UNDEFINED_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 table:\n{}",
        mismatches.join("\n")
    );
}

/// The `ReduceUnion` family carries its null algebra: on ONE store shared
/// by two projects that differ only in `strictNullChecks`, `string | null`
/// reduced under each project's own policy is `null | string` for the
/// strict project and `string` for the other, whichever project asks
/// first, and a nullable-only member list erases to `null` (which wins
/// over `undefined`).
#[test]
fn reduce_union_null_policies_do_not_warm_hit() {
    for strict_first in [true, false] {
        let host = two_policy_host();
        let dispatch = ProjectSemanticDispatch::new(&host);
        let strict_policy = dispatch.nullability_for("/strict/main.ts");
        let loose_policy = dispatch.nullability_for("/loose/main.ts");
        assert_eq!(strict_policy, NullabilityPolicy::Strict);
        assert_eq!(loose_policy, NullabilityPolicy::Erased);
        let graph = host.project_type_store().semantic_graph();
        let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
        let null = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Null));
        let undefined = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Undefined));
        let reduce = |members: &[SemanticNodeId], nullability: NullabilityPolicy| match dispatch
            .execute_type_node(SemanticQueryKey::ReduceUnion {
                members: Arc::from(members),
                nullability,
            }) {
            QueryResult::Value(output) => answer_text(&host, output.value),
            other => panic!("ReduceUnion produced no value: {other:?}"),
        };
        let order = if strict_first {
            [strict_policy, loose_policy]
        } else {
            [loose_policy, strict_policy]
        };
        for nullability in order {
            let expected = match nullability {
                NullabilityPolicy::Strict => "null | string",
                NullabilityPolicy::Erased => "string",
            };
            assert_eq!(
                reduce(&[string, null], nullability),
                expected,
                "`string | null` under {nullability:?} (strict first: {strict_first})"
            );
        }
        assert_eq!(
            reduce(&[undefined, null], NullabilityPolicy::Erased),
            "null",
            "a nullable-only union is `null` when it names `null`"
        );
        assert_eq!(
            reduce(&[undefined, null], NullabilityPolicy::Strict),
            "null | undefined"
        );
    }
}

/// The canonical stamp records the algebra it was proven under: the same
/// member list minted under both algebras is two nodes, and the pre-seal
/// closure skips only a top proven under ITS OWN algebra — a strict
/// canonical `string | null` sealed for an erased key is re-closed to
/// `string`, never passed through as already canonical.
#[test]
fn canonical_stamp_is_scoped_to_its_null_algebra() {
    use crate::semantic_query::composite::CompositeOriginCategory;

    let host = VerterHost::new_standalone(HostConfig::default());
    let dispatch = ProjectSemanticDispatch::new(&host);
    let graph = host.project_type_store().semantic_graph();
    let string = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::String));
    let number = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Number));
    let null = graph.intern_node(SemanticNodeData::Primitive(PrimitiveKind::Null));

    let strict = dispatch.intern_normalized_union(&[string, number], NullabilityPolicy::Strict);
    let erased = dispatch.intern_normalized_union(&[string, number], NullabilityPolicy::Erased);
    assert_ne!(strict, erased, "the two algebras must not share one union");
    let category = |node: SemanticNodeId| match graph.node_data(node).as_deref() {
        Some(SemanticNodeData::Union(members)) => members.origin_category(),
        other => panic!("expected a union, got {other:?}"),
    };
    assert_eq!(
        category(strict),
        CompositeOriginCategory::Canonical(NullabilityPolicy::Strict)
    );
    assert_eq!(
        category(erased),
        CompositeOriginCategory::Canonical(NullabilityPolicy::Erased)
    );

    let strict_with_null =
        dispatch.intern_normalized_union(&[string, null], NullabilityPolicy::Strict);
    let result = crate::semantic_query::FlowReturnResult::new(
        graph,
        strict_with_null,
        crate::flow_completion_inventory::NormalCompletion::minted_for_fixture(false),
        None,
    );
    let sealed_strict =
        dispatch.close_flow_result_pre_seal(result.clone(), NullabilityPolicy::Strict);
    assert_eq!(sealed_strict.return_type(), strict_with_null);
    let sealed_erased = dispatch.close_flow_result_pre_seal(result, NullabilityPolicy::Erased);
    assert_eq!(answer_text(&host, sealed_erased.return_type()), "string");
}

/// `{ x: null }` is a strict subtype of `{ x: string }` only when
/// `strictNullChecks` is off, so the return join's subtype reduction
/// answers differently per policy: TypeScript 7.0.2 prints
/// `{ x: string; } | { x: null; }` strict and `{ x: string; }` off.
const SUBTYPE_REDUCTION: &str = r#"
declare const withString: { x: string };
declare const withNull: { x: null };
export function reduced(c: boolean) { if (c) return withString; return withNull; }
"#;

/// A relation the flow evaluation opens is decided under the options of
/// the function's OWN project, never the request's: a strict project's
/// caller that is the FIRST to demand a loose project's function (cold)
/// gets — and leaves cached — the loose answer, and the converse holds.
#[test]
fn relation_verdicts_follow_the_functions_own_file() {
    let host = two_policy_host();
    upsert(&host, "/loose/reduced.ts", SUBTYPE_REDUCTION);
    upsert(&host, "/strict/reduced.ts", SUBTYPE_REDUCTION);
    upsert(
        &host,
        "/strict/caller.ts",
        "import { reduced } from \"../loose/reduced\";\n\
         export function caller(c: boolean) { return reduced(c); }\n",
    );
    upsert(
        &host,
        "/loose/caller.ts",
        "import { reduced } from \"../strict/reduced\";\n\
         export function caller(c: boolean) { return reduced(c); }\n",
    );
    let loose_answer = "{ x: string }";
    let strict_answer = "{ x: null } | { x: string }";
    // The callers ask first, so each callee is inferred cold inside a
    // request whose own project has the OTHER setting.
    assert_eq!(observe(&host, "/strict/caller.ts", "caller"), loose_answer);
    assert_eq!(observe(&host, "/loose/caller.ts", "caller"), strict_answer);
    assert_eq!(observe(&host, "/loose/reduced.ts", "reduced"), loose_answer);
    assert_eq!(
        observe(&host, "/strict/reduced.ts", "reduced"),
        strict_answer
    );
}

/// A dispatch with no request context at all still decides a function's
/// relations under the function's own project: the evaluation knows the
/// file's policy, so it never falls back to the strict default.
#[test]
fn relation_verdicts_without_a_request_use_the_functions_own_file() {
    let host = two_policy_host();
    upsert(&host, "/loose/reduced.ts", SUBTYPE_REDUCTION);
    let dispatch = ProjectSemanticDispatch::new(&host);
    let key = dispatch.flow_return_key_for(&identity("/loose/reduced.ts", "reduced"));
    assert_eq!(key.context.policy.nullability, NullabilityPolicy::Erased);
    match dispatch.execute_flow_return(key) {
        FlowReturnStep::Complete(result) => {
            assert!(result.degradation().is_none(), "{:?}", result.degradation());
            assert_eq!(answer_text(&host, result.return_type()), "{ x: string }");
        }
        other => panic!("the flow return produced no value: {other:?}"),
    }
}

const STRICT_IMPLICIT_ROOT: &str = "/strict-implicit";
const LOOSE_IMPLICIT_ROOT: &str = "/loose-implicit";

/// One host carrying four tsconfig-backed projects: the 2×2 of
/// `strictNullChecks` × `noImplicitAny`, every other option equal.
fn four_policy_host() -> VerterHost {
    VerterHost::new_standalone_with_tsconfig_projects(
        HostConfig::default(),
        &[
            (STRICT_ROOT, r#"{ "compilerOptions": { "strict": true } }"#),
            (
                LOOSE_ROOT,
                r#"{ "compilerOptions": { "strict": true, "strictNullChecks": false } }"#,
            ),
            (
                STRICT_IMPLICIT_ROOT,
                r#"{ "compilerOptions": { "strict": true, "noImplicitAny": false } }"#,
            ),
            (
                LOOSE_IMPLICIT_ROOT,
                r#"{ "compilerOptions": { "strict": true, "strictNullChecks": false, "noImplicitAny": false } }"#,
            ),
        ],
    )
}

/// Every `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` row of `table` for `file` upserted into the four
/// projects of [`four_policy_host`], as the mismatch list.
fn matrix_mismatches(
    host: &VerterHost,
    file: &str,
    source: &str,
    table: &[(&str, &str, &str, &str, &str)],
) -> Vec<String> {
    let roots = [
        STRICT_ROOT,
        LOOSE_ROOT,
        STRICT_IMPLICIT_ROOT,
        LOOSE_IMPLICIT_ROOT,
    ];
    for root in roots {
        upsert(host, &format!("{root}/{file}"), source);
    }
    let mut mismatches = Vec::new();
    for (symbol, strict, loose, strict_implicit, loose_implicit) in table {
        for (root, expected) in
            roots
                .into_iter()
                .zip([strict, loose, strict_implicit, loose_implicit])
        {
            let canonical = format!("{root}/{file}");
            let observed = observe(host, &canonical, symbol);
            if observed != *expected {
                mismatches.push(format!(
                    "{canonical} `{symbol}`: expected `{expected}`, observed `{observed}`"
                ));
            }
        }
    }
    mismatches
}

/// Array literals whose elements read the frame: parameters, locals,
/// calls, nested literals, spreads, holes and const assertions.
const ARRAY_SOURCE: &str = r#"
declare function str(): string;
declare function lit(): "k";
export function arrParam(v: string) { return [v]; }
export function arrUnionParam(v: string | number) { return [v]; }
export function arrNullableParam(v: string | null) { return [v]; }
export function arrOptParam(v?: string) { return [v]; }
export function arrLitParam(v: "a") { return [v]; }
export function arrConstLocal() { const x = "a"; return [x]; }
export function arrLetLocal() { let x = "a"; return [x]; }
export function arrConstNum() { const n = 1; return [n, 2]; }
export function arrTwoParams(a: string, b: number) { return [a, b]; }
export function arrParamAndNull(v: string) { return [v, null]; }
export function arrLocalObj(v: string) { const o = { a: v }; return [o]; }
export function arrObjElem(v: string) { return [{ a: v }]; }
export function arrNested(v: string) { return [[v]]; }
export function arrInObj(v: string) { return { a: [v] }; }
export function arrCallElem() { return [str()]; }
export function arrLitCallElem() { return [lit()]; }
export function arrTernaryElem(c: boolean, v: string) { return [c ? v : 1]; }
export function arrSubtype(a: { x: string }, b: { x: string; y: number }) { return [a, b]; }
export function arrUnionSubtype(a: "x", b: string) { return [a, b]; }
export function arrUnionLitParams(a: "x" | "y", b: 1) { return [a, b]; }
export function arrBoolLits() { return [true, false]; }
export function arrAny(a: any) { return [a, 1]; }
export function arrUnknown(u: unknown) { return [u, 1]; }
export function arrFn(v: string) { return [() => v]; }
export function arrEmpty() { return []; }
export function arrNestedEmpty() { return [[]]; }
export function arrHoleOnly() { return [,]; }
export function arrHoleAndNum() { return [, 1]; }
export function arrElision(v: string) { return [v, , v]; }
export function arrVoidElem() { return [void 0, 1]; }
export function arrUndefElem(v: string) { return [undefined, v]; }
export function arrUndefParam(v: undefined) { return [v, 1]; }
export function arrLetNull() { let x = null; return [x]; }
export function arrLetNullCond(c: boolean) { let x = null; if (c) x = "s"; return [x]; }
export function arrConstNull() { const x = null; return [x]; }
export function arrDeclNull(v: null) { return [v]; }
export function arrDeclNullAndStr(v: null, s: string) { return [v, s]; }
export function arrSpread(xs: string[]) { return [...xs]; }
export function arrSpreadReadonly(xs: readonly string[]) { return [...xs]; }
export function arrSpreadTuple(t: [string, number]) { return [...t]; }
export function arrSpreadOptTuple(t: [string, number?]) { return [...t]; }
export function arrSpreadRestTuple(t: [string, ...number[]]) { return [...t]; }
export function arrSpreadMixed(v: number, xs: string[]) { return [v, ...xs]; }
export function arrSpreadLocal() { const xs = [1, 2]; return [...xs]; }
export function arrSpreadAny(x: any) { return [...x]; }
export function arrSpreadString(s: string) { return [...s]; }
export function arrSpreadUnion(xs: number[] | string[]) { return [...xs]; }
export function arrSpreadLiteralString(s: "ab") { return [...s]; }
export function arrSpreadEmpty() { return [...[]]; }
export function arrSpreadConstLit() { return [...([1, "a"] as const)]; }
export function arrLocalConstTuple() { const a = [1, 2] as const; return [a]; }
export function arrSatisfies(v: "a") { return [v] satisfies string[]; }
export function arrAsConst(v: string) { return [v] as const; }
export function arrAsConstLocal() { const x = "a"; return [x] as const; }
export function arrAsConstLet() { let x = "a"; return [x] as const; }
export function arrAsConstLits() { return ["a", 1] as const; }
export function arrAsConstEmpty() { return [] as const; }
export function arrAsConstNested(v: string) { return [[v, 1]] as const; }
export function arrAsConstObj(v: string) { return [{ a: v, b: 1 }] as const; }
export function arrAsConstNull() { return [null] as const; }
export function arrAsConstDeclNull(v: null) { return [v] as const; }
export function arrAsConstOpt(v?: string) { return [v] as const; }
export function arrAsConstHole(v: string) { return [v, , v] as const; }
export function arrAsConstSpread(t: [string, number]) { return [...t] as const; }
export function arrAsConstSpreadArr(xs: string[]) { return [...xs] as const; }
export function arrAsConstSpreadMixed(v: number, xs: string[]) { return [v, ...xs] as const; }
export function arrAsConstSpreadOpt(t: [string, number?]) { return [...t] as const; }
export function arrAsConstSpreadLabeled(t: [a: string, b?: number]) { return [...t] as const; }
export function arrAsConstSpreadRest(t: [string, ...number[]]) { return [...t] as const; }
export function objAsConstNested() { return { a: { b: 1 }, c: [1] } as const; }
"#;

/// `(symbol, strict, off)` for [`ARRAY_SOURCE`], each the checker's answer
/// on TypeScript 7.0.2 (the checker's print follows each row).
const ARRAY_TABLE: &[(&str, &str, &str)] = &[
    // string[] / string[]
    ("arrParam", "string[]", "string[]"),
    // (string | number)[] / (string | number)[]
    (
        "arrUnionParam",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // (string | null)[] / string[]
    ("arrNullableParam", "(null | string)[]", "string[]"),
    // (string | undefined)[] / string[]
    ("arrOptParam", "(string | undefined)[]", "string[]"),
    // "a"[] / "a"[]
    ("arrLitParam", "\"a\"[]", "\"a\"[]"),
    // string[] / string[]
    ("arrConstLocal", "string[]", "string[]"),
    // string[] / string[]
    ("arrLetLocal", "string[]", "string[]"),
    // number[] / number[]
    ("arrConstNum", "number[]", "number[]"),
    // (string | number)[] / (string | number)[]
    ("arrTwoParams", "(number | string)[]", "(number | string)[]"),
    // (string | null)[] / string[]
    ("arrParamAndNull", "(null | string)[]", "string[]"),
    // { a: string; }[] / { a: string; }[]
    ("arrLocalObj", "{ a: string }[]", "{ a: string }[]"),
    // { a: string; }[] / { a: string; }[]
    ("arrObjElem", "{ a: string }[]", "{ a: string }[]"),
    // string[][] / string[][]
    ("arrNested", "string[][]", "string[][]"),
    // { a: string[]; } / { a: string[]; }
    ("arrInObj", "{ a: string[] }", "{ a: string[] }"),
    // string[] / string[]
    ("arrCallElem", "string[]", "string[]"),
    // "k"[] / "k"[]
    ("arrLitCallElem", "\"k\"[]", "\"k\"[]"),
    // (string | number)[] / (string | number)[]
    (
        "arrTernaryElem",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // { x: string; }[] / { x: string; }[]
    ("arrSubtype", "{ x: string }[]", "{ x: string }[]"),
    // string[] / string[]
    ("arrUnionSubtype", "string[]", "string[]"),
    // ("x" | "y" | 1)[] / ("x" | "y" | 1)[]
    (
        "arrUnionLitParams",
        "(\"x\" | \"y\" | 1)[]",
        "(\"x\" | \"y\" | 1)[]",
    ),
    // boolean[] / boolean[]
    ("arrBoolLits", "boolean[]", "boolean[]"),
    // any[] / any[]
    ("arrAny", "any[]", "any[]"),
    // unknown[] / unknown[]
    ("arrUnknown", "unknown[]", "unknown[]"),
    // (() => string)[] / (() => string)[]
    ("arrFn", "(() => string)[]", "(() => string)[]"),
    // never[] / any[]
    ("arrEmpty", "never[]", "any[]"),
    // never[][] / any[][]
    ("arrNestedEmpty", "never[][]", "any[][]"),
    // undefined[] / any[]
    ("arrHoleOnly", "undefined[]", "any[]"),
    // (number | undefined)[] / number[]
    ("arrHoleAndNum", "(number | undefined)[]", "number[]"),
    // (string | undefined)[] / string[]
    ("arrElision", "(string | undefined)[]", "string[]"),
    // (number | undefined)[] / number[]
    ("arrVoidElem", "(number | undefined)[]", "number[]"),
    // (string | undefined)[] / string[]
    ("arrUndefElem", "(string | undefined)[]", "string[]"),
    // (number | undefined)[] / number[]
    ("arrUndefParam", "(number | undefined)[]", "number[]"),
    // null[] / any[]
    ("arrLetNull", "null[]", "any[]"),
    // (string | null)[] / string[]
    ("arrLetNullCond", "(null | string)[]", "string[]"),
    // null[] / any[]
    ("arrConstNull", "null[]", "any[]"),
    // null[] / null[]
    ("arrDeclNull", "null[]", "null[]"),
    // (string | null)[] / string[]
    ("arrDeclNullAndStr", "(null | string)[]", "string[]"),
    // string[] / string[]
    ("arrSpread", "string[]", "string[]"),
    // string[] / string[]
    ("arrSpreadReadonly", "string[]", "string[]"),
    // (string | number)[] / (string | number)[]
    (
        "arrSpreadTuple",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // (string | number | undefined)[] / (string | number)[]
    (
        "arrSpreadOptTuple",
        "(number | string | undefined)[]",
        "(number | string)[]",
    ),
    // (string | number)[] / (string | number)[]
    (
        "arrSpreadRestTuple",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // (string | number)[] / (string | number)[]
    (
        "arrSpreadMixed",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // number[] / number[]
    ("arrSpreadLocal", "number[]", "number[]"),
    // any[] / any[]
    ("arrSpreadAny", "any[]", "any[]"),
    // string[] / string[]
    ("arrSpreadString", "string[]", "string[]"),
    // (string | number)[] / (string | number)[]
    (
        "arrSpreadUnion",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // string[] / string[]
    ("arrSpreadLiteralString", "string[]", "string[]"),
    // never[] / any[]
    ("arrSpreadEmpty", "never[]", "any[]"),
    // ("a" | 1)[] / ("a" | 1)[]
    ("arrSpreadConstLit", "(\"a\" | 1)[]", "(\"a\" | 1)[]"),
    // (readonly [1, 2])[] / (readonly [1, 2])[]
    (
        "arrLocalConstTuple",
        "(readonly [1, 2])[]",
        "(readonly [1, 2])[]",
    ),
    // "a"[] / "a"[]
    ("arrSatisfies", "\"a\"[]", "\"a\"[]"),
    // readonly [string] / readonly [string]
    ("arrAsConst", "readonly [string]", "readonly [string]"),
    // readonly ["a"] / readonly ["a"]
    ("arrAsConstLocal", "readonly [\"a\"]", "readonly [\"a\"]"),
    // readonly [string] / readonly [string]
    ("arrAsConstLet", "readonly [string]", "readonly [string]"),
    // readonly ["a", 1] / readonly ["a", 1]
    (
        "arrAsConstLits",
        "readonly [\"a\", 1]",
        "readonly [\"a\", 1]",
    ),
    // readonly [] / readonly []
    ("arrAsConstEmpty", "readonly []", "readonly []"),
    // readonly [readonly [string, 1]] / readonly [readonly [string, 1]]
    (
        "arrAsConstNested",
        "readonly [readonly [string, 1]]",
        "readonly [readonly [string, 1]]",
    ),
    // readonly [{ readonly a: string; readonly b: 1; }] / readonly [{ readonly a: string; readonly b: 1; }]
    (
        "arrAsConstObj",
        "readonly [{ readonly a: string; readonly b: 1 }]",
        "readonly [{ readonly a: string; readonly b: 1 }]",
    ),
    // readonly [null] / readonly [any]
    ("arrAsConstNull", "readonly [null]", "readonly [any]"),
    // readonly [null] / readonly [null]
    ("arrAsConstDeclNull", "readonly [null]", "readonly [null]"),
    // readonly [string | undefined] / readonly [string]
    (
        "arrAsConstOpt",
        "readonly [string | undefined]",
        "readonly [string]",
    ),
    // readonly [string, undefined, string] / readonly [string, any, string]
    (
        "arrAsConstHole",
        "readonly [string, undefined, string]",
        "readonly [string, any, string]",
    ),
    // readonly [string, number] / readonly [string, number]
    (
        "arrAsConstSpread",
        "readonly [string, number]",
        "readonly [string, number]",
    ),
    // readonly string[] / readonly string[]
    (
        "arrAsConstSpreadArr",
        "readonly string[]",
        "readonly string[]",
    ),
    // readonly [number, ...string[]] / readonly [number, ...string[]]
    (
        "arrAsConstSpreadMixed",
        "readonly [number, ...string[]]",
        "readonly [number, ...string[]]",
    ),
    // readonly [string, (number | undefined)?] / readonly [string, number?]
    (
        "arrAsConstSpreadOpt",
        "readonly [string, (number | undefined)?]",
        "readonly [string, number?]",
    ),
    // readonly [a: string, b?: number | undefined] / readonly [a: string, b?: number]
    (
        "arrAsConstSpreadLabeled",
        "readonly [a: string, b?: number | undefined]",
        "readonly [a: string, b?: number]",
    ),
    // readonly [string, ...number[]] / readonly [string, ...number[]]
    (
        "arrAsConstSpreadRest",
        "readonly [string, ...number[]]",
        "readonly [string, ...number[]]",
    ),
    // { readonly a: { readonly b: 1; }; readonly c: readonly [1]; } / { readonly a: { readonly b: 1; }; readonly c: readonly [1]; }
    (
        "objAsConstNested",
        "{ readonly a: { readonly b: 1 }; readonly c: readonly [1] }",
        "{ readonly a: { readonly b: 1 }; readonly c: readonly [1] }",
    ),
];

/// An array literal's elements are flow expressions: `[v]` over a
/// parameter or a local reads the frame's value. Without a const
/// assertion the value is `E[]`, `E` the subtype-reduced union of the
/// elements read at a mutable location (a fresh literal or a read of a
/// widening-literal local widens), a spread contributing its source's
/// element type (a tuple's optional element with `undefined` under
/// `strictNullChecks`), a hole the widening `undefined`, and an empty
/// literal `never[]` (`any[]` with `strictNullChecks` off). Under `as
/// const` it is the readonly tuple of the pinned element values, nested
/// literals const too and a spread spliced in. With `strictNullChecks` off
/// a widening nullable element vanishes beside any other element and an
/// element union made only of them is `any`. Each row matches its own
/// project's TypeScript 7.0.2 answer.
#[test]
fn array_literals_infer_from_their_element_values() {
    let host = two_policy_host();
    let mismatches = table_mismatches(&host, "arrays.ts", ARRAY_SOURCE, ARRAY_TABLE);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 table:\n{}",
        mismatches.join("\n")
    );
}

/// Generators yielding parameters, locals and literals, in every
/// statement form a yield can sit in except a loop.
const YIELD_SOURCE: &str = r#"
interface Generator<T, TReturn, TNext> {}
interface AsyncGenerator<T, TReturn, TNext> {}
export function* genParam(v: string) { yield v; }
export function* genUnionParam(v: string | number) { yield v; }
export function* genNullableParam(v: string | null) { yield v; }
export function* genOptParam(v?: string) { yield v; }
export function* genLitParam(v: "a") { yield v; }
export function* genConstLocal() { const x = "a"; yield x; }
export function* genLetLocal() { let x = "a"; yield x; }
export function* genConstNum() { const n = 1; yield n; }
export function* genConstTwo() { const a = 1; const b = 2; yield a; yield b; }
export function* genMixed(v: string) { const n = 1; yield v; yield n; }
export function* genMixedLit(v: string) { yield v; yield 1; }
export function* genLocalObj(v: string) { const o = { a: v }; yield o; }
export function* genObjYield(v: string) { yield { a: v }; }
export function* genArrYield(v: string) { yield [v]; }
export function* genLocalArr() { const a = [1]; yield a; }
export function* genLetReassigned(c: boolean) { let x: string | number = 1; if (c) x = "s"; yield x; }
export function* genNarrowed(v: string | number) { if (typeof v === "string") yield v; }
export function* genLetNull() { let x = null; yield x; }
export function* genLetNullCond(c: boolean) { let x = null; if (c) x = "s"; yield x; }
export function* genConstNull() { const x = null; yield x; }
export function* genDeclLocal() { const x: "a" = "a"; yield x; }
export function* genLetAnnot() { let x: string = "a"; yield x; }
export function* genAsConstLocal() { const x = "a" as const; yield x; }
export function* genParamReturn(v: string) { yield v; return v; }
export function* genLocalReturn() { const x = 1; yield x; return x; }
export function* genAfterReturn(c: boolean, v: string) { if (c) return 1; yield v; }
export function* genIfElse(c: boolean, v: string) { if (c) { yield v; } else { yield 1; } }
export function* genSwitch(k: number, v: string) { switch (k) { case 1: yield v; break; default: yield 1; } }
export function* genTry(v: string) { try { yield v; } catch { yield 1; } }
export function* genTryFinally(v: string) { try { yield v; } finally { yield 1; } }
export function* genLabeled(v: string, c: boolean) { lbl: { if (c) break lbl; yield v; } }
export function* genBlock(v: string) { { const x = v; yield x; } }
export function* genTernaryYield(c: boolean, v: string) { yield c ? v : 1; }
export function* genNestedGen(v: string) { const inner = function* () { yield 1; }; yield v; }
export async function* agenParam(v: string) { yield v; }
export async function* agenLocal() { const x = 1; yield x; }
"#;

/// `(symbol, strict, off)` for [`YIELD_SOURCE`], each the checker's answer
/// on TypeScript 7.0.2 (the checker's print follows each row).
const YIELD_TABLE: &[(&str, &str, &str)] = &[
    // Generator<string, void, unknown> / Generator<string, void, unknown>
    (
        "genParam",
        "Generator<string, void, unknown>",
        "Generator<string, void, unknown>",
    ),
    // Generator<string | number, void, unknown> / Generator<string | number, void, unknown>
    (
        "genUnionParam",
        "Generator<number | string, void, unknown>",
        "Generator<number | string, void, unknown>",
    ),
    // Generator<string | null, void, unknown> / Generator<string, void, unknown>
    (
        "genNullableParam",
        "Generator<null | string, void, unknown>",
        "Generator<string, void, unknown>",
    ),
    // Generator<string | undefined, void, unknown> / Generator<string, void, unknown>
    (
        "genOptParam",
        "Generator<string | undefined, void, unknown>",
        "Generator<string, void, unknown>",
    ),
    // Generator<"a", void, unknown> / Generator<"a", void, unknown>
    (
        "genLitParam",
        "Generator<\"a\", void, unknown>",
        "Generator<\"a\", void, unknown>",
    ),
    // Generator<string, void, unknown> / Generator<string, void, unknown>
    (
        "genConstLocal",
        "Generator<string, void, unknown>",
        "Generator<string, void, unknown>",
    ),
    // Generator<string, void, unknown> / Generator<string, void, unknown>
    (
        "genLetLocal",
        "Generator<string, void, unknown>",
        "Generator<string, void, unknown>",
    ),
    // Generator<number, void, unknown> / Generator<number, void, unknown>
    (
        "genConstNum",
        "Generator<number, void, unknown>",
        "Generator<number, void, unknown>",
    ),
    // Generator<1 | 2, void, unknown> / Generator<1 | 2, void, unknown>
    (
        "genConstTwo",
        "Generator<1 | 2, void, unknown>",
        "Generator<1 | 2, void, unknown>",
    ),
    // Generator<string | 1, void, unknown> / Generator<string | 1, void, unknown>
    (
        "genMixed",
        "Generator<1 | string, void, unknown>",
        "Generator<1 | string, void, unknown>",
    ),
    // Generator<string | 1, void, unknown> / Generator<string | 1, void, unknown>
    (
        "genMixedLit",
        "Generator<1 | string, void, unknown>",
        "Generator<1 | string, void, unknown>",
    ),
    // Generator<{ a: string; }, void, unknown> / Generator<{ a: string; }, void, unknown>
    (
        "genLocalObj",
        "Generator<{ a: string }, void, unknown>",
        "Generator<{ a: string }, void, unknown>",
    ),
    // Generator<{ a: string; }, void, unknown> / Generator<{ a: string; }, void, unknown>
    (
        "genObjYield",
        "Generator<{ a: string }, void, unknown>",
        "Generator<{ a: string }, void, unknown>",
    ),
    // Generator<string[], void, unknown> / Generator<string[], void, unknown>
    (
        "genArrYield",
        "Generator<string[], void, unknown>",
        "Generator<string[], void, unknown>",
    ),
    // Generator<number[], void, unknown> / Generator<number[], void, unknown>
    (
        "genLocalArr",
        "Generator<number[], void, unknown>",
        "Generator<number[], void, unknown>",
    ),
    // Generator<string | number, void, unknown> / Generator<string | number, void, unknown>
    (
        "genLetReassigned",
        "Generator<number | string, void, unknown>",
        "Generator<number | string, void, unknown>",
    ),
    // Generator<string, void, unknown> / Generator<string, void, unknown>
    (
        "genNarrowed",
        "Generator<string, void, unknown>",
        "Generator<string, void, unknown>",
    ),
    // Generator<null, void, unknown> / Generator<any, void, unknown>
    (
        "genLetNull",
        "Generator<null, void, unknown>",
        "Generator<any, void, unknown>",
    ),
    // Generator<string | null, void, unknown> / Generator<string, void, unknown>
    (
        "genLetNullCond",
        "Generator<null | string, void, unknown>",
        "Generator<string, void, unknown>",
    ),
    // Generator<null, void, unknown> / Generator<any, void, unknown>
    (
        "genConstNull",
        "Generator<null, void, unknown>",
        "Generator<any, void, unknown>",
    ),
    // Generator<"a", void, unknown> / Generator<"a", void, unknown>
    (
        "genDeclLocal",
        "Generator<\"a\", void, unknown>",
        "Generator<\"a\", void, unknown>",
    ),
    // Generator<string, void, unknown> / Generator<string, void, unknown>
    (
        "genLetAnnot",
        "Generator<string, void, unknown>",
        "Generator<string, void, unknown>",
    ),
    // Generator<"a", void, unknown> / Generator<"a", void, unknown>
    (
        "genAsConstLocal",
        "Generator<\"a\", void, unknown>",
        "Generator<\"a\", void, unknown>",
    ),
    // Generator<string, string, unknown> / Generator<string, string, unknown>
    (
        "genParamReturn",
        "Generator<string, string, unknown>",
        "Generator<string, string, unknown>",
    ),
    // Generator<number, number, unknown> / Generator<number, number, unknown>
    (
        "genLocalReturn",
        "Generator<number, number, unknown>",
        "Generator<number, number, unknown>",
    ),
    // Generator<string, 1 | undefined, unknown> / Generator<string, number, unknown>
    (
        "genAfterReturn",
        "Generator<string, 1 | undefined, unknown>",
        "Generator<string, number, unknown>",
    ),
    // Generator<string | 1, void, unknown> / Generator<string | 1, void, unknown>
    (
        "genIfElse",
        "Generator<1 | string, void, unknown>",
        "Generator<1 | string, void, unknown>",
    ),
    // Generator<string | 1, void, unknown> / Generator<string | 1, void, unknown>
    (
        "genSwitch",
        "Generator<1 | string, void, unknown>",
        "Generator<1 | string, void, unknown>",
    ),
    // Generator<string | 1, void, unknown> / Generator<string | 1, void, unknown>
    (
        "genTry",
        "Generator<1 | string, void, unknown>",
        "Generator<1 | string, void, unknown>",
    ),
    // Generator<string | 1, void, unknown> / Generator<string | 1, void, unknown>
    (
        "genTryFinally",
        "Generator<1 | string, void, unknown>",
        "Generator<1 | string, void, unknown>",
    ),
    // Generator<string, void, unknown> / Generator<string, void, unknown>
    (
        "genLabeled",
        "Generator<string, void, unknown>",
        "Generator<string, void, unknown>",
    ),
    // Generator<string, void, unknown> / Generator<string, void, unknown>
    (
        "genBlock",
        "Generator<string, void, unknown>",
        "Generator<string, void, unknown>",
    ),
    // Generator<string | 1, void, unknown> / Generator<string | 1, void, unknown>
    (
        "genTernaryYield",
        "Generator<1 | string, void, unknown>",
        "Generator<1 | string, void, unknown>",
    ),
    // Generator<string, void, unknown> / Generator<string, void, unknown>
    (
        "genNestedGen",
        "Generator<string, void, unknown>",
        "Generator<string, void, unknown>",
    ),
    // AsyncGenerator<string, void, unknown> / AsyncGenerator<string, void, unknown>
    (
        "agenParam",
        "AsyncGenerator<string, void, unknown>",
        "AsyncGenerator<string, void, unknown>",
    ),
    // AsyncGenerator<number, void, unknown> / AsyncGenerator<number, void, unknown>
    (
        "agenLocal",
        "AsyncGenerator<number, void, unknown>",
        "AsyncGenerator<number, void, unknown>",
    ),
];

/// A statement-position `yield x` is a value root exactly as a return
/// argument is: the demand reaches the local it reads, the narrowing its
/// branch establishes and the object it builds, and the yield join widens
/// a lone fresh literal (a `const` local's included) as the return join
/// does. Each row matches its own project's TypeScript 7.0.2 answer.
#[test]
fn generator_yields_of_parameters_and_locals_join_like_returns() {
    let host = two_policy_host();
    let mismatches = table_mismatches(&host, "yield-values.ts", YIELD_SOURCE, YIELD_TABLE);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 table:\n{}",
        mismatches.join("\n")
    );
}

/// A yield the join does not model — inside a loop, nested in another
/// expression, or a `yield*` delegation — never leaves a partial yield
/// type. TypeScript 7.0.2 types every one of these (`genWhile`,
/// `genNested`, `genInCall` and `genDelegate` are `Generator<string,
/// void, unknown>`; `genWhileBare` is `Generator<undefined, void,
/// unknown>`, `Generator<any, void, unknown>` with `strictNullChecks`
/// off); the lane answers no value for a loop, as it does for a
/// return-bearing one, and a typed gap for the rest.
#[test]
fn yields_the_join_does_not_model_never_publish_a_partial_yield_type() {
    const SOURCE: &str = r#"
interface Generator<T, TReturn, TNext> {}
declare function touch(v: unknown): void;
export function* genWhile(v: string, c: boolean) { while (c) { yield v; } }
export function* genWhileBare(c: boolean) { while (c) { yield; } }
export function* genNested(v: string) { const r = yield v; }
export function* genInCall(v: string) { touch(yield v); }
export function* genDelegate(xs: string[]) { yield* xs; }
"#;
    let host = two_policy_host();
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        let canonical = format!("{root}/yield-gaps.ts");
        upsert(&host, &canonical, SOURCE);
        for symbol in ["genWhile", "genWhileBare"] {
            assert_eq!(
                observe(&host, &canonical, symbol),
                "<no value: Failure(Unsupported(Loop))>",
                "{canonical} `{symbol}`"
            );
        }
        for symbol in ["genNested", "genInCall", "genDelegate"] {
            let observed = observe(&host, &canonical, symbol);
            assert!(
                observed.ends_with("[degraded Some(FlowGap(UnmodeledExpression))]"),
                "{canonical} `{symbol}`: {observed}"
            );
        }
    }
}

/// `void` values in every value position, and `void` statements whose
/// operand writes or calls.
const VOID_SOURCE: &str = r#"
interface Generator<T, TReturn, TNext> {}
declare function declared(): string;
declare const anyCallee: any;
declare function assertStr(v: unknown): asserts v is string;
export function voidCall() { return void declared(); }
export function voidAnyCall() { return void anyCallee(); }
export function voidUnknownCall(f: Function) { return void f(); }
export function voidParam(v: string) { return void v; }
export function voidLocalCall() { const g = () => 1; return void g(); }
export function voidAssertCall(x: string | number) { return void assertStr(x); }
export function voidNew() { return void new Date(); }
export function voidAndNum(c: boolean) { if (c) return 1; return void declared(); }
export function voidAndStr(c: boolean, s: string) { return c ? s : void declared(); }
export function voidInObj() { return { a: void declared() }; }
export function voidInArr() { return [void declared()]; }
export function voidAssign() { let x: string | number = 1; return void (x = "s"); }
export function voidInFinally() { let x: string | number = 1; try { return void (x = "s"); } finally { } }
export function* genVoid() { yield void declared(); }
export function voidConst() { const y = void declared(); return y; }
export function voidLet(c: boolean) { let y = void declared(); if (c) y = undefined; return y; }
export function voidAssignThenRead() { let x: string | number = 1; void (x = "s"); return x; }
export function voidAssertThenRead(x: string | number) { void assertStr(x); return x; }
export function constVoidAssertThenRead(x: string | number) { const y = void assertStr(x); return x; }
export function voidAnyThenRead(x: string | number) { void anyCallee(x); return x; }
export function stmtAssertThenRead(x: string | number) { assertStr(x); return x; }
"#;

/// `(symbol, strict, off)` for [`VOID_SOURCE`], each the checker's answer
/// on TypeScript 7.0.2 (the checker's print follows each row).
const VOID_TABLE: &[(&str, &str, &str)] = &[
    // undefined / any
    ("voidCall", "undefined", "any"),
    // undefined / any
    ("voidAnyCall", "undefined", "any"),
    // undefined / any
    ("voidUnknownCall", "undefined", "any"),
    // undefined / any
    ("voidParam", "undefined", "any"),
    // undefined / any
    ("voidLocalCall", "undefined", "any"),
    // undefined / any
    ("voidAssertCall", "undefined", "any"),
    // undefined / any
    ("voidNew", "undefined", "any"),
    // 1 | undefined / number
    ("voidAndNum", "1 | undefined", "number"),
    // string | undefined / string
    ("voidAndStr", "string | undefined", "string"),
    // { a: undefined; } / { a: any; }
    ("voidInObj", "{ a: undefined }", "{ a: any }"),
    // undefined[] / any[]
    ("voidInArr", "undefined[]", "any[]"),
    // undefined / any
    ("voidAssign", "undefined", "any"),
    // undefined / any
    ("voidInFinally", "undefined", "any"),
    // Generator<undefined, void, unknown> / Generator<any, void, unknown>
    (
        "genVoid",
        "Generator<undefined, void, unknown>",
        "Generator<any, void, unknown>",
    ),
    // undefined / any
    ("voidConst", "undefined", "any"),
    // undefined / any
    ("voidLet", "undefined", "any"),
    // string / string
    ("voidAssignThenRead", "string", "string"),
    // string | number / string | number
    ("voidAssertThenRead", "number | string", "number | string"),
    // string | number / string | number
    (
        "constVoidAssertThenRead",
        "number | string",
        "number | string",
    ),
    // string | number / string | number
    ("voidAnyThenRead", "number | string", "number | string"),
    // string / string
    ("stmtAssertThenRead", "string", "string"),
];

/// A `void` value is the widening `undefined` whatever its operand, so a
/// callee the lane cannot certify changes nothing about it. The operand
/// still runs, and only its effects the checker can observe are kept: a
/// whole-binding write in a `void` statement retypes the binding, while a
/// call that is the operand is never entered into control flow — the
/// checker binds only a statement's own call or a comma's left operand —
/// so an `asserts` callee there narrows nothing. Each row matches its own
/// project's TypeScript 7.0.2 answer.
#[test]
fn void_values_are_undefined_whatever_the_operand() {
    let host = two_policy_host();
    let mismatches = table_mismatches(&host, "voids.ts", VOID_SOURCE, VOID_TABLE);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 table:\n{}",
        mismatches.join("\n")
    );
}

/// Unannotated `let` / `var` declarations with no initializer or a
/// nullish one, read directly and through joins of widening and declared
/// nullable writes.
const AUTO_TYPED_SOURCE: &str = r#"
interface Generator<T, TReturn, TNext> {}
export function letNull() { let x = null; return x; }
export function letUndef() { let x = undefined; return x; }
export function letParenNull() { let x = (null); return x; }
export function varNull() { var x = null; return x; }
export function letVoid() { let x = void 0; return x; }
export function letNullCond(c: boolean) { let x = null; if (c) x = "s"; return x; }
export function letUndefCond(c: boolean) { let x = undefined; if (c) x = 1; return x; }
export function varNullCond(c: boolean) { var x = null; if (c) x = "s"; return x; }
export function letNullAssigned() { let x = null; x = "s"; return x; }
export function letNullRead() { let x = null; x = "s"; const y = x; return y; }
export function letNullNull(c: boolean) { let x = null; if (c) x = null; return x; }
export function letNullDecl(c: boolean, v: null) { let x = null; if (c) x = v; return x; }
export function noInit() { let x; return x; }
export function noInitAssigned() { let x; x = "s"; return x; }
export function noInitCond(c: boolean) { let x; if (c) x = "s"; return x; }
export function noInitNull() { let x; x = null; return x; }
export function noInitBoth(c: boolean) { let x; if (c) x = "s"; else x = 1; return x; }
export function varNoInit(c: boolean) { var x; if (c) x = 1; return x; }
export function constFromLet() { let x = null; const y = x; return y; }
export function returnTernaryLocal(c: boolean) { let x = null; return c ? x : undefined; }
export function objFromLet() { let n = null; return { a: n }; }
export function arrFromLet() { let n = null; return [n]; }
export function letTernary(c: boolean) { let x = c ? null : undefined; return x; }
export function* genLetNullCond(c: boolean) { let x = null; if (c) x = "s"; yield x; }
export function autoDeclElseWiden(c: boolean, v: null) { let x = null; if (c) { x = v; } else { x = null; } return x; }
export function autoWidenElseDecl(c: boolean, v: null) { let x = null; if (c) { x = null; } else { x = v; } return x; }
export function autoDeclThenWiden(c: boolean, d: boolean, v: null) { let x = null; if (c) x = v; if (d) x = null; return x; }
export function autoNoInitDecl(c: boolean, v: null) { let x; if (c) x = v; else x = null; return x; }
export function autoNoInitWiden(c: boolean) { let x; if (c) x = null; return x; }
export function autoNoInitWidenBoth(c: boolean) { let x; if (c) x = null; else x = undefined; return x; }
export function autoUndefDecl(c: boolean, v: undefined) { let x = undefined; if (c) x = v; else x = undefined; return x; }
export function autoTernaryAssign(c: boolean, v: null) { let x = null; x = c ? v : null; return x; }
export function autoDeclThenReturnEarly(c: boolean, v: null) { let x = null; if (c) { x = v; return x; } x = null; return x; }
export function autoDeclReadInBranch(c: boolean, v: null) { let x = null; if (c) { x = v; } else { x = null; } const y = x; return y; }
export function autoObj(c: boolean, v: null) { let x = null; if (c) { x = v; } else { x = null; } return { a: x }; }
export function autoArr(c: boolean, v: null) { let x = null; if (c) { x = v; } else { x = null; } return [x]; }
export function* autoGen(c: boolean, v: null) { let x = null; if (c) { x = v; } else { x = null; } yield x; }
export function autoWidenAfterDeclPath(c: boolean, v: null) { let x = null; if (c) { x = null; } else { x = v; x = null; } return x; }
export function autoSwitch(k: number, v: null) { let x = null; switch (k) { case 1: x = v; break; case 2: x = null; break; } return x; }
export function autoArrMixed(c: boolean) { let x = null; if (c) x = null; return [x, 1]; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`AUTO_TYPED_SOURCE`], each the checker's answer on
/// TypeScript 7.0.2 (the checker's four prints follow each row).
const AUTO_TYPED_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // null / any / null / any
    ("letNull", "null", "any", "null", "any"),
    // undefined / any / undefined / any
    ("letUndef", "undefined", "any", "undefined", "any"),
    // null / any / null / any
    ("letParenNull", "null", "any", "null", "any"),
    // null / any / null / any
    ("varNull", "null", "any", "null", "any"),
    // undefined / any / undefined / any
    ("letVoid", "undefined", "any", "undefined", "any"),
    // string | null / string / null / any
    // (TS2322 without `noImplicitAny`: the write is rejected against the
    // declared type, whose read stands)
    ("letNullCond", "null | string", "string", "null", "any"),
    // number | undefined / number / undefined / any
    // (TS2322 without `noImplicitAny`: the write is rejected against the
    // declared type, whose read stands)
    (
        "letUndefCond",
        "number | undefined",
        "number",
        "undefined",
        "any",
    ),
    // string | null / string / null / any
    // (TS2322 without `noImplicitAny`: the write is rejected against the
    // declared type, whose read stands)
    ("varNullCond", "null | string", "string", "null", "any"),
    // string / string / null / any
    // (TS2322 without `noImplicitAny`: the write is rejected against the
    // declared type, whose read stands)
    ("letNullAssigned", "string", "string", "null", "any"),
    // string / string / null / any
    // (TS2322 without `noImplicitAny`: the write is rejected against the
    // declared type, whose read stands)
    ("letNullRead", "string", "string", "null", "any"),
    // null / any / null / any
    ("letNullNull", "null", "any", "null", "any"),
    // null / null / null / any
    ("letNullDecl", "null", "null", "null", "any"),
    // undefined / undefined / any / any
    ("noInit", "undefined", "undefined", "any", "any"),
    // string / string / any / any
    ("noInitAssigned", "string", "string", "any", "any"),
    // string | undefined / string / any / any
    ("noInitCond", "string | undefined", "string", "any", "any"),
    // null / any / any / any
    ("noInitNull", "null", "any", "any", "any"),
    // string | number / string | number / any / any
    (
        "noInitBoth",
        "number | string",
        "number | string",
        "any",
        "any",
    ),
    // number | undefined / number / any / any
    ("varNoInit", "number | undefined", "number", "any", "any"),
    // null / any / null / any
    ("constFromLet", "null", "any", "null", "any"),
    // null | undefined / any / null | undefined / any
    (
        "returnTernaryLocal",
        "null | undefined",
        "any",
        "null | undefined",
        "any",
    ),
    // { a: null; } / { a: any; } / { a: null; } / { a: any; }
    (
        "objFromLet",
        "{ a: null }",
        "{ a: any }",
        "{ a: null }",
        "{ a: any }",
    ),
    // null[] / any[] / null[] / any[]
    ("arrFromLet", "null[]", "any[]", "null[]", "any[]"),
    // null | undefined / any / null | undefined / any
    (
        "letTernary",
        "null | undefined",
        "any",
        "null | undefined",
        "any",
    ),
    // Generator<string | null, void, unknown> / Generator<string, void, unknown> / Generator<null, void, unknown> / Generator<any, void, unknown>
    // (TS2322 without `noImplicitAny`: the write is rejected against the
    // declared type, whose read stands)
    (
        "genLetNullCond",
        "Generator<null | string, void, unknown>",
        "Generator<string, void, unknown>",
        "Generator<null, void, unknown>",
        "Generator<any, void, unknown>",
    ),
    // null / null / null / any
    ("autoDeclElseWiden", "null", "null", "null", "any"),
    // null / null / null / any
    ("autoWidenElseDecl", "null", "null", "null", "any"),
    // null / null / null / any
    ("autoDeclThenWiden", "null", "null", "null", "any"),
    // null / null / any / any
    ("autoNoInitDecl", "null", "null", "any", "any"),
    // null | undefined / null / any / any
    ("autoNoInitWiden", "null | undefined", "null", "any", "any"),
    // null | undefined / any / any / any
    (
        "autoNoInitWidenBoth",
        "null | undefined",
        "any",
        "any",
        "any",
    ),
    // undefined / undefined / undefined / any
    (
        "autoUndefDecl",
        "undefined",
        "undefined",
        "undefined",
        "any",
    ),
    // null / null / null / any
    ("autoTernaryAssign", "null", "null", "null", "any"),
    // null / null / null / any
    ("autoDeclThenReturnEarly", "null", "null", "null", "any"),
    // null / null / null / any
    ("autoDeclReadInBranch", "null", "null", "null", "any"),
    // { a: null; } / { a: null; } / { a: null; } / { a: any; }
    (
        "autoObj",
        "{ a: null }",
        "{ a: null }",
        "{ a: null }",
        "{ a: any }",
    ),
    // null[] / null[] / null[] / any[]
    ("autoArr", "null[]", "null[]", "null[]", "any[]"),
    // Generator<null, void, unknown> / Generator<null, void, unknown> / Generator<null, void, unknown> / Generator<any, void, unknown>
    (
        "autoGen",
        "Generator<null, void, unknown>",
        "Generator<null, void, unknown>",
        "Generator<null, void, unknown>",
        "Generator<any, void, unknown>",
    ),
    // null / any / null / any
    ("autoWidenAfterDeclPath", "null", "any", "null", "any"),
    // null / null / null / any
    ("autoSwitch", "null", "null", "null", "any"),
    // (number | null)[] / number[] / (number | null)[] / any[]
    (
        "autoArrMixed",
        "(null | number)[]",
        "number[]",
        "(null | number)[]",
        "any[]",
    ),
];

/// `noImplicitAny` and `strictNullChecks` together decide an unannotated
/// `let` / `var` with no initializer or a bare `null` / `undefined` one.
/// Under `noImplicitAny` it is AUTO-TYPED: it holds `undefined` (no
/// initializer) or its initializer's value until a write retypes it, and
/// with `strictNullChecks` off a bare nullish value is the WIDENING
/// nullable type, read per path — where a widening path meets a path
/// holding a declared nullable or the initializer-less `undefined`, the
/// value is that non-widening one, and only an all-widening join widens to
/// `any`. Without `noImplicitAny` the variable is DECLARED as its
/// initializer's widened type (`any` with no initializer, `null` /
/// `undefined` when strict and `any` when not), which no write retypes; a
/// `void 0` or conditional nullable initializer is declared that way
/// under both. Each cell matches its own project's TypeScript 7.0.2 answer.
#[test]
fn implicit_any_and_auto_typed_locals_follow_both_policies() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(&host, "auto-typed.ts", AUTO_TYPED_SOURCE, AUTO_TYPED_TABLE);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// The flow-return key states the function's own project's
/// `noImplicitAny`, read the same way its `strictNullChecks` is: two
/// projects that differ only in it key the same function differently.
#[test]
fn flow_policy_states_the_functions_own_no_implicit_any() {
    let host = four_policy_host();
    let source = "export function f() { let x; return x; }\n";
    let dispatch = ProjectSemanticDispatch::new(&host);
    let mut keys = Vec::new();
    for (root, no_implicit_any) in [(STRICT_ROOT, true), (STRICT_IMPLICIT_ROOT, false)] {
        let canonical = format!("{root}/policy.ts");
        upsert(&host, &canonical, source);
        let key = dispatch.flow_return_key_for(&identity(&canonical, "f"));
        assert_eq!(
            key.context.policy.no_implicit_any, no_implicit_any,
            "{canonical}"
        );
        assert_eq!(key.context.policy.nullability, NullabilityPolicy::Strict);
        keys.push(key);
    }
    assert_ne!(keys[0].context.policy, keys[1].context.policy);
}

/// Generic applications and mapped types reached as flow values: alias
/// applications, calls returning them, and member reads through them.
const SHARED_APPLICATION_SOURCE: &str = r#"
type Partial<T> = { [P in keyof T]?: T[P] };
type Pick<T, K extends keyof T> = { [P in K]: T[P] };
type Readonly<T> = { readonly [P in keyof T]: T[P] };
type Record<K extends keyof any, T> = { [P in K]: T };
type Opt<T> = T | undefined;
type OptNum<T> = T | undefined | number;
type Wrap<T> = { v: T };
type Nul<T> = T | null;
type Pair<A, B> = A | B | undefined;
type Three<T> = T | boolean | undefined;
type Plain<T> = T | number;
interface P { a: string; b: number | null }
declare function optOf<T>(v: T): Opt<T>;
declare function wrapOf<T>(v: T): Wrap<T | null>;
declare function pairOf<T>(v: T): [T, T | null];
declare function idOf<T>(v: T): T;
export function optParam(v: Opt<string>) { return v; }
export function optNested(v: Opt<Opt<string>>) { return v; }
export function optRead(v: { a: Opt<string> }) { return v.a; }
export function optInObj(v: { a: Opt<string> }) { return v; }
export function optInArr(v: Opt<string>[]) { return v; }
export function optNumParam(v: OptNum<string>) { return v; }
export function optCall(v: string) { return optOf(v); }
export function optCallInArr(v: string) { return [optOf(v)]; }
export function optCallInObj(v: string) { return { a: optOf(v) }; }
export function wrapCall(v: string) { return wrapOf(v); }
export function pairCall(v: string) { return pairOf(v); }
export function idCall(v: string | null) { return idOf(v); }
export function partialWhole(q: Partial<P>) { return q; }
export function partialRead(p: Partial<P>) { return p.a; }
export function partialReadB(p: Partial<P>) { return p.b; }
export function pickRead(p: Pick<P, "b">) { return p.b; }
export function pickWhole(p: Pick<P, "b">) { return p; }
export function readonlyRead(p: Readonly<P>) { return p.b; }
export function recordRead(r: Record<"k", string | null>) { return r.k; }
export function recordWhole(r: Record<"k", string | null>) { return r; }
export function optNumRead(v: { a: OptNum<string> }) { return v.a; }
export function optUnionArgAlias(v: Opt<string | number>) { return v; }
export function optNumAlias(v: OptNum<string>) { return v; }
export function nulUnionArg(v: Nul<string | number>) { return v; }
export function pairAlias(v: Pair<string, number>) { return v; }
export function threeAlias(v: Three<string>) { return v; }
export function optLiteralUnionArg(v: Opt<"a" | "b">) { return v; }
export function plainAlias(v: Plain<string>) { return v; }
export function plainUnionArg(v: Plain<string | boolean>) { return v; }
export function optNullableArg(v: Opt<string | null>) { return v; }
export function optNumUnionArg(v: OptNum<string | boolean>) { return v; }
"#;

/// `(symbol, strict, off)` for [`SHARED_APPLICATION_SOURCE`], each the
/// checker's answer on TypeScript 7.0.2 (the checker's print follows each
/// row).
const SHARED_APPLICATION_TABLE: &[(&str, &str, &str)] = &[
    // Opt<string> / string
    ("optParam", "Opt<string>", "string"),
    // Opt<Opt<string>> / string
    ("optNested", "Opt<Opt<string>>", "string"),
    // Opt<string> / string
    ("optRead", "Opt<string>", "string"),
    // { a: Opt<string>; } / { a: string; }
    ("optInObj", "{ a: Opt<string> }", "{ a: string }"),
    // Opt<string>[] / string[]
    ("optInArr", "Opt<string>[]", "string[]"),
    // OptNum<string> / OptNum<string>
    ("optNumParam", "OptNum<string>", "OptNum<string>"),
    // Opt<string> / string
    ("optCall", "Opt<string>", "string"),
    // Opt<string>[] / string[]
    ("optCallInArr", "Opt<string>[]", "string[]"),
    // { a: Opt<string>; } / { a: string; }
    ("optCallInObj", "{ a: Opt<string> }", "{ a: string }"),
    // Wrap<string | null> / Wrap<string>
    ("wrapCall", "Wrap<null | string>", "Wrap<string>"),
    // [string, string | null] / [string, string]
    ("pairCall", "[string, null | string]", "[string, string]"),
    // string | null / string
    ("idCall", "null | string", "string"),
    // Partial<P> / Partial<P>
    ("partialWhole", "Partial<P>", "Partial<P>"),
    // string | undefined / string
    ("partialRead", "string | undefined", "string"),
    // number | null | undefined / number
    ("partialReadB", "null | number | undefined", "number"),
    // number | null / number
    ("pickRead", "null | number", "number"),
    // Pick<P, "b"> / Pick<P, "b">
    ("pickWhole", "Pick<P, \"b\">", "Pick<P, \"b\">"),
    // number | null / number
    ("readonlyRead", "null | number", "number"),
    // string | null / string
    ("recordRead", "null | string", "string"),
    // Record<"k", string | null> / Record<"k", string>
    (
        "recordWhole",
        "Record<\"k\", null | string>",
        "Record<\"k\", string>",
    ),
    // OptNum<string> / OptNum<string>
    ("optNumRead", "OptNum<string>", "OptNum<string>"),
    // Opt<string | number> / string | number
    (
        "optUnionArgAlias",
        "Opt<number | string>",
        "number | string",
    ),
    // OptNum<string> / OptNum<string>
    ("optNumAlias", "OptNum<string>", "OptNum<string>"),
    // Nul<string | number> / string | number
    ("nulUnionArg", "Nul<number | string>", "number | string"),
    // Pair<string, number> / Pair<string, number>
    ("pairAlias", "Pair<string, number>", "Pair<string, number>"),
    // Three<string> / Three<string>
    ("threeAlias", "Three<string>", "Three<string>"),
    // Opt<"a" | "b"> / "a" | "b"
    ("optLiteralUnionArg", "Opt<\"a\" | \"b\">", "\"a\" | \"b\""),
    // Plain<string> / Plain<string>
    ("plainAlias", "Plain<string>", "Plain<string>"),
    // Plain<string | boolean> / Plain<string | boolean>
    (
        "plainUnionArg",
        "Plain<boolean | string>",
        "Plain<boolean | string>",
    ),
    // Opt<string | null> / string
    ("optNullableArg", "Opt<null | string>", "string"),
    // OptNum<string | boolean> / OptNum<string | boolean>
    (
        "optNumUnionArg",
        "OptNum<boolean | string>",
        "OptNum<boolean | string>",
    ),
];

/// A generic application entering the flow was built by the checker under
/// the function's own algebra. With `strictNullChecks` off its type
/// arguments carry no nullable member (`Record<"k", string | null>` is
/// `Record<"k", string>`, `wrapOf(v)` over `Wrap<T | null>` is
/// `Wrap<string>`), and an application whose union loses its nullable
/// members is that union (`Opt<string>` over `T | undefined` is
/// `string`) — named by its alias only while the checker still names it
/// (`OptNum<string>`, `Pair<string, number>`), and never when what is
/// left is one argument's own union (`Opt<string | number>` is `string |
/// number`). A named declaration keeps its name (`Partial<P>`), its
/// members erasing where a read projects them. Each row matches its own
/// project's TypeScript 7.0.2 answer.
#[test]
fn shared_type_applications_follow_the_functions_own_algebra() {
    let host = two_policy_host();
    let mismatches = table_mismatches(
        &host,
        "applications.ts",
        SHARED_APPLICATION_SOURCE,
        SHARED_APPLICATION_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 table:\n{}",
        mismatches.join("\n")
    );
}

/// The whole-return answer of `symbol` reduced to the altitude the checker
/// prints a type at (a conditional application resolved, a named one kept),
/// through the production structural-fact loop.
fn observe_at_checker_altitude(host: &VerterHost, canonical: &str, symbol: &str) -> String {
    let carrier = host.get_flow_return_type_with_audit(
        &identity(canonical, symbol),
        ReturnProjectionDemand::whole_return(),
    );
    let Ok(result) = carrier.as_result() else {
        return "<no value>".to_owned();
    };
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let demand = dispatch.normalize_node_keeping_declaration_refs_for_tests(
        result.return_type(),
        crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    );
    match demand.into_complete_node() {
        Some(node) => answer_text(host, node),
        None => "<partial>".to_owned(),
    }
}

/// Conditional applications reached as flow values.
const CONDITIONAL_APPLICATION_SOURCE: &str = r#"
type Exclude<T, U> = T extends U ? never : T;
type Extract<T, U> = T extends U ? T : never;
type NonNullable<T> = T & {};
type Box<T> = T extends null ? "none" : { v: T };
type IsNullable<T> = null extends T ? true : false;
type IsUndef<T> = undefined extends T ? "y" : "n";
declare function boxOf<T>(v: T): Box<T>;
declare function exNull<T>(v: T): Exclude<T, null>;
export function condDistParam(b: Box<string | null>) { return b; }
export function condDistCall(v: string | null) { return boxOf(v); }
export function excludeNull(v: Exclude<string | null, null>) { return v; }
export function excludeNullCall(v: string | null) { return exNull(v); }
export function excludeUndefNull(v: Exclude<string | undefined, null>) { return v; }
export function extractNull(v: Extract<string | null, null>) { return v; }
export function nonNullableParam(v: NonNullable<string | null>) { return v; }
export function isNullableStr(x: IsNullable<string>) { return x; }
export function isUndefNum(x: IsUndef<number>) { return x; }
"#;

/// With `strictNullChecks` off a conditional application decides under the
/// function's own algebra: its erased argument distributes over what the
/// checker built (`Box<string | null>` resolves as `Box<string>`, never
/// reaching the `null` branch; `Extract<string | null, null>` is
/// `never`), and a check relating `null` / `undefined` holds as it does
/// without `strictNullChecks` (`null extends string`). Read at the
/// checker's print altitude, each row is its project's TypeScript 7.0.2
/// answer. `NonNullable<T>` (`T & {}`) is `string` for both projects on
/// the checker, with or without the null member the strict argument keeps:
/// its erased argument is the only difference, and it changes nothing.
#[test]
fn conditional_applications_decide_under_the_functions_own_algebra() {
    let host = two_policy_host();
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        upsert(
            &host,
            &format!("{root}/conditionals.ts"),
            CONDITIONAL_APPLICATION_SOURCE,
        );
    }
    let loose = format!("{LOOSE_ROOT}/conditionals.ts");
    let strict = format!("{STRICT_ROOT}/conditionals.ts");
    let rows: &[(&str, &str, &str)] = &[
        // { v: string; } with strictNullChecks off
        (LOOSE_ROOT, "condDistParam", "{ v: string }"),
        // { v: string; }
        (LOOSE_ROOT, "condDistCall", "{ v: string }"),
        // string
        (LOOSE_ROOT, "excludeNull", "string"),
        // string
        (LOOSE_ROOT, "excludeNullCall", "string"),
        // string (string | undefined when strict)
        (LOOSE_ROOT, "excludeUndefNull", "string"),
        (STRICT_ROOT, "excludeUndefNull", "string | undefined"),
        // never (the .d.ts print; null when strict)
        (LOOSE_ROOT, "extractNull", "never"),
        // true / false
        (LOOSE_ROOT, "isNullableStr", "true"),
        (STRICT_ROOT, "isNullableStr", "false"),
        // "y" / "n"
        (LOOSE_ROOT, "isUndefNum", "\"y\""),
        (STRICT_ROOT, "isUndefNum", "\"n\""),
    ];
    let mut mismatches = Vec::new();
    for (root, symbol, expected) in rows {
        let canonical = if *root == LOOSE_ROOT { &loose } else { &strict };
        let observed = observe_at_checker_altitude(&host, canonical, symbol);
        if observed != *expected {
            mismatches.push(format!(
                "{canonical} `{symbol}`: expected `{expected}`, observed `{observed}`"
            ));
        }
    }
    assert_eq!(
        observe(&host, &strict, "nonNullableParam"),
        "NonNullable<null | string>"
    );
    assert_eq!(
        observe(&host, &loose, "nonNullableParam"),
        "NonNullable<string>"
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 prints:\n{}",
        mismatches.join("\n")
    );
}

/// Returns, array elements and yields joining two DECLARED object types,
/// an object literal beside a declared type, object and tuple literals
/// holding a bare `null` / `undefined` member or element, and declared
/// types whose `k` members are different unit types.
const OBJECT_JOIN_SOURCE: &str = r#"
interface Generator<T, TReturn, TNext> {}
interface KN { k: null; v: string }
interface KS { k: "s"; v: string }
export function retDeclSubtype(c: boolean, a: { x: string }, b: { x: string; y: number }) { if (c) return a; return b; }
export function retDeclSupertypeLast(c: boolean, a: { x: string; y: number }, b: { x: string }) { if (c) return a; return b; }
export function retLitVsDecl(c: boolean, b: { x: string; y: number }) { if (c) return { x: "s" }; return b; }
export function retDeclVsLit(c: boolean, a: { x: string }) { if (c) return a; return { x: "s", y: 1 }; }
export function arrDeclSubtype(a: { x: string }, b: { x: string; y: number }) { return [a, b]; }
export function* genDeclSubtype(a: { x: string }, b: { x: string; y: number }) { yield a; yield b; }
export function retNullObj(c: boolean) { if (c) return { a: null }; return { a: "s" }; }
export function retNullObjOnly(c: boolean) { if (c) return { a: null }; return { a: undefined }; }
export function retNestedNull(c: boolean) { if (c) return { a: { b: null } }; return { a: { b: 1 } }; }
export function retNullObjDecl(c: boolean, o: { a: string }) { if (c) return { a: null }; return o; }
export function retMixedObj(c: boolean) { if (c) return { a: null, b: 1 }; return { a: "s", b: 2 }; }
export function arrNullObj() { return [{ a: null }, { a: "s" }]; }
export function arrNullObjTuple() { return [[null], ["s"]] as const; }
export function* genNullObj() { yield { a: null }; yield { a: "s" }; }
export function retNullTupleJoin(c: boolean) { if (c) return [null] as const; return ["s"] as const; }
export function arrNullTuples() { return [[null] as const, ["s"] as const]; }
export function retNullObjInArr(c: boolean) { if (c) return { a: [null] as const }; return { a: ["s"] as const }; }
export function retHoleTuple(c: boolean) { if (c) return [,] as const; return ["s"] as const; }
export function retNullTupleVsDecl(c: boolean, s: string) { if (c) return [null] as const; return [s] as const; }
export function retDiscDecl(c: boolean, a: { k: null; v: string }, b: { k: "s"; v: string }) { if (c) return a; return b; }
export function retDiscIface(c: boolean, a: KN, b: KS) { if (c) return a; return b; }
"#;

/// `(symbol, strict, off)` for [`OBJECT_JOIN_SOURCE`], each the checker's
/// answer on TypeScript 7.0.2 (the checker's print follows each row).
/// `retNullObjOnly` off prints `{ a: any } | { a: any }` there: the
/// reduction keeps both fresh literals apart (their `a` members are
/// different unit types, `null` and `undefined`), and each widens to its
/// own `{ a: any }` object — two identities of ONE structure, which the
/// graph interns as the single type the row expects.
const OBJECT_JOIN_TABLE: &[(&str, &str, &str)] = &[
    // { x: string; } / { x: string; }
    ("retDeclSubtype", "{ x: string }", "{ x: string }"),
    // { x: string; } / { x: string; }
    ("retDeclSupertypeLast", "{ x: string }", "{ x: string }"),
    // { x: string; y: number; } | { x: string; } / { x: string; y: number; } | { x: string; }
    (
        "retLitVsDecl",
        "{ x: string } | { x: string; y: number }",
        "{ x: string } | { x: string; y: number }",
    ),
    // { x: string; } | { x: string; y: number; } / { x: string; } | { x: string; y: number; }
    (
        "retDeclVsLit",
        "{ x: string } | { x: string; y: number }",
        "{ x: string } | { x: string; y: number }",
    ),
    // { x: string; }[] / { x: string; }[]
    ("arrDeclSubtype", "{ x: string }[]", "{ x: string }[]"),
    // Generator<{ x: string; }, void, unknown> / Generator<{ x: string; }, void, unknown>
    (
        "genDeclSubtype",
        "Generator<{ x: string }, void, unknown>",
        "Generator<{ x: string }, void, unknown>",
    ),
    // { a: null; } | { a: string; } / { a: string; }
    ("retNullObj", "{ a: null } | { a: string }", "{ a: string }"),
    // { a: null; } | { a: undefined; } / { a: any; } | { a: any; }
    (
        "retNullObjOnly",
        "{ a: null } | { a: undefined }",
        "{ a: any }",
    ),
    // { a: { b: null; }; } | { a: { b: number; }; } / { a: { b: number; }; }
    (
        "retNestedNull",
        "{ a: { b: null } } | { a: { b: number } }",
        "{ a: { b: number } }",
    ),
    // { a: string; } | { a: null; } / { a: string; }
    (
        "retNullObjDecl",
        "{ a: null } | { a: string }",
        "{ a: string }",
    ),
    // { a: null; b: number; } | { a: string; b: number; } / { a: string; b: number; }
    (
        "retMixedObj",
        "{ a: null; b: number } | { a: string; b: number }",
        "{ a: string; b: number }",
    ),
    // ({ a: null; } | { a: string; })[] / { a: string; }[]
    (
        "arrNullObj",
        "({ a: null } | { a: string })[]",
        "{ a: string }[]",
    ),
    // readonly [readonly [null], readonly ["s"]] / readonly [readonly [any], readonly ["s"]]
    (
        "arrNullObjTuple",
        "readonly [readonly [null], readonly [\"s\"]]",
        "readonly [readonly [any], readonly [\"s\"]]",
    ),
    // Generator<{ a: null; } | { a: string; }, void, unknown> / Generator<{ a: string; }, void, unknown>
    (
        "genNullObj",
        "Generator<{ a: null } | { a: string }, void, unknown>",
        "Generator<{ a: string }, void, unknown>",
    ),
    // readonly [null] | readonly ["s"] / readonly [any] | readonly ["s"]
    (
        "retNullTupleJoin",
        "readonly [\"s\"] | readonly [null]",
        "readonly [\"s\"] | readonly [any]",
    ),
    // (readonly [null] | readonly ["s"])[] / (readonly [any] | readonly ["s"])[]
    (
        "arrNullTuples",
        "(readonly [\"s\"] | readonly [null])[]",
        "(readonly [\"s\"] | readonly [any])[]",
    ),
    // { a: readonly [null]; } | { a: readonly ["s"]; } / { a: readonly ["s"]; }
    (
        "retNullObjInArr",
        "{ a: readonly [\"s\"] } | { a: readonly [null] }",
        "{ a: readonly [\"s\"] }",
    ),
    // readonly [undefined] | readonly ["s"] / readonly [any] | readonly ["s"]
    (
        "retHoleTuple",
        "readonly [\"s\"] | readonly [undefined]",
        "readonly [\"s\"] | readonly [any]",
    ),
    // readonly [null] | readonly [string] / readonly [string]
    (
        "retNullTupleVsDecl",
        "readonly [null] | readonly [string]",
        "readonly [string]",
    ),
    // { k: null; v: string; } | { k: "s"; v: string; } / { k: null; v: string; } | { k: "s"; v: string; }
    (
        "retDiscDecl",
        "{ k: \"s\"; v: string } | { k: null; v: string }",
        "{ k: \"s\"; v: string } | { k: null; v: string }",
    ),
    // KN | KS / KN | KS
    ("retDiscIface", "KN | KS", "KN | KS"),
];

/// The subtype reduction of a return join, an array's element union or a
/// generator's yield union absorbs a declared object type into a declared
/// supertype of it (two parameters' `{ x: string }` and `{ x: string; y:
/// number }` are `{ x: string }`), while an object LITERAL on either side
/// keeps both: the checker normalizes a literal's missing members, and a
/// literal target rejects a source with members it lacks. With
/// `strictNullChecks` off the reduction compares object literals BEFORE
/// their bare `null` / `undefined` members widen to `any`, as the
/// checker widens only the reduced union: `{ a: null }` beside `{ a: "s" }`
/// is absorbed and the join is `{ a: string }`, where the widened `{ a:
/// any }` would have absorbed `{ a: string }` instead. An arm whose first
/// unit-typed property is a DIFFERENT unit type from its peer's is never
/// absorbed, though `null` relates to `"s"` there: `[null] as const`
/// beside `["s"] as const` is `readonly [any] | readonly ["s"]`. Each row
/// matches its own project's TypeScript 7.0.2 answer.
#[test]
fn object_arms_reduce_to_their_supertype_before_widening() {
    let host = two_policy_host();
    let mismatches = table_mismatches(
        &host,
        "object-joins.ts",
        OBJECT_JOIN_SOURCE,
        OBJECT_JOIN_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 table:\n{}",
        mismatches.join("\n")
    );
}

/// Operations on an evolving array: `push` / `unshift` calls and element writes under a number-like index, on `const`, `let` and `var` bindings.
const EVOLVING_OPERATIONS_SOURCE: &str = r#"
function makeEvolving() { const a = []; a.push(1); a.push("s"); return a; }
export function returnedEmpty() { const a = []; return a; }
export function pushOne() { const a = []; a.push(1); return a; }
export function pushTwo() { const a = []; a.push(1); a.push("s"); return a; }
export function pushMany() { const a = []; a.push(1, "s"); return a; }
export function pushNoArgs() { const a = []; a.push(); return a; }
export function unshiftOne() { const a = []; a.unshift(true); return a; }
export function unshiftAndPush() { const a = []; a.unshift(1); a.push("s"); return a; }
export function pushLiteralConst() { const a = []; a.push("lit" as const); return a; }
export function pushParam(v: "x" | "y") { const a = []; a.push(v); return a; }
export function pushAny(x: any) { const a = []; a.push(x); a.push(1); return a; }
export function pushUnknown(x: unknown) { const a = []; a.push(x); a.push(1); return a; }
export function pushNever(x: never) { const a = []; a.push(x); return a; }
export function pushNull() { const a = []; a.push(null); return a; }
export function pushUndefined() { const a = []; a.push(undefined); return a; }
export function pushNullRead() { const a = []; a.push(null); const b = a; return b; }
export function pushArray() { const a = []; a.push([1]); return a; }
export function pushEmptyArray() { const a = []; a.push([]); return a; }
export function pushTwoArrays() { const a = []; a.push([1]); a.push(["s"]); return a; }
export function pushFunction() { const a = []; a.push(() => 1); return a; }
export function pushAsConstObj() { const a = []; a.push({ k: "v" } as const); return a; }
export function pushSubtypes(x: { a: number }, y: { a: number; b: number }) { const a = []; a.push(y); a.push(x); return a; }
export function pushStringEnumLike(k: "a" | "b") { const a = []; a.push(k); a.push(k); return a; }
export function pushSpread(xs: number[]) { const a = []; a.push(...xs); return a; }
export function spreadString(s: string) { const a = []; a.push(...s); return a; }
export function spreadTuple(t: [number, string]) { const a = []; a.push(...t); return a; }
export function pushOptional() { const a = []; a?.push(1); return a; }
export function parenPush() { const a = []; (a).push(1); return a; }
export function letPush() { let a = []; a.push(1); return a; }
export function varPush() { var a = []; a.push(1); return a; }
export function letUnshift() { let a = []; a.unshift(1); return a; }
export function varUnshift() { var a = []; a.unshift("s"); return a; }
export function indexWrite() { const a = []; a[0] = "x"; return a; }
export function letIndexWrite() { let a = []; a[0] = 1; return a; }
export function varIndexWrite() { var a = []; a[0] = 1; return a; }
export function parenWrite() { const a = []; (a)[0] = 1; return a; }
export function anyIndexWrite() { const a = []; a["k" as any] = 1; return a; }
export function stringKeyWrite() { const a = []; a["k"] = 1; return a; }
export function stringTypedIndex(k: string) { const a = []; a[k] = 1; return a; }
export function unionIndex(k: number | string) { const a = []; a[k] = 1; return a; }
export function literalUnionIndex(k: 0 | 1) { const a = []; a[k] = "s"; return a; }
export function compoundWrite() { const a = []; a[0] = 1; a[0] += 1; return a; }
export function compoundKey() { const a = []; a[0] = 1; a[1] ??= "s"; return a; }
export function destructureWrite() { const a = []; [a[0]] = [1]; return a; }
export function lengthAssign() { const a = []; a.length = 0; a.push(1); return a; }
export function returnPush() { const a = []; return a.push(1); }
export function writeThenPushExpr() { const a = []; return a.push(1); }
export function parenthesized() { const a = ([]); return a; }
export function returnedFromFunction() { return makeEvolving(); }
export function returnedFromFunctionLet() { let a = []; a[0] = 1; a.unshift(true); return a; }
"#;

/// Each row: the checker's print under strict, `strictNullChecks` off,
/// `noImplicitAny` off, and both off (TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`), with the diagnostics each reports on the
/// function's line (`code@column`); then the four answers.
const EVOLVING_OPERATIONS_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // any[] {TS7034@41,TS7005@56} / any[] {TS7034@41,TS7005@56} / never[] / any[]
    ("returnedEmpty", "any[]", "any[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@50} / any[]
    ("pushOne", "number[]", "number[]", "never[]", "any[]"),
    // (string | number)[] / (string | number)[] / never[] {TS2345@50,TS2345@61} / any[]
    (
        "pushTwo",
        "(number | string)[]",
        "(number | string)[]",
        "never[]",
        "any[]",
    ),
    // (string | number)[] / (string | number)[] / never[] {TS2345@51} / any[]
    (
        "pushMany",
        "(number | string)[]",
        "(number | string)[]",
        "never[]",
        "any[]",
    ),
    // any[] {TS7034@38,TS7005@63} / any[] {TS7034@38,TS7005@63} / never[] / any[]
    ("pushNoArgs", "any[]", "any[]", "never[]", "any[]"),
    // boolean[] / boolean[] / never[] {TS2345@56} / any[]
    ("unshiftOne", "boolean[]", "boolean[]", "never[]", "any[]"),
    // (string | number)[] / (string | number)[] / never[] {TS2345@60,TS2345@71} / any[]
    (
        "unshiftAndPush",
        "(number | string)[]",
        "(number | string)[]",
        "never[]",
        "any[]",
    ),
    // string[] / string[] / never[] {TS2345@59} / any[]
    (
        "pushLiteralConst",
        "string[]",
        "string[]",
        "never[]",
        "any[]",
    ),
    // string[] / string[] / never[] {TS2345@64} / any[]
    ("pushParam", "string[]", "string[]", "never[]", "any[]"),
    // any[] / any[] / never[] {TS2345@56,TS2345@67} / any[]
    ("pushAny", "any[]", "any[]", "never[]", "any[]"),
    // unknown[] / unknown[] / never[] {TS2345@64,TS2345@75} / any[]
    ("pushUnknown", "unknown[]", "unknown[]", "never[]", "any[]"),
    // any[] {TS7034@45,TS7005@71} / any[] {TS7034@45,TS7005@71} / never[] / any[]
    ("pushNever", "any[]", "any[]", "never[]", "any[]"),
    // null[] / any[] {TS7010@17} / never[] {TS2345@51} / any[]
    ("pushNull", "null[]", "any[]", "never[]", "any[]"),
    // undefined[] / any[] {TS7010@17} / never[] {TS2345@56} / any[]
    ("pushUndefined", "undefined[]", "any[]", "never[]", "any[]"),
    // null[] / any[] {TS7005@68} / never[] {TS2345@55} / any[]
    ("pushNullRead", "null[]", "any[]", "never[]", "any[]"),
    // number[][] / number[][] / never[] {TS2345@52} / any[]
    ("pushArray", "number[][]", "number[][]", "never[]", "any[]"),
    // never[][] / any[][] {TS7010@17} / never[] {TS2345@57} / any[]
    ("pushEmptyArray", "never[][]", "any[][]", "never[]", "any[]"),
    // (string[] | number[])[] / (string[] | number[])[] / never[] {TS2345@56,TS2345@69} / any[]
    (
        "pushTwoArrays",
        "(number[] | string[])[]",
        "(number[] | string[])[]",
        "never[]",
        "any[]",
    ),
    // (() => number)[] / (() => number)[] / never[] {TS2345@55} / any[]
    (
        "pushFunction",
        "(() => number)[]",
        "(() => number)[]",
        "never[]",
        "any[]",
    ),
    // { readonly k: "v"; }[] / { readonly k: "v"; }[] / never[] {TS2345@57} / any[]
    (
        "pushAsConstObj",
        "{ readonly k: \"v\" }[]",
        "{ readonly k: \"v\" }[]",
        "never[]",
        "any[]",
    ),
    // { a: number; }[] / { a: number; }[] / never[] {TS2345@100,TS2345@111} / any[]
    (
        "pushSubtypes",
        "{ a: number }[]",
        "{ a: number }[]",
        "never[]",
        "any[]",
    ),
    // string[] / string[] / never[] {TS2345@73,TS2345@84} / any[]
    (
        "pushStringEnumLike",
        "string[]",
        "string[]",
        "never[]",
        "any[]",
    ),
    // number[] / number[] / never[] {TS2345@65} / any[]
    ("pushSpread", "number[]", "number[]", "never[]", "any[]"),
    // string[] / string[] / never[] {TS2345@64} / any[]
    ("spreadString", "string[]", "string[]", "never[]", "any[]"),
    // (string | number)[] / (string | number)[] / never[] {TS2345@73} / any[]
    (
        "spreadTuple",
        "(number | string)[]",
        "(number | string)[]",
        "never[]",
        "any[]",
    ),
    // number[] / number[] / never[] {TS2345@56} / any[]
    ("pushOptional", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@54} / any[]
    ("parenPush", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@48} / any[]
    ("letPush", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@48} / any[]
    ("varPush", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@54} / any[]
    ("letUnshift", "number[]", "number[]", "never[]", "any[]"),
    // string[] / string[] / never[] {TS2345@54} / any[]
    ("varUnshift", "string[]", "string[]", "never[]", "any[]"),
    // string[] / string[] / never[] {TS2322@46} / any[]
    ("indexWrite", "string[]", "string[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2322@47} / any[]
    ("letIndexWrite", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2322@47} / any[]
    ("varIndexWrite", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2322@46} / any[]
    ("parenWrite", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2322@49} / any[]
    ("anyIndexWrite", "number[]", "number[]", "never[]", "any[]"),
    // any[] {TS7034@42,TS7005@50,TS7015@52,TS7005@69} / any[] {TS7034@42,TS7005@50,TS7015@52,TS7005@69} / never[] / any[]
    ("stringKeyWrite", "any[]", "any[]", "never[]", "any[]"),
    // any[] {TS7034@53,TS7005@61,TS7015@63,TS7005@78} / any[] {TS7034@53,TS7005@61,TS7015@63,TS7005@78} / never[] / any[]
    ("stringTypedIndex", "any[]", "any[]", "never[]", "any[]"),
    // any[] {TS7034@56,TS7005@64,TS7015@66,TS7005@81} / any[] {TS7034@56,TS7005@64,TS7015@66,TS7005@81} / never[] / any[]
    ("unionIndex", "any[]", "any[]", "never[]", "any[]"),
    // string[] / string[] / never[] {TS2322@61} / any[]
    (
        "literalUnionIndex",
        "string[]",
        "string[]",
        "never[]",
        "any[]",
    ),
    // number[] / number[] / never[] {TS2322@49,TS2322@59} / any[]
    ("compoundWrite", "number[]", "number[]", "never[]", "any[]"),
    // number[] {TS2322@57} / number[] {TS2322@57} / never[] {TS2322@47,TS2322@57} / any[]
    ("compoundKey", "number[]", "number[]", "never[]", "any[]"),
    // any[] {TS7034@44,TS7005@53,TS7005@73} / any[] {TS7034@44,TS7005@53,TS7005@73} / never[] {TS2322@53} / any[]
    ("destructureWrite", "any[]", "any[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@69} / any[]
    ("lengthAssign", "number[]", "number[]", "never[]", "any[]"),
    // number / number / number {TS2345@60} / number
    ("returnPush", "number", "number", "number", "number"),
    // number / number / number {TS2345@67} / number
    ("writeThenPushExpr", "number", "number", "number", "number"),
    // never[] / any[] {TS7005@41} / never[] / any[]
    ("parenthesized", "never[]", "any[]", "never[]", "any[]"),
    // (string | number)[] / (string | number)[] / never[] / any[]
    (
        "returnedFromFunction",
        "(number | string)[]",
        "(number | string)[]",
        "never[]",
        "any[]",
    ),
    // (number | boolean)[] / (number | boolean)[] / never[] {TS2322@57,TS2345@77} / any[]
    (
        "returnedFromFunctionLet",
        "(boolean | number)[]",
        "(boolean | number)[]",
        "never[]",
        "any[]",
    ),
];

/// References to an evolving array that are not operations on it: each reads the array finalized where it stands, and the array keeps evolving after it.
const EVOLVING_FINALIZING_SOURCE: &str = r#"
export function readThenPush() { const a = []; const b = a; a.push(1); return b; }
export function readInElement() { const a = []; return [a]; }
export function evolvingInObject() { const a = []; a.push(1); return { a }; }
export function exprPush() { const a = []; const n = a.push(1); return [n, a]; }
export function assignExprValue() { const a = []; const v = (a[0] = "s"); return [v, a]; }
export function selfPush() { const a = []; a.push(a); return a; }
export function pushThenReadThenPush() { const a = []; a.push(1); const b = a; a.push("s"); return [b, a]; }
export function readBetweenPushes() { const a = []; a.push(1); const b = a; a.push("s"); return b; }
export function readBetweenPushesWhole() { const a = []; a.push(1); const b = a; a.push("s"); return a; }
export function earlyReturn(c: boolean) { const a = []; if (c) return a; a.push(1); return a; }
"#;

/// Each row: the checker's print under strict, `strictNullChecks` off,
/// `noImplicitAny` off, and both off (TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`), with the diagnostics each reports on the
/// function's line (`code@column`); then the four answers.
const EVOLVING_FINALIZING_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // any[] {TS7034@40,TS7005@58} / any[] {TS7034@40,TS7005@58} / never[] {TS2345@68} / any[]
    ("readThenPush", "any[]", "any[]", "never[]", "any[]"),
    // any[][] {TS7034@41,TS7005@57} / any[][] {TS7034@41,TS7005@57} / never[][] / any[][]
    (
        "readInElement",
        "any[][]",
        "any[][]",
        "never[][]",
        "any[][]",
    ),
    // { a: number[]; } / { a: number[]; } / { a: never[]; } {TS2345@59} / { a: any[]; }
    (
        "evolvingInObject",
        "{ a: number[] }",
        "{ a: number[] }",
        "{ a: never[] }",
        "{ a: any[] }",
    ),
    // (number | number[])[] / (number | number[])[] / (number | never[])[] {TS2345@61} / (number | any[])[]
    (
        "exprPush",
        "(number | number[])[]",
        "(number | number[])[]",
        "(never[] | number)[]",
        "(any[] | number)[]",
    ),
    // (string | string[])[] / (string | string[])[] / (string | never[])[] {TS2322@62} / (string | any[])[]
    (
        "assignExprValue",
        "(string | string[])[]",
        "(string | string[])[]",
        "(never[] | string)[]",
        "(any[] | string)[]",
    ),
    // any[][] {TS7034@36,TS7005@51} / any[][] {TS7034@36,TS7005@51} / never[] {TS2345@51} / any[]
    ("selfPush", "any[][]", "any[][]", "never[]", "any[]"),
    // (string | number)[][] / (string | number)[][] / never[][] {TS2345@63,TS2345@87} / any[][]
    (
        "pushThenReadThenPush",
        "(number | string)[][]",
        "(number | string)[][]",
        "never[][]",
        "any[][]",
    ),
    // number[] / number[] / never[] {TS2345@60,TS2345@84} / any[]
    (
        "readBetweenPushes",
        "number[]",
        "number[]",
        "never[]",
        "any[]",
    ),
    // (string | number)[] / (string | number)[] / never[] {TS2345@65,TS2345@89} / any[]
    (
        "readBetweenPushesWhole",
        "(number | string)[]",
        "(number | string)[]",
        "never[]",
        "any[]",
    ),
    // any[] {TS7034@49,TS7005@71} / any[] {TS7034@49,TS7005@71} / never[] {TS2345@81} / any[]
    ("earlyReturn", "any[]", "any[]", "never[]", "any[]"),
];

/// Assignments to an evolving array binding: an unparenthesized `[]` starts a new evolving array, any other value is held when assignable to `any[]` (else `any[]`), and paths holding an ordinary value join the evolving ones under subtype reduction.
const EVOLVING_ASSIGNMENTS_SOURCE: &str = r#"
export function reassignLiteral() { let a = []; a = [1]; return a; }
export function reassignAfterRead() { let a = []; a.push(1); const b = a; a = ["s"]; return [a, b]; }
export function letReassignAfterRead() { let a = []; a.push(1); const b = a; a = [true]; a.push("s"); return [a, b]; }
export function varReassignAfterRead() { var a = []; a.push(1); const b = a; a = [true]; return [a, b]; }
export function resetEmpty() { let a = []; a.push(1); a = []; a.push("s"); return a; }
export function parenReset() { let a = []; a = ([]); a.push(1); return a; }
export function parenAssign() { let a = []; a = ([]); a.push(1); return a; }
export function assignThenPush() { let a = []; a = [1]; a.push("s"); return a; }
export function assignString() { let a = []; a.push(1); a = "s" as any as string; return a; }
export function assignTuple() { let a = []; a = [1, "s"] as [number, string]; return a; }
export function valueReset() { let a = []; a.push(1); const b = (a = []); a.push("s"); return [a, b]; }
export function mixedJoin(c: boolean) { let a = []; if (c) a = [1]; return a; }
export function mixedJoin2(c: boolean) { let a = []; if (c) { a = [1]; } else { a.push("s"); } return a; }
export function mixedJoin3(c: boolean) { let a = []; a.push(true); if (c) a = [1]; return a; }
export function mixedJoinRead(c: boolean) { let a = []; if (c) a = [1]; const b = a; return b; }
"#;

/// Each row: the checker's print under strict, `strictNullChecks` off,
/// `noImplicitAny` off, and both off (TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`), with the diagnostics each reports on the
/// function's line (`code@column`); then the four answers.
const EVOLVING_ASSIGNMENTS_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number[] / number[] / never[] {TS2322@54} / any[]
    (
        "reassignLiteral",
        "number[]",
        "number[]",
        "never[]",
        "any[]",
    ),
    // (string[] | number[])[] / (string[] | number[])[] / never[][] {TS2345@58,TS2322@80} / any[][]
    (
        "reassignAfterRead",
        "(number[] | string[])[]",
        "(number[] | string[])[]",
        "never[][]",
        "any[][]",
    ),
    // (number[] | boolean[])[] {TS2345@97} / (number[] | boolean[])[] {TS2345@97} / never[][] {TS2345@61,TS2322@83,TS2345@97} / any[][]
    (
        "letReassignAfterRead",
        "(boolean[] | number[])[]",
        "(boolean[] | number[])[]",
        "never[][]",
        "any[][]",
    ),
    // (number[] | boolean[])[] / (number[] | boolean[])[] / never[][] {TS2345@61,TS2322@83} / any[][]
    (
        "varReassignAfterRead",
        "(boolean[] | number[])[]",
        "(boolean[] | number[])[]",
        "never[][]",
        "any[][]",
    ),
    // string[] / string[] / never[] {TS2345@51,TS2345@70} / any[]
    ("resetEmpty", "string[]", "string[]", "never[]", "any[]"),
    // never[] {TS2345@61} / any[] {TS7010@17} / never[] {TS2345@61} / any[]
    ("parenReset", "never[]", "any[]", "never[]", "any[]"),
    // never[] {TS2345@62} / any[] {TS7010@17} / never[] {TS2345@62} / any[]
    ("parenAssign", "never[]", "any[]", "never[]", "any[]"),
    // number[] {TS2345@64} / number[] {TS2345@64} / never[] {TS2322@53,TS2345@64} / any[]
    ("assignThenPush", "number[]", "number[]", "never[]", "any[]"),
    // any[] {TS2322@57} / any[] {TS2322@57} / never[] {TS2345@53,TS2322@57} / any[] {TS2322@57}
    ("assignString", "any[]", "any[]", "never[]", "any[]"),
    // [number, string] / [number, string] / never[] {TS2322@45} / any[]
    (
        "assignTuple",
        "[number, string]",
        "[number, string]",
        "never[]",
        "any[]",
    ),
    // string[][] / any[][] {TS7005@61} / never[][] {TS2345@51,TS2345@82} / any[][]
    (
        "valueReset",
        "string[][]",
        "any[][]",
        "never[][]",
        "any[][]",
    ),
    // any[] {TS7034@45,TS7005@76} / any[] {TS7034@45,TS7005@76} / never[] {TS2322@65} / any[]
    ("mixedJoin", "any[]", "any[]", "never[]", "any[]"),
    // string[] | number[] / string[] | number[] / never[] {TS2322@68,TS2345@88} / any[]
    (
        "mixedJoin2",
        "number[] | string[]",
        "number[] | string[]",
        "never[]",
        "any[]",
    ),
    // number[] | boolean[] / number[] | boolean[] / never[] {TS2345@61,TS2322@80} / any[]
    (
        "mixedJoin3",
        "boolean[] | number[]",
        "boolean[] | number[]",
        "never[]",
        "any[]",
    ),
    // any[] {TS7034@49,TS7005@83} / any[] {TS7034@49,TS7005@83} / never[] {TS2322@69} / any[]
    ("mixedJoinRead", "any[]", "any[]", "never[]", "any[]"),
];

/// Evolving arrays where paths meet: `if` / `switch` / `try` arms, conditional and logical expressions in statement and value position.
const EVOLVING_BRANCHES_SOURCE: &str = r#"
export function branchPush(c: boolean) { const a = []; if (c) { a.push(1); } else { a.push("s"); } return a; }
export function branchOnePush(c: boolean) { const a = []; if (c) { a.push(1); } return a; }
export function pushInIf(c: boolean) { const a = []; if (c) a.push(1); return a; }
export function switchPush(k: number) { const a = []; switch (k) { case 1: a.push(1); break; default: a.push("s"); } return a; }
export function pushInTry() { const a = []; try { a.push(1); } catch { a.push("s"); } return a; }
export function pushInTernary(c: boolean) { const a = []; c ? a.push(1) : a.push("s"); return a; }
export function pushInLogical(c: boolean) { const a = []; c && a.push(1); return a; }
export function orPush(c: boolean) { const a = []; c || a.push(1); return a; }
export function coalescePush(x: number | null) { const a = []; x ?? a.push("s"); return a; }
export function narrowedAndPush(x: string | null) { const a = []; x !== null && a.push(x); return a; }
export function ternaryWrite(c: boolean) { const a = []; c ? (a[0] = 1) : (a[0] = true); return a; }
export function valueTernaryPush(c: boolean) { const a = []; const n = c ? a.push(1) : a.push("s"); return [n, a]; }
export function valueTernaryRead(c: boolean) { const a = []; const r = c ? a.push(1) : a; return r; }
"#;

/// Each row: the checker's print under strict, `strictNullChecks` off,
/// `noImplicitAny` off, and both off (TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`), with the diagnostics each reports on the
/// function's line (`code@column`); then the four answers.
const EVOLVING_BRANCHES_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // (string | number)[] / (string | number)[] / never[] {TS2345@72,TS2345@92} / any[]
    (
        "branchPush",
        "(number | string)[]",
        "(number | string)[]",
        "never[]",
        "any[]",
    ),
    // number[] / number[] / never[] {TS2345@75} / any[]
    ("branchOnePush", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@68} / any[]
    ("pushInIf", "number[]", "number[]", "never[]", "any[]"),
    // (string | number)[] / (string | number)[] / never[] {TS2345@83,TS2345@110} / any[]
    (
        "switchPush",
        "(number | string)[]",
        "(number | string)[]",
        "never[]",
        "any[]",
    ),
    // (string | number)[] / (string | number)[] / never[] {TS2345@58,TS2345@79} / any[]
    (
        "pushInTry",
        "(number | string)[]",
        "(number | string)[]",
        "never[]",
        "any[]",
    ),
    // (string | number)[] / (string | number)[] / never[] {TS2345@70,TS2345@82} / any[]
    (
        "pushInTernary",
        "(number | string)[]",
        "(number | string)[]",
        "never[]",
        "any[]",
    ),
    // number[] / number[] / never[] {TS2345@71} / any[]
    ("pushInLogical", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@64} / any[]
    ("orPush", "number[]", "number[]", "never[]", "any[]"),
    // string[] / string[] / never[] {TS2345@76} / any[]
    ("coalescePush", "string[]", "string[]", "never[]", "any[]"),
    // string[] / string[] / never[] {TS2345@88} / any[]
    (
        "narrowedAndPush",
        "string[]",
        "string[]",
        "never[]",
        "any[]",
    ),
    // (number | boolean)[] / (number | boolean)[] / never[] {TS2322@63,TS2322@76} / any[]
    (
        "ternaryWrite",
        "(boolean | number)[]",
        "(boolean | number)[]",
        "never[]",
        "any[]",
    ),
    // (number | (string | number)[])[] / (number | (string | number)[])[] / (number | never[])[] {TS2345@83,TS2345@95} / (number | any[])[]
    (
        "valueTernaryPush",
        "((number | string)[] | number)[]",
        "((number | string)[] | number)[]",
        "(never[] | number)[]",
        "(any[] | number)[]",
    ),
    // number | any[] {TS7034@54,TS7005@88} / number | any[] {TS7034@54,TS7005@88} / number | never[] {TS2345@83} / number | any[]
    (
        "valueTernaryRead",
        "any[] | number",
        "any[] | number",
        "never[] | number",
        "any[] | number",
    ),
];

/// Evolving arrays in loops: each reference's loop-head type is its entry type joined with one pass of the iteration in which it reads its own entry type.
const EVOLVING_LOOPS_SOURCE: &str = r#"
export function loopWrite(n: number) { const a = []; for (let i = 0; i < n; i++) { a[i] = i; } return a; }
export function letLoopWrite(n: number) { let a = []; for (let i = 0; i < n; i++) { a[i] = i; } return a; }
export function varLoopWrite(n: number) { var a = []; for (let i = 0; i < n; i++) { a[i] = "s"; } return a; }
export function loopPush(xs: string[]) { const a = []; for (const x of xs) { a.push(x); } return a; }
export function whileLoop(n: number) { const a = []; while (n > 0) { a.push(n); n--; } return a; }
export function doWhile(n: number) { const a = []; do { a.push(n); n--; } while (n > 0); return a; }
export function forOfNarrow(xs: (string | null)[]) { const a = []; for (const x of xs) { if (x !== null) a.push(x); } return a; }
export function forInLoop(o: { p: number }) { const a = []; for (const k in o) { a.push(k); } return a; }
export function nestedLoop(xs: number[][]) { const a = []; for (const r of xs) { for (const v of r) { a.push(v); } } return a; }
export function breakLoop(xs: number[]) { const a = []; for (const x of xs) { if (x > 1) break; a.push(x); } return a; }
export function continueLoop(xs: number[]) { const a = []; for (const x of xs) { if (x > 1) continue; a.push("s"); } return a; }
export function labeledBreak(xs: number[][]) { const a = []; outer: for (const r of xs) { for (const v of r) { if (v) break outer; a.push(v); } } return a; }
export function returnInLoop(xs: number[]) { const a = []; for (const x of xs) { a.push(x); if (x) return a; } return a; }
export function loopTwoKinds(xs: number[], ys: string[]) { const a = []; for (const x of xs) { a.push(x); } for (const y of ys) { a.push(y); } return a; }
export function selfPushLoop(xs: number[]) { const a = []; for (const x of xs) { a.push(a); } return a; }
export function typedSelfPushLoop(xs: number[]) { const a = []; a.push(1); for (const x of xs) { a.push(a); } return a; }
export function wrapLoop(xs: number[]) { const a = []; a.push(1); for (const x of xs) { a.push([a]); } return a; }
export function copyLoop(xs: number[]) { const a = []; const b = []; a.push(1); for (const x of xs) { b.push(a); a.push(b); } return a; }
export function chainedLoop(xs: number[]) { const a = []; const b = []; for (const x of xs) { a.push(b); b.push(x); } return a; }
export function chainedLoop3(xs: string[]) { const a = []; const b = []; const c = []; for (const x of xs) { a.push(b); b.push(c); c.push(x); } return a; }
export function nestPushLoop(xs: number[]) { const a = []; let b: any = 1; for (const x of xs) { a.push(b); b = [b]; } return a; }
"#;

/// Each row: the checker's print under strict, `strictNullChecks` off,
/// `noImplicitAny` off, and both off (TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`), with the diagnostics each reports on the
/// function's line (`code@column`); then the four answers.
const EVOLVING_LOOPS_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number[][] / number[][] / never[] {TS2345@102,TS2345@113} / any[]
    (
        "chainedLoop",
        "number[][]",
        "number[][]",
        "never[]",
        "any[]",
    ),
    // string[][][] / string[][][] / never[] {TS2345@117,TS2345@128,TS2345@139} / any[]
    (
        "chainedLoop3",
        "string[][][]",
        "string[][][]",
        "never[]",
        "any[]",
    ),
    // number[] / number[] / never[] {TS2322@84} / any[]
    ("loopWrite", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2322@85} / any[]
    ("letLoopWrite", "number[]", "number[]", "never[]", "any[]"),
    // string[] / string[] / never[] {TS2322@85} / any[]
    ("varLoopWrite", "string[]", "string[]", "never[]", "any[]"),
    // string[] / string[] / never[] {TS2345@85} / any[]
    ("loopPush", "string[]", "string[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@77} / any[]
    ("whileLoop", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@64} / any[]
    ("doWhile", "number[]", "number[]", "never[]", "any[]"),
    // string[] / string[] / never[] {TS2345@113} / any[]
    ("forOfNarrow", "string[]", "string[]", "never[]", "any[]"),
    // string[] / string[] / never[] {TS2345@89} / any[]
    ("forInLoop", "string[]", "string[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@110} / any[]
    ("nestedLoop", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@104} / any[]
    ("breakLoop", "number[]", "number[]", "never[]", "any[]"),
    // string[] / string[] / never[] {TS2345@110} / any[]
    ("continueLoop", "string[]", "string[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@139} / any[]
    ("labeledBreak", "number[]", "number[]", "never[]", "any[]"),
    // number[] / number[] / never[] {TS2345@89} / any[]
    ("returnInLoop", "number[]", "number[]", "never[]", "any[]"),
    // (string | number)[] / (string | number)[] / never[] {TS2345@103,TS2345@138} / any[]
    (
        "loopTwoKinds",
        "(number | string)[]",
        "(number | string)[]",
        "never[]",
        "any[]",
    ),
    // any[][] {TS7034@52,TS7005@89} / any[][] {TS7034@52,TS7005@89} / never[] {TS2345@89} / any[]
    ("selfPushLoop", "any[][]", "any[][]", "never[]", "any[]"),
    // (number | number[])[] / (number | number[])[] / never[] {TS2345@72,TS2345@105} / any[]
    (
        "typedSelfPushLoop",
        "(number | number[])[]",
        "(number | number[])[]",
        "never[]",
        "any[]",
    ),
    // (number | number[][])[] / (number | number[][])[] / never[] {TS2345@63,TS2345@96} / any[]
    (
        "wrapLoop",
        "(number | number[][])[]",
        "(number | number[][])[]",
        "never[]",
        "any[]",
    ),
    // (number | number[][])[] / (number | number[][])[] / never[] {TS2345@77,TS2345@110,TS2345@121} / any[]
    (
        "copyLoop",
        "(number | number[][])[]",
        "(number | number[][])[]",
        "never[]",
        "any[]",
    ),
    // any[] / any[] / never[] {TS2345@105} / any[]
    ("nestPushLoop", "any[]", "any[]", "never[]", "any[]"),
];

/// Evolving arrays captured by a nested function: a `const` or `var` one, and a `let` one not past its last assignment, read the declared `autoArrayType`; a `let` one past its last assignment continues from the array it holds where the function is created, and operations inside the function evolve it.
const EVOLVING_CAPTURES_SOURCE: &str = r#"
export function captured() { const a = []; const f = () => a; a.push(1); return f(); }
export function capturedLater() { const a = []; a.push(1); const f = () => a; return f(); }
export function readInClosureLater() { const a = []; a.push(1); return () => a; }
export function letCaptured() { let a = []; const f = () => a; a.push(1); return f(); }
export function varCaptured() { var a = []; const f = () => a; a.push(1); return f(); }
export function letCapturedLater() { let a = []; a.push(1); return () => a; }
export function letCapturedBefore() { let a = []; const f = () => a; a.push(1); return f; }
export function varCapturedLater() { var a = []; a.push(1); return () => a; }
export function letCapturedAfterReassign() { let a = []; a = []; a.push(1); return () => a; }
export function letCapturedThenReassign() { let a = []; a.push(1); const f = () => a; a = ["s"]; return f; }
export function letCapturedLoopAssign(xs: number[]) { let a = []; for (const x of xs) { a = [x]; } a.push("s"); return () => a; }
export function nestedEvolving() { const f = () => { const b = []; b.push(1); return b; }; return f; }
export function closureReadsAfterPushLet() { let a = []; a.push(1); const f = () => { a.push("s"); return a; }; return f; }
export function nestedArrowPushLet() { let a = []; a.push(1); const f = () => { a.push("s"); return a; }; return f; }
export function nestedArrowReadLet() { let a = []; a.push(1); const f = () => { const b = a; a.push("s"); return b; }; return f; }
export function nestedArrowPushConst() { const a = []; a.push(1); const f = () => { a.push("s"); return a; }; return f; }
export function nestedTernaryPushLet() { let a = []; a.push(1); const f = (c: boolean) => { c ? a.push("s") : 0; return a; }; return f; }
export function nestedLoopPushLet() { let a = []; a.push(1); const f = (xs: string[]) => { for (const x of xs) { a.push(x); } return a; }; return f; }
export function nestedLetReset() { let a = []; a.push(1); const f = () => { a = []; return a; }; return f; }
"#;

/// Each row: the checker's print under strict, `strictNullChecks` off,
/// `noImplicitAny` off, and both off (TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`), with the diagnostics each reports on the
/// function's line (`code@column`); then the four answers.
const EVOLVING_CAPTURES_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // (c: boolean) => (string | number)[] / (c: boolean) => (string | number)[] / (c: boolean) => never[] {TS2345@61,TS2345@104} / (c: boolean) => any[]
    (
        "nestedTernaryPushLet",
        "(c: boolean) => (number | string)[]",
        "(c: boolean) => (number | string)[]",
        "(c: boolean) => never[]",
        "(c: boolean) => any[]",
    ),
    // (xs: string[]) => (string | number)[] / (xs: string[]) => (string | number)[] / (xs: string[]) => never[] {TS2345@58,TS2345@121} / (xs: string[]) => any[]
    (
        "nestedLoopPushLet",
        "(xs: string[]) => (number | string)[]",
        "(xs: string[]) => (number | string)[]",
        "(xs: string[]) => never[]",
        "(xs: string[]) => any[]",
    ),
    // any[] {TS7034@36,TS7005@60} / any[] {TS7034@36,TS7005@60} / never[] {TS2345@70} / any[]
    ("captured", "any[]", "any[]", "never[]", "any[]"),
    // any[] {TS7034@41,TS7005@76} / any[] {TS7034@41,TS7005@76} / never[] {TS2345@56} / any[]
    ("capturedLater", "any[]", "any[]", "never[]", "any[]"),
    // () => any[] {TS7034@46,TS7005@78} / () => any[] {TS7034@46,TS7005@78} / () => never[] {TS2345@61} / () => any[]
    (
        "readInClosureLater",
        "() => any[]",
        "() => any[]",
        "() => never[]",
        "() => any[]",
    ),
    // any[] {TS7034@37,TS7005@61} / any[] {TS7034@37,TS7005@61} / never[] {TS2345@71} / any[]
    ("letCaptured", "any[]", "any[]", "never[]", "any[]"),
    // any[] {TS7034@37,TS7005@61} / any[] {TS7034@37,TS7005@61} / never[] {TS2345@71} / any[]
    ("varCaptured", "any[]", "any[]", "never[]", "any[]"),
    // () => number[] / () => number[] / () => never[] {TS2345@57} / () => any[]
    (
        "letCapturedLater",
        "() => number[]",
        "() => number[]",
        "() => never[]",
        "() => any[]",
    ),
    // () => any[] {TS7034@43,TS7005@67} / () => any[] {TS7034@43,TS7005@67} / () => never[] {TS2345@77} / () => any[]
    (
        "letCapturedBefore",
        "() => any[]",
        "() => any[]",
        "() => never[]",
        "() => any[]",
    ),
    // () => any[] {TS7034@42,TS7005@74} / () => any[] {TS7034@42,TS7005@74} / () => never[] {TS2345@57} / () => any[]
    (
        "varCapturedLater",
        "() => any[]",
        "() => any[]",
        "() => never[]",
        "() => any[]",
    ),
    // () => number[] / () => number[] / () => never[] {TS2345@73} / () => any[]
    (
        "letCapturedAfterReassign",
        "() => number[]",
        "() => number[]",
        "() => never[]",
        "() => any[]",
    ),
    // () => any[] {TS7034@49,TS7005@84} / () => any[] {TS7034@49,TS7005@84} / () => never[] {TS2345@64,TS2322@92} / () => any[]
    (
        "letCapturedThenReassign",
        "() => any[]",
        "() => any[]",
        "() => never[]",
        "() => any[]",
    ),
    // () => any[] {TS7034@59,TS7005@126} / () => any[] {TS7034@59,TS7005@126} / () => never[] {TS2322@94,TS2345@107} / () => any[]
    (
        "letCapturedLoopAssign",
        "() => any[]",
        "() => any[]",
        "() => never[]",
        "() => any[]",
    ),
    // () => number[] / () => number[] / () => never[] {TS2345@75} / () => any[]
    (
        "nestedEvolving",
        "() => number[]",
        "() => number[]",
        "() => never[]",
        "() => any[]",
    ),
    // () => (string | number)[] / () => (string | number)[] / () => never[] {TS2345@65,TS2345@94} / () => any[]
    (
        "closureReadsAfterPushLet",
        "() => (number | string)[]",
        "() => (number | string)[]",
        "() => never[]",
        "() => any[]",
    ),
    // () => (string | number)[] / () => (string | number)[] / () => never[] {TS2345@59,TS2345@88} / () => any[]
    (
        "nestedArrowPushLet",
        "() => (number | string)[]",
        "() => (number | string)[]",
        "() => never[]",
        "() => any[]",
    ),
    // () => number[] / () => number[] / () => never[] {TS2345@59,TS2345@101} / () => any[]
    (
        "nestedArrowReadLet",
        "() => number[]",
        "() => number[]",
        "() => never[]",
        "() => any[]",
    ),
    // () => any[] {TS7034@48,TS7005@105} / () => any[] {TS7034@48,TS7005@105} / () => never[] {TS2345@63,TS2345@92} / () => any[]
    (
        "nestedArrowPushConst",
        "() => any[]",
        "() => any[]",
        "() => never[]",
        "() => any[]",
    ),
    // () => any[] {TS7034@40,TS7005@92} / () => any[] {TS7034@40,TS7005@92} / () => never[] {TS2345@55} / () => any[]
    (
        "nestedLetReset",
        "() => any[]",
        "() => any[]",
        "() => never[]",
        "() => any[]",
    ),
];

// ──────────────────────────────────────────────────────────────────────
// The checker's evolving array (`autoArrayType`)
// ──────────────────────────────────────────────────────────────────────

/// Every `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` row of `table` answers its own project's measured
/// TypeScript 7.0.2 print ([`matrix_mismatches`]).
fn assert_measured_matrix(file: &str, source: &str, table: &[(&str, &str, &str, &str, &str)]) {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(&host, file, source, table);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Under `noImplicitAny` an unannotated declaration initialised to an
/// unparenthesized `[]` is the checker's EVOLVING array. Its element type
/// grows only at a `push` / `unshift` call on it (each argument adds the
/// base type of its literal, a spread argument its source's element
/// types, and `never` adds nothing) and at an element write `a[i] = v`
/// whose index is number-like (`a["k"] = 1` and a `string` or `number |
/// string` index add nothing); a compound write, an element inside a
/// destructuring target and a `length` write are ordinary references. A
/// read with nothing added is `any[]` (TS7034 at the declaration, TS7005
/// at the read); otherwise the array of the added types' subtype-reduced
/// union — with `strictNullChecks` off a widening `null` / `undefined`
/// element reads `any` alone. Without `noImplicitAny` the binding is
/// declared as the literal's widened type (`never[]`, `any[]` with
/// `strictNullChecks` off) and no operation retypes it.
#[test]
fn evolving_array_operations_grow_the_element_type() {
    assert_measured_matrix(
        "evolving-operations.ts",
        EVOLVING_OPERATIONS_SOURCE,
        EVOLVING_OPERATIONS_TABLE,
    );
}

/// Every reference to an evolving array that is not one of its operations
/// FINALIZES it there: it reads the array of the elements added so far
/// (`any[]` with none), and the array goes on evolving past it — `const b
/// = a; a.push(1)` leaves `b` `any[]`, and `a.push(1); const b = a;
/// a.push("s")` reads `number[]` in `b` and `(string | number)[]` in `a`.
#[test]
fn a_reference_finalizes_an_evolving_array_where_it_reads() {
    assert_measured_matrix(
        "evolving-finalizing.ts",
        EVOLVING_FINALIZING_SOURCE,
        EVOLVING_FINALIZING_TABLE,
    );
}

/// A write to an evolving-array binding takes the checker's
/// `autoArrayType` assignment rule: an unparenthesized `[]` starts a new
/// evolving array (`a = ([])` assigns `never[]`), any other value is held
/// when it is assignable to `any[]` and is `any[]` otherwise, and a join
/// of a path holding an ordinary value with an evolving one finalizes the
/// evolving one and reduces by subtype — `if (c) a = [1]` over an
/// untouched `let a = []` is `any[]`.
#[test]
fn writes_to_an_evolving_array_follow_the_auto_array_rule() {
    assert_measured_matrix(
        "evolving-assignments.ts",
        EVOLVING_ASSIGNMENTS_SOURCE,
        EVOLVING_ASSIGNMENTS_TABLE,
    );
}

/// Where paths meet, evolving arrays join by the union of their elements:
/// the arms of an `if`, a `switch` or a `try`, and the operands of a
/// conditional or logical expression, whose flow branches at the test in
/// statement and value position alike — `c ? a.push(1) : a` reads the
/// untouched array in its alternate.
#[test]
fn evolving_arrays_join_where_paths_meet() {
    assert_measured_matrix(
        "evolving-branches.ts",
        EVOLVING_BRANCHES_SOURCE,
        EVOLVING_BRANCHES_TABLE,
    );
}

/// In a loop, each reference's loop-head type is its entry type joined
/// with ONE pass of the iteration in which it reads its own entry type and
/// every other reference its own loop-head type: `a.push(a)` over `const
/// a = []; a.push(1)` is `(number | number[])[]`, never a growing nest,
/// while `a.push(v)` over `v = w; w = "s"` reaches the `string` two
/// iterations away. Breaks, `continue`s and returns leave from the pass
/// where every reference reads its loop-head type.
#[test]
fn evolving_arrays_in_loops_take_the_checkers_loop_head() {
    assert_measured_matrix(
        "evolving-loops.ts",
        EVOLVING_LOOPS_SOURCE,
        EVOLVING_LOOPS_TABLE,
    );
}

/// A nested function reads a captured evolving array by the checker's
/// capture rule: a `const` or `var` one, or a `let` one assigned after
/// the function is created or inside a nested function, reads the declared
/// `autoArrayType` (`any[]`), and an operation inside the function starts
/// from it; a `let` one past its last assignment continues from the array
/// it holds where the function is created, so an operation inside the
/// function evolves that array further.
#[test]
fn closures_read_a_captured_evolving_array_by_the_capture_rule() {
    assert_measured_matrix(
        "evolving-captures.ts",
        EVOLVING_CAPTURES_SOURCE,
        EVOLVING_CAPTURES_TABLE,
    );
}

/// Array types relate covariantly in their element (the checker's
/// `arrayVariances`), and a join reduces by the SUBTYPE relation, under
/// which `any` is below nothing but `any` / `unknown`: a return join of
/// `number[]` and `any[]` is `any[]`, of `string[]` and `(string |
/// number)[]` is `(string | number)[]`, and so is an array literal's
/// element union of the two.
const ARRAY_JOIN_SOURCE: &str = r#"
export function anyJoin(c: boolean, x: number[], y: any[]) { if (c) return x; return y; }
export function subtypeJoin(c: boolean, x: string[], y: (string | number)[]) { if (c) return x; return y; }
export function arrayOfArrays(x: number[], y: (string | number)[]) { return [x, y]; }
export function arrayOfLocals(x: number[], y: (string | number)[]) { const b = x; return [b, y]; }
"#;

/// Each row: the checker's print under strict, `strictNullChecks` off,
/// `noImplicitAny` off, and both off (TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`; no diagnostics); then the four answers.
const ARRAY_JOIN_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // any[] / any[] / any[] / any[]
    ("anyJoin", "any[]", "any[]", "any[]", "any[]"),
    // (string | number)[] / (string | number)[] / (string | number)[] / (string | number)[]
    (
        "subtypeJoin",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // (string | number)[][] / (string | number)[][] / (string | number)[][] / (string | number)[][]
    (
        "arrayOfArrays",
        "(number | string)[][]",
        "(number | string)[][]",
        "(number | string)[][]",
        "(number | string)[][]",
    ),
    // (string | number)[][] / (string | number)[][] / (string | number)[][] / (string | number)[][]
    (
        "arrayOfLocals",
        "(number | string)[][]",
        "(number | string)[][]",
        "(number | string)[][]",
        "(number | string)[][]",
    ),
];

#[test]
fn array_joins_reduce_by_the_covariant_subtype_relation() {
    assert_measured_matrix("array-joins.ts", ARRAY_JOIN_SOURCE, ARRAY_JOIN_TABLE);
}

/// Arithmetic binary expressions over parameters, literals, locals, enums and `any` / `unknown` / nullish operands.
const ARITHMETIC_SOURCE: &str = r#"
export function addNum(i: number) { return i + 1; }
export function addLit() { return 1 + 2; }
export function addStr(i: number, s: string) { return i + s; }
export function addStrLit(i: number) { return "a" + i; }
export function addAny(a: any, i: number) { return a + i; }
export function addAnyStr(a: any) { return a + "s"; }
export function addUnion(u: 1 | 2, i: number) { return u + i; }
export function addNumUnionStr(u: number | string, i: number) { return u + i; }
export function addBool(b: boolean, i: number) { return b + i; }
export function addNull(i: number) { return null + i; }
export function addUndef(u: undefined, i: number) { return u + i; }
export function addUnknown(u: unknown, i: number) { return u + i; }
export function addTemplate(i: number) { return `${i}` + i; }
export function subNum(i: number) { return i - 1; }
export function subAny(a: any) { return a - 1; }
export function subBigNum(a: bigint, i: number) { return a - i; }
export function subStr(s: string) { return s - 1; }
export function mulUnknown(u: unknown) { return u * 2; }
export function shiftNum(i: number) { return i << 2; }
export function powNum(i: number) { return i ** 2; }
export function bitAny(a: any, b: any) { return a | b; }
export function modBigAny(a: bigint, b: any) { return a % b; }
export function addEnum(e: E, i: number) { return e + i; }
export function addStrEnum(e: S, i: number) { return e + i; }
export function nestedArith(i: number, s: string) { return i + 1 + s; }
export function parenArith(i: number) { return (i + 1) * 2; }
export function localArith() { let x = 0; x = x + 1; return x; }
export function constArith() { const x = 1 + 1; return x; }
export function arrElem(i: number) { return [i + 1]; }
export function addBigParams(a: bigint, b: bigint) { return a + b; }
export function mulBigParams(a: bigint, b: bigint) { return a * b; }
export function subBigLit(a: bigint, b: 1n | 2n) { return a - b; }
export function parenAssignNarrow() { let x: string | number = "s"; (x) = 5; return x; }
enum E { A, B }
enum S { A = "a" }
"#;

/// Each row: the checker's print under strict, `strictNullChecks` off,
/// `noImplicitAny` off, and both off (TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`), with the diagnostics each reports on the
/// function's line (`code@column`); then the four answers.
const ARITHMETIC_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number / number / number / number
    ("addNum", "number", "number", "number", "number"),
    // number / number / number / number
    ("addLit", "number", "number", "number", "number"),
    // string / string / string / string
    ("addStr", "string", "string", "string", "string"),
    // string / string / string / string
    ("addStrLit", "string", "string", "string", "string"),
    // any / any / any / any
    ("addAny", "any", "any", "any", "any"),
    // string / string / string / string
    ("addAnyStr", "string", "string", "string", "string"),
    // number / number / number / number
    ("addUnion", "number", "number", "number", "number"),
    // any {TS2365@72} / any {TS2365@72} / any {TS2365@72} / any {TS2365@72}
    ("addNumUnionStr", "any", "any", "any", "any"),
    // any {TS2365@57} / any {TS2365@57} / any {TS2365@57} / any {TS2365@57}
    ("addBool", "any", "any", "any", "any"),
    // any {TS18050@45} / any {TS2365@45} / any {TS18050@45} / any {TS2365@45}
    ("addNull", "any", "any", "any", "any"),
    // any {TS18048@60} / any {TS2365@60} / any {TS18048@60} / any {TS2365@60}
    ("addUndef", "any", "any", "any", "any"),
    // any {TS18046@60} / any {TS2365@60} / any {TS18046@60} / any {TS2365@60}
    ("addUnknown", "any", "any", "any", "any"),
    // string / string / string / string
    ("addTemplate", "string", "string", "string", "string"),
    // number / number / number / number
    ("subNum", "number", "number", "number", "number"),
    // number / number / number / number
    ("subAny", "number", "number", "number", "number"),
    // any {TS2365@58} / any {TS2365@58} / any {TS2365@58} / any {TS2365@58}
    ("subBigNum", "any", "any", "any", "any"),
    // number {TS2362@44} / number {TS2362@44} / number {TS2362@44} / number {TS2362@44}
    ("subStr", "number", "number", "number", "number"),
    // number {TS18046@49} / number {TS2362@49} / number {TS18046@49} / number {TS2362@49}
    ("mulUnknown", "number", "number", "number", "number"),
    // number / number / number / number
    ("shiftNum", "number", "number", "number", "number"),
    // number / number / number / number
    ("powNum", "number", "number", "number", "number"),
    // number / number / number / number
    ("bitAny", "number", "number", "number", "number"),
    // bigint / bigint / bigint / bigint
    ("modBigAny", "bigint", "bigint", "bigint", "bigint"),
    // number / number / number / number
    ("addEnum", "number", "number", "number", "number"),
    // string / string / string / string
    ("addStrEnum", "string", "string", "string", "string"),
    // string / string / string / string
    ("nestedArith", "string", "string", "string", "string"),
    // number / number / number / number
    ("parenArith", "number", "number", "number", "number"),
    // number / number / number / number
    ("localArith", "number", "number", "number", "number"),
    // number / number / number / number
    ("constArith", "number", "number", "number", "number"),
    // number[] / number[] / number[] / number[]
    ("arrElem", "number[]", "number[]", "number[]", "number[]"),
    // bigint / bigint / bigint / bigint
    ("addBigParams", "bigint", "bigint", "bigint", "bigint"),
    // bigint / bigint / bigint / bigint
    ("mulBigParams", "bigint", "bigint", "bigint", "bigint"),
    // bigint / bigint / bigint / bigint
    ("subBigLit", "bigint", "bigint", "bigint", "bigint"),
    // number / number / number / number
    ("parenAssignNarrow", "number", "number", "number", "number"),
];

/// Assignments and updates through TS carriers: through non-null `!` and parentheses they assign the binding; through a type assertion (`as`, `satisfies`, `<T>`) they do not.
const ASSERTED_TARGETS_SOURCE: &str = r#"
export function asAssignNum() { let a = []; a.push(1); (a as any) = 5; return a; }
export function asAssignLet() { let x: string | number = "s"; (x as any) = 5; return x; }
export function asAssignNarrow(y: string | number) { let x = y; if (typeof x === "string") { (x as any) = 5; return x; } return 0; }
export function asAssignLiteral() { let x = "s" as string | number; x = 1; (x as any) = "t"; return x; }
export function parenAssignNarrow() { let x: string | number = "s"; (x) = 5; return x; }
export function nonNullAssign() { let x: string | number = "s"; x! = 5; return x; }
export function satisfiesAssign() { let x: string | number = "s"; (x satisfies string | number) = 5; return x; }
export function angleAssign() { let x: string | number = "s"; (<any>x) = 5; return x; }
export function asUpdate() { let x: string | number = "s"; (x as any)++; return x; }
export function asCaptureAfter() { let x: string | number = "s"; (x as any) = 5; return () => x; }
export function asArrayWrite() { let a = []; a.push(1); (a as any)[0] = "s"; return a; }
"#;

/// Each row: the checker's print under strict, `strictNullChecks` off,
/// `noImplicitAny` off, and both off (TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`), with the diagnostics each reports on the
/// function's line (`code@column`); then the four answers.
const ASSERTED_TARGETS_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number[] / number[] / never[] {TS2345@52} / any[]
    ("asAssignNum", "number[]", "number[]", "never[]", "any[]"),
    // string / string / string / string
    ("asAssignLet", "string", "string", "string", "string"),
    // string | 0 / string | 0 / string | 0 / string | 0
    (
        "asAssignNarrow",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // number / number / number / number
    ("asAssignLiteral", "number", "number", "number", "number"),
    // number / number / number / number
    ("parenAssignNarrow", "number", "number", "number", "number"),
    // number / number / number / number
    ("nonNullAssign", "number", "number", "number", "number"),
    // string {TS2322@67} / string {TS2322@67} / string {TS2322@67} / string {TS2322@67}
    ("satisfiesAssign", "string", "string", "string", "string"),
    // string / string / string / string
    ("angleAssign", "string", "string", "string", "string"),
    // string / string / string / string
    ("asUpdate", "string", "string", "string", "string"),
    // () => string / () => string / () => string / () => string
    (
        "asCaptureAfter",
        "() => string",
        "() => string",
        "() => string",
        "() => string",
    ),
    // number[] / number[] / never[] {TS2345@53} / any[]
    ("asArrayWrite", "number[]", "number[]", "never[]", "any[]"),
];

/// Writes to ordinary variables in loops, and updates in control tests.
const LOOP_WRITES_SOURCE: &str = r#"
export function counterFor(n: number) { let x = 0; for (let i = 0; i < n; i++) { x = x + 1; } return x; }
export function widenFor(n: number) { let x: string | number = 0; for (let i = 0; i < n; i++) { x = "s"; } return x; }
export function whileWrite(n: number) { let x: string | number | boolean = 0; while (n-- > 0) { x = "s"; } return x; }
export function doWrite(n: number) { let x: string | number | boolean = 0; do { x = true; } while (n-- > 0); return x; }
export function breakWrite(n: number) { let x: string | number | boolean = 0; for (let i = 0; i < n; i++) { if (i > 2) break; x = "s"; } return x; }
export function continueWrite(n: number) { let x: string | number | boolean = 0; for (let i = 0; i < n; i++) { if (i > 2) continue; x = "s"; } return x; }
export function autoLoop(n: number) { let x; for (let i = 0; i < n; i++) { x = i; } return x; }
export function autoNullLoop(n: number) { let x = null; for (let i = 0; i < n; i++) { x = "s"; } return x; }
export function narrowedAfter(n: number) { let x: string | number = 0; while (n > 0) { x = "s"; n--; } return typeof x === "string" ? x : 1; }
export function incLoop(n: number) { let x = 0; while (x < n) { x++; } return x; }
export function varLoop(n: number) { var x: string | number = 0; for (let i = 0; i < n; i++) { x = "s"; } return x; }
export function returnInLoopWrite(n: number) { let x: string | number = 0; for (let i = 0; i < n; i++) { x = "s"; if (i) return x; } return x; }
export function autoReadInBody(n: number) { let x; for (let i = 0; i < n; i++) { if (i > 0) return x; x = i; } return 0; }
export function varReadInBody(n: number) { var x: string | number = 0; for (let i = 0; i < n; i++) { if (i > 0) return x; x = "s"; } return true; }
export function doFalseWrite() { let x: "a" | "b" | "c" = "a"; do { x = "b" } while (false); return x }
export function doFalseTwo(f: boolean) { let x: "a" | "b" | "c" = "a"; do { if (f) { x = "b" } else { x = "c" } } while (false); return x }
export function ifTestUpdate(n: number) { if (n-- > 0) { return n; } return 1; }
"#;

/// Each row: the checker's print under strict, `strictNullChecks` off,
/// `noImplicitAny` off, and both off (TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`), with the diagnostics each reports on the
/// function's line (`code@column`); then the four answers.
const LOOP_WRITES_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "b" / "b" / "b" / "b"
    ("doFalseWrite", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b" | "c" / "b" | "c" / "b" | "c" / "b" | "c"
    (
        "doFalseTwo",
        "\"b\" | \"c\"",
        "\"b\" | \"c\"",
        "\"b\" | \"c\"",
        "\"b\" | \"c\"",
    ),
    // number | undefined / number / any / any
    (
        "autoReadInBody",
        "number | undefined",
        "number",
        "any",
        "any",
    ),
    // string | number | true / string | number | true / string | number | true / string | number | true
    (
        "varReadInBody",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // number / number / number / number
    ("counterFor", "number", "number", "number", "number"),
    // string | number / string | number / string | number / string | number
    (
        "widenFor",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number / string | number / string | number / string | number
    (
        "whileWrite",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // boolean / boolean / boolean / boolean
    ("doWrite", "boolean", "boolean", "boolean", "boolean"),
    // string | number / string | number / string | number / string | number
    (
        "breakWrite",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number / string | number / string | number / string | number
    (
        "continueWrite",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // number | undefined / number / any / any
    ("autoLoop", "number | undefined", "number", "any", "any"),
    // string | null / string / null {TS2322@87} / any
    ("autoNullLoop", "null | string", "string", "null", "any"),
    // string | 1 / string | 1 / string | 1 / string | 1
    (
        "narrowedAfter",
        "1 | string",
        "1 | string",
        "1 | string",
        "1 | string",
    ),
    // number / number / number / number
    ("incLoop", "number", "number", "number", "number"),
    // string | number / string | number / string | number / string | number
    (
        "varLoop",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number / string | number / string | number / string | number
    (
        "returnInLoopWrite",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // number / number / number / number
    ("ifTestUpdate", "number", "number", "number", "number"),
];

/// Logical expressions whose right operand writes a binding, in value and statement position.
const VALUE_LOGICAL_SOURCE: &str = r#"
export function valueLogicalRead(c: boolean) { const a = []; const r = c && a.push(1); return [r, a]; }
export function valueOrRead(c: boolean) { const a = []; const r = c || a.push(1); return [r, a]; }
export function valueCoalesceRead(x: number | null) { const a = []; const r = x ?? a.push("s"); return [r, a]; }
export function valueLogicalOnly(c: boolean) { const a = []; return c && a.push(1); }
export function valueStrLeft(s: string) { const a = []; const r = s && a.push(1); return [r, a]; }
export function valueNumOr(n: number) { const a = []; const r = n || a.push(1); return [r, a]; }
export function valueObjAnd(o: { k: 1 }) { const a = []; const r = o && a.push(1); return [r, a]; }
export function valueNullOr(v: string | null) { const a = []; const r = v || a.push(1); return [r, a]; }
export function valueUndefCoalesce(v?: string) { const a = []; const r = v ?? a.push(1); return [r, a]; }
export function stmtAssignAnd(c: boolean) { let x: string | number = "s"; c && (x = 1); return x; }
export function valueNarrowAnd(v: string | null) { const a = []; const r = v !== null && a.push(v); return [r, a]; }
"#;

/// Each row: the checker's print under strict, `strictNullChecks` off,
/// `noImplicitAny` off, and both off (TypeScript 7.0.2, `--declaration
/// --emitDeclarationOnly`), with the diagnostics each reports on the
/// function's line (`code@column`); then the four answers.
const VALUE_LOGICAL_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // (number | false | number[])[] / (number | number[])[] / (number | false | never[])[] {TS2345@84} / (number | any[])[]
    (
        "valueLogicalRead",
        "(false | number | number[])[]",
        "(number | number[])[]",
        "(false | never[] | number)[]",
        "(any[] | number)[]",
    ),
    // (number | true | number[])[] / (number | true | number[])[] / (number | true | never[])[] {TS2345@79} / (number | true | any[])[]
    (
        "valueOrRead",
        "(number | number[] | true)[]",
        "(number | number[] | true)[]",
        "(never[] | number | true)[]",
        "(any[] | number | true)[]",
    ),
    // (number | string[])[] / (number | string[])[] / (number | never[])[] {TS2345@91} / (number | any[])[]
    (
        "valueCoalesceRead",
        "(number | string[])[]",
        "(number | string[])[]",
        "(never[] | number)[]",
        "(any[] | number)[]",
    ),
    // number | false / number / number | false {TS2345@81} / number
    (
        "valueLogicalOnly",
        "false | number",
        "number",
        "false | number",
        "number",
    ),
    // (number | "" | number[])[] / (number | number[])[] / (number | "" | never[])[] {TS2345@79} / (number | any[])[]
    (
        "valueStrLeft",
        "(\"\" | number | number[])[]",
        "(number | number[])[]",
        "(\"\" | never[] | number)[]",
        "(any[] | number)[]",
    ),
    // (number | number[])[] / (number | number[])[] / (number | never[])[] {TS2345@77} / (number | any[])[]
    (
        "valueNumOr",
        "(number | number[])[]",
        "(number | number[])[]",
        "(never[] | number)[]",
        "(any[] | number)[]",
    ),
    // (number | number[])[] / (number | number[])[] / (number | never[])[] {TS2345@80} / (number | any[])[]
    (
        "valueObjAnd",
        "(number | number[])[]",
        "(number | number[])[]",
        "(never[] | number)[]",
        "(any[] | number)[]",
    ),
    // (string | number | number[])[] / (string | number | number[])[] / (string | number | never[])[] {TS2345@85} / (string | number | any[])[]
    (
        "valueNullOr",
        "(number | number[] | string)[]",
        "(number | number[] | string)[]",
        "(never[] | number | string)[]",
        "(any[] | number | string)[]",
    ),
    // (string | number | number[])[] / (string | number | number[])[] / (string | number | never[])[] {TS2345@86} / (string | number | any[])[]
    (
        "valueUndefCoalesce",
        "(number | number[] | string)[]",
        "(number | number[] | string)[]",
        "(never[] | number | string)[]",
        "(any[] | number | string)[]",
    ),
    // string | number / string | number / string | number / string | number
    (
        "stmtAssignAnd",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // (number | false | string[])[] / (number | string[])[] / (number | false | never[])[] {TS2345@97} / (number | any[])[]
    (
        "valueNarrowAnd",
        "(false | number | string[])[]",
        "(number | string[])[]",
        "(false | never[] | number)[]",
        "(any[] | number)[]",
    ),
];

/// An arithmetic binary expression takes the checker's rule
/// (`checkBinaryLikeExpression`). `+` is `number` over two number-like
/// operands, `bigint` over two bigint-like ones, `string` when either is
/// string-like, and `any` when either is `any`, where "like" never admits
/// `any`, `unknown`, `void`, `null` or `undefined`. Every other combination
/// is the checker's error, typed `any` (`b + i` over `b: boolean`, `null +
/// i`). Every other arithmetic operator is `number` unless an operand may be
/// a bigint, and `bigint` over two bigint-like operands (`any` admitted),
/// else `any`.
#[test]
fn arithmetic_expressions_take_the_checkers_operand_rule() {
    assert_measured_matrix("arithmetic.ts", ARITHMETIC_SOURCE, ARITHMETIC_TABLE);
}

/// The checker assigns through parentheses and non-null `!` and never
/// through a type assertion: `(x as any) = 5`, `(<any>x) = 5` and `(x
/// satisfies T) = 5` leave `x` as it was — it stays past its last
/// assignment for a closure, and an evolving array keeps its elements —
/// while `x! = 5` and `(x) = 5` retype it. An update through a type
/// assertion (`(x as any)++`) likewise leaves the binding alone.
#[test]
fn assignments_through_a_type_assertion_are_not_assignments() {
    assert_measured_matrix(
        "asserted-targets.ts",
        ASSERTED_TARGETS_SOURCE,
        ASSERTED_TARGETS_TABLE,
    );
}

/// Writes to ordinary variables in a loop take each reference's loop-head
/// type (its entry type joined with one pass of the body) rather than the
/// loop refusal, through `for`, `while` and `do`, `break`, `continue` and a
/// `return` inside the body; an initializer-less `let` or a `var` written
/// in the body joins its never-assigned entry path as an `if` does. An
/// update a control test evaluates on every path (`while (n-- > 0)`,
/// `if (n-- > 0)`) applies before the test branches.
#[test]
fn loop_writes_take_each_references_loop_head() {
    assert_measured_matrix("loop-writes.ts", LOOP_WRITES_SOURCE, LOOP_WRITES_TABLE);
}

/// A logical expression whose right operand writes a binding branches at
/// its left operand: the right operand runs on the edge the left one does
/// not short-circuit, under the left operand's guard, the paths join, and
/// the value is the checker's logical result — `&&` the left operand's
/// definitely-falsy part with the right operand (with `strictNullChecks`
/// off, the right operand's base type's), `||` the left operand without its
/// falsy parts with the right operand, `??` the non-nullable left operand
/// with the right one.
#[test]
fn logical_expressions_that_write_branch_at_their_left_operand() {
    assert_measured_matrix(
        "value-logical.ts",
        VALUE_LOGICAL_SOURCE,
        VALUE_LOGICAL_TABLE,
    );
}
