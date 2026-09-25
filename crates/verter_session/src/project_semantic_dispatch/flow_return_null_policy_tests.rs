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
            let mut members: Vec<String> = surface
                .index_signatures
                .iter()
                .map(|signature| {
                    format!(
                        "{}[k: {}]: {}",
                        if signature.readonly { "readonly " } else { "" },
                        answer_text(host, signature.key_type),
                        answer_text(host, signature.value_type)
                    )
                })
                .collect();
            members.extend(surface.positive_members().iter().map(|member| {
                format!(
                    "{}{}{}: {}",
                    if member.readonly { "readonly " } else { "" },
                    member.key.as_string().unwrap_or("<key>"),
                    if member.optional { "?" } else { "" },
                    answer_text(host, member.value)
                )
            }));
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
                        "{}{}{}: {}",
                        if param.rest { "..." } else { "" },
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

/// A yield the join does not model — nested in another expression, or a
/// `yield*` delegation — never leaves a partial yield type, while a yield
/// inside a loop joins like any other. TypeScript 7.0.2 types every one of
/// these (`genWhile`, `genNested`, `genInCall` and `genDelegate` are
/// `Generator<string, void, unknown>`; `genWhileBare` is
/// `Generator<undefined, void, unknown>`, `Generator<any, void, unknown>`
/// with `strictNullChecks` off); the lane answers the loops' yields and a
/// typed gap for the rest.
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
        assert_eq!(
            observe(&host, &canonical, "genWhile"),
            "Generator<string, void, unknown>",
            "{canonical} `genWhile`"
        );
        assert_eq!(
            observe(&host, &canonical, "genWhileBare"),
            if root == STRICT_ROOT {
                "Generator<undefined, void, unknown>"
            } else {
                "Generator<any, void, unknown>"
            },
            "{canonical} `genWhileBare`"
        );
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
type Box<T> = T extends null ? "none" : { v: T };
interface Foo { a: string }
class Bar { b = 1 }
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
export function nonNullableIface(v: NonNullable<Foo | null>) { return v; }
export function nonNullableClass(v: NonNullable<Bar | undefined>) { return v; }
export function nonNullableUnion(v: NonNullable<string | number | null>) { return v; }
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
/// answer. The library `NonNullable<T>` (`T & {}`) prints its reduced
/// type for both projects, with or without the null member the strict
/// argument keeps: `NonNullable<string | null>` is `string`, and over an
/// interface or class reference it is that reference (`Foo`, `Bar`). A
/// result that is still a union (`NonNullable<string | number | null>`)
/// is the union here, where the checker's print keeps the alias name it
/// attaches to that union — the one presentation difference, the two
/// being the same type.
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
        // string / string
        (STRICT_ROOT, "nonNullableParam", "string"),
        (LOOSE_ROOT, "nonNullableParam", "string"),
        // Foo / Foo
        (STRICT_ROOT, "nonNullableIface", "Foo"),
        (LOOSE_ROOT, "nonNullableIface", "Foo"),
        // Bar / Bar
        (STRICT_ROOT, "nonNullableClass", "Bar"),
        (LOOSE_ROOT, "nonNullableClass", "Bar"),
        // NonNullable<string | number | null> / NonNullable<string | number>
        // (the alias the checker names the union by)
        (STRICT_ROOT, "nonNullableUnion", "number | string"),
        (LOOSE_ROOT, "nonNullableUnion", "number | string"),
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

/// Loops holding returns, yields, breaks, continues and writes: every loop form, labeled jumps, a test that narrows, element bindings, dependent writes and an inferred binding the checker cannot type.
const LOOP_SOURCE: &str = r#"
interface Generator<T, TReturn, TNext> {}
declare function assertNumber(v: unknown): asserts v is number;
export function retInFor(n: number) { for (let i = 0; i < n; i++) { if (i > 3) return "big"; } return 0; }
export function retInForOf(xs: string[]) { for (const x of xs) { if (x) return x; } return null; }
export function retLoopVar(c: boolean) { let x: string | number = 1; while (c) { if (c) return x; x = "s"; } return 0; }
export function retLoopVar3(c: boolean) { let x: string | number | boolean = 1; while (c) { if (c) return x; if (x === 1) { x = "s"; } else { x = true; } } return 0; }
export function retDoWhile(c: boolean) { let x: string | number | boolean = 1; do { if (c) return x; x = "s"; } while (c); return x; }
export function retDoWhileFalse(c: boolean) { let x: "a" | "b" = "a"; do { if (c) x = "b"; } while (false); return x; }
export function retForIn(o: object) { for (const k in o) { return k; } return 0; }
export function retWhileTrue(c: boolean) { let x: number | string = 1; while (true) { if (c) return x; x = "s"; } }
export function retBreak(c: boolean) { let x: string | number | boolean = 1; while (c) { if (c) { x = true; break; } x = "s"; } return x; }
export function retContinue(c: boolean) { let x: string | number | boolean = 1; while (c) { if (c) { x = true; continue; } x = "s"; if (c) return x; } return 0; }
export function retLabeled(c: boolean) { let x: string | number | boolean = 1; outer: while (c) { while (c) { if (c) return x; x = "s"; if (c) break outer; } x = true; } return x; }
export function retContinueOuter(c: boolean) { let x: string | number = 1; outer: while (c) { while (c) { x = "s"; continue outer; } } return x; }
export function retWriteAfter(c: boolean) { let x: string | number = 1; while (c) { x = "s"; } return x; }
export function retNarrowTest(x: string | number) { while (typeof x === "string") { return x; } return x; }
export function retNarrowTestW(y: string | number) { let x = y; while (typeof x === "string") { x = 1; } return x; }
export function retVarLoop(n: number) { for (var i = 0; i < n; i++) { if (i) return i; } return "none"; }
export function retVarAfter(n: number) { for (var i = 0; i < n; i++) {} return i; }
export function retForOfLet(xs: number[]) { let last: number | string = "none"; for (const x of xs) { last = x; } return last; }
export function retForOfNarrow(xs: (string | number)[]) { for (const x of xs) { if (typeof x === "string") return x; } return 0; }
export function retForOfTupleArr(xs: [number, string]) { for (const x of xs) { if (typeof x === "string") return x; } return true; }
export function retForOfString(s: string) { for (const ch of s) { return ch; } return 0; }
export function retChain(c: boolean) { let x: 1 | 2 | 3 = 1; while (c) { if (x === 1) x = 2; else if (x === 2) x = 3; } return x; }
export function retChainIn(c: boolean) { let x: 1 | 2 | 3 = 1; while (c) { if (x === 3) return x; if (x === 1) x = 2; else if (x === 2) x = 3; } return 0; }
export function retChain4(c: boolean) { let x: 1 | 2 | 3 | 4 = 1; while (c) { if (x === 1) x = 2; else if (x === 2) x = 3; else if (x === 3) x = 4; } return x; }
export function retTwoVars(c: boolean) { let a: 1 | 2 | 3 = 1; let b: 1 | 2 | 3 = 1; while (c) { a = b; b = 2; if (c) b = 3; } return a; }
export function retSwap(c: boolean) { let a: 1 | 2 | 3 = 1; let b: 1 | 2 | 3 = 2; while (c) { const t = a; a = b; b = t; } return [a, b]; }
export function retNarrowChain(c: boolean) { let x: string | number | boolean = 1; while (c) { if (typeof x === "number") x = "s"; else if (typeof x === "string") x = true; } return x; }
export function retAssertInLoop(x: string | number) { do { assertNumber(x); break; } while (true); return x; }
export function retLabeledExit(x: "a" | "b") { exit: while (true) { if (x === "a") break exit; throw 0; } return x; }
export function retCompoundLoop(n: number) { let total = 0; for (let i = 0; i < n; i++) { total += i; } return total; }
export function retBreakOuterBlock(x: string | null) { outer: { if (x === null) { for (;;) { break outer } } return 0 } return x }
export function* yieldLoop(n: number) { for (let i = 0; i < n; i++) yield i; }
export function* yieldLoopVar(c: boolean) { let x: string | number = 1; while (c) { yield x; x = "s"; } }
export function* yieldForOf(xs: string[]) { for (const x of xs) yield x; return 1; }
export function* yieldDoWhile(c: boolean) { let x: string | number | boolean = 1; do { yield x; x = c ? "s" : true; } while (c); }
export function* yieldBoth(c: boolean) { while (c) { yield 1; if (c) return "done"; } return 0; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`LOOP_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const LOOP_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "big" | 0 / "big" | 0 / "big" | 0 / "big" | 0
    (
        "retInFor",
        "\"big\" | 0",
        "\"big\" | 0",
        "\"big\" | 0",
        "\"big\" | 0",
    ),
    // string | null / string / string | null / string
    (
        "retInForOf",
        "null | string",
        "string",
        "null | string",
        "string",
    ),
    // string | number / string | number / string | number / string | number
    (
        "retLoopVar",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number | true / string | number | true / string | number | true / string | number | true
    (
        "retLoopVar3",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | number / string | number / string | number / string | number
    (
        "retDoWhile",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // "a" | "b" / "a" | "b" / "a" | "b" / "a" | "b"
    (
        "retDoWhileFalse",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // string | 0 / string | 0 / string | 0 / string | 0
    (
        "retForIn",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // string | number / string | number / string | number / string | number
    (
        "retWhileTrue",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number | true / string | number | true / string | number | true / string | number | true
    (
        "retBreak",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | 0 / string | 0 / string | 0 / string | 0
    (
        "retContinue",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // string | number | true / string | number | true / string | number | true / string | number | true
    (
        "retLabeled",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | number / string | number / string | number / string | number
    (
        "retContinueOuter",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number / string | number / string | number / string | number
    (
        "retWriteAfter",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number / string | number / string | number / string | number
    (
        "retNarrowTest",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // number / number / number / number
    ("retNarrowTestW", "number", "number", "number", "number"),
    // number | "none" / number | "none" / number | "none" / number | "none"
    (
        "retVarLoop",
        "\"none\" | number",
        "\"none\" | number",
        "\"none\" | number",
        "\"none\" | number",
    ),
    // number / number / number / number
    ("retVarAfter", "number", "number", "number", "number"),
    // string | number / string | number / string | number / string | number
    (
        "retForOfLet",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | 0 / string | 0 / string | 0 / string | 0
    (
        "retForOfNarrow",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // string | true / string | true / string | true / string | true
    (
        "retForOfTupleArr",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | 0 / string | 0 / string | 0 / string | 0
    (
        "retForOfString",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // 1 | 2 | 3 / 1 | 2 | 3 / 1 | 2 | 3 / 1 | 2 | 3
    (
        "retChain",
        "1 | 2 | 3",
        "1 | 2 | 3",
        "1 | 2 | 3",
        "1 | 2 | 3",
    ),
    // 0 | 3 / 0 | 3 / 0 | 3 / 0 | 3
    ("retChainIn", "0 | 3", "0 | 3", "0 | 3", "0 | 3"),
    // 1 | 2 | 3 | 4 / 1 | 2 | 3 | 4 / 1 | 2 | 3 | 4 / 1 | 2 | 3 | 4
    (
        "retChain4",
        "1 | 2 | 3 | 4",
        "1 | 2 | 3 | 4",
        "1 | 2 | 3 | 4",
        "1 | 2 | 3 | 4",
    ),
    // 1 | 2 | 3 / 1 | 2 | 3 / 1 | 2 | 3 / 1 | 2 | 3
    (
        "retTwoVars",
        "1 | 2 | 3",
        "1 | 2 | 3",
        "1 | 2 | 3",
        "1 | 2 | 3",
    ),
    // (1 | 2 | 3)[] / (1 | 2 | 3)[] / (1 | 2 | 3)[] / (1 | 2 | 3)[]
    (
        "retSwap",
        "(1 | 2 | 3)[]",
        "(1 | 2 | 3)[]",
        "(1 | 2 | 3)[]",
        "(1 | 2 | 3)[]",
    ),
    // string | number | true / string | number | true / string | number | true / string | number | true
    (
        "retNarrowChain",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // number / number / number / number
    ("retAssertInLoop", "number", "number", "number", "number"),
    // "a" / "a" / "a" / "a"
    ("retLabeledExit", "\"a\"", "\"a\"", "\"a\"", "\"a\""),
    // number / number / number / number
    ("retCompoundLoop", "number", "number", "number", "number"),
    // 0 | null / string | 0 / 0 | null / string | 0
    (
        "retBreakOuterBlock",
        "0 | null",
        "0 | string",
        "0 | null",
        "0 | string",
    ),
    // Generator<number, void, unknown> / Generator<number, void, unknown> / Generator<number, void, unknown> / Generator<number, void, unknown>
    (
        "yieldLoop",
        "Generator<number, void, unknown>",
        "Generator<number, void, unknown>",
        "Generator<number, void, unknown>",
        "Generator<number, void, unknown>",
    ),
    // Generator<string | number, void, unknown> / Generator<string | number, void, unknown> / Generator<string | number, void, unknown> / Generator<string | number, void, unknown>
    (
        "yieldLoopVar",
        "Generator<number | string, void, unknown>",
        "Generator<number | string, void, unknown>",
        "Generator<number | string, void, unknown>",
        "Generator<number | string, void, unknown>",
    ),
    // Generator<string, number, unknown> / Generator<string, number, unknown> / Generator<string, number, unknown> / Generator<string, number, unknown>
    (
        "yieldForOf",
        "Generator<string, number, unknown>",
        "Generator<string, number, unknown>",
        "Generator<string, number, unknown>",
        "Generator<string, number, unknown>",
    ),
    // Generator<string | number | true, void, unknown> / Generator<string | number | true, void, unknown> / Generator<string | number | true, void, unknown> / Generator<string | number | true, void, unknown>
    (
        "yieldDoWhile",
        "Generator<number | string | true, void, unknown>",
        "Generator<number | string | true, void, unknown>",
        "Generator<number | string | true, void, unknown>",
        "Generator<number | string | true, void, unknown>",
    ),
    // Generator<number, "done" | 0, unknown> / Generator<number, "done" | 0, unknown> / Generator<number, "done" | 0, unknown> / Generator<number, "done" | 0, unknown>
    (
        "yieldBoth",
        "Generator<number, \"done\" | 0, unknown>",
        "Generator<number, \"done\" | 0, unknown>",
        "Generator<number, \"done\" | 0, unknown>",
        "Generator<number, \"done\" | 0, unknown>",
    ),
];

/// A loop is evaluated to the checker's fixed point: the state at its head
/// joins the state entering it with every back edge (the body's end and
/// each `continue`, after a `for` update), iterated until it stops
/// changing, and every return, yield and break of the body reads the
/// converged head. `retChain` reaches `3` only on the third pass;
/// `retSwap` binds the checker's `any` for `t`, whose initializer the
/// loop feeds from `t` itself through a union-declared binding (TS7022
/// under `noImplicitAny`). Each cell matches its own project's
/// TypeScript 7.0.2 answer.
#[test]
fn loops_iterate_to_the_checkers_fixed_point() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(&host, "loops.ts", LOOP_SOURCE, LOOP_TABLE);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Returns and yields no path reaches: after a return, an exhaustive `if`, a labeled break, a throw, a loop that never completes, a literal-`false` loop test, and an abrupt `finally` crossed by a break.
const UNREACHABLE_SOURCE: &str = r#"
interface Generator<T, TReturn, TNext> {}
export function afterReturn(x: string) { return 1; return x; }
export function afterIfElse(x: string | number) { if (typeof x === "string") { return 1; } else { return 2; } return x; }
export function afterBreakOuter(x: string | null) { outer: { break outer; return 0; } return x; }
export function afterThrow(x: string | number) { throw 0; return x; }
export function afterWriteReadsDeclared(x: string | number) { x = "s"; return 1; return x; }
export function afterReturnString() { return 1; return "a"; }
export function deadWhile() { while (false) { return 1; } return "a"; }
export function deadFor() { for (let i = 0; false; i++) { return i; } return "a"; }
export function afterForever(x: string | null) { outer: { for (;;) { break outer } return 0 } return x }
export function breakFinally() { while (true) { try { break } finally { return "a" as const } } }
export function breakFinallyAfter(c: boolean) { while (c) { try { break } finally { return "a" as const } } return "b" as const; }
export function switchBreakFinally(x: number) { switch (x) { case 1: try { break } finally { return "a" as const } } }
export function labeledFinallyThen() { OUT: INNER: { try { break INNER; } finally { return "a" as const; } } return "b" as const; }
export function* yieldAfterReturn() { yield 1; return 0; yield "s"; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`UNREACHABLE_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const UNREACHABLE_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string | 1 / string | 1 / string | 1 / string | 1
    (
        "afterReturn",
        "1 | string",
        "1 | string",
        "1 | string",
        "1 | string",
    ),
    // string | number / string | number / string | number / string | number
    (
        "afterIfElse",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | 0 | null / string | 0 / string | 0 | null / string | 0
    (
        "afterBreakOuter",
        "0 | null | string",
        "0 | string",
        "0 | null | string",
        "0 | string",
    ),
    // string | number / string | number / string | number / string | number
    (
        "afterThrow",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number / string | number / string | number / string | number
    (
        "afterWriteReadsDeclared",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // "a" | 1 / "a" | 1 / "a" | 1 / "a" | 1
    (
        "afterReturnString",
        "\"a\" | 1",
        "\"a\" | 1",
        "\"a\" | 1",
        "\"a\" | 1",
    ),
    // "a" | 1 / "a" | 1 / "a" | 1 / "a" | 1
    (
        "deadWhile",
        "\"a\" | 1",
        "\"a\" | 1",
        "\"a\" | 1",
        "\"a\" | 1",
    ),
    // number | "a" / number | "a" / number | "a" / number | "a"
    (
        "deadFor",
        "\"a\" | number",
        "\"a\" | number",
        "\"a\" | number",
        "\"a\" | number",
    ),
    // string | 0 | null / string | 0 / string | 0 | null / string | 0
    (
        "afterForever",
        "0 | null | string",
        "0 | string",
        "0 | null | string",
        "0 | string",
    ),
    // "a" | undefined / "a" / "a" | undefined / "a"
    (
        "breakFinally",
        "\"a\" | undefined",
        "\"a\"",
        "\"a\" | undefined",
        "\"a\"",
    ),
    // "a" | "b" / "a" | "b" / "a" | "b" / "a" | "b"
    (
        "breakFinallyAfter",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "a" | undefined / "a" / "a" | undefined / "a"
    (
        "switchBreakFinally",
        "\"a\" | undefined",
        "\"a\"",
        "\"a\" | undefined",
        "\"a\"",
    ),
    // "a" | "b" / "a" | "b" / "a" | "b" / "a" | "b"
    (
        "labeledFinallyThen",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // Generator<"s" | 1, number, unknown> / Generator<"s" | 1, number, unknown> / Generator<"s" | 1, number, unknown> / Generator<"s" | 1, number, unknown>
    (
        "yieldAfterReturn",
        "Generator<\"s\" | 1, number, unknown>",
        "Generator<\"s\" | 1, number, unknown>",
        "Generator<\"s\" | 1, number, unknown>",
        "Generator<\"s\" | 1, number, unknown>",
    ),
];

/// The checker aggregates every return and yield of a body, those no path
/// reaches included, and reads every reference there at its declared type:
/// `afterWriteReadsDeclared` reads `x: string | number` after `x = "s"`.
/// A break an abrupt `finally` replaces still contributes its implicit
/// `undefined` when the statement after its target reaches the end.
/// Each cell matches its own project's TypeScript 7.0.2 answer.
#[test]
fn unreachable_returns_and_yields_read_declared_types() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "unreachable.ts",
        UNREACHABLE_SOURCE,
        UNREACHABLE_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Whole-binding writes whose value nothing consumes — an unselected initializer, an expression statement, `void` of a write, a conditional arm, a sequence operand, an object member, an array element — and compound writes.
const DISCARDED_WRITE_SOURCE: &str = r#"
export function voidInit() { let x: string | number = 1; const y = void (x = "s"); return x; }
export function voidInObj() { let x: string | number = 1; const o = { a: void (x = "s") }; return x; }
export function voidArr() { let x: string | number = 1; return [void (x = "s"), x]; }
export function voidTernary(c: boolean) { let x: string | number = 1; const y = c ? void (x = "s") : 0; return x; }
export function voidValue() { let x = 1; return void (x = 2); }
export function voidLetEvolve() { let x; const y = void (x = "s"); return x; }
export function voidSelected() { let x: string | number = 1; const y = void (x = "s"); return [x, y]; }
export function voidSelectedObj() { let x: string | number = 1; const o = { a: void (x = "s") }; return [x, o]; }
export function voidAssignTernary() { let x: string | number = 1; return [void (x = true ? "s" : 2), x]; }
export function asgInit() { let x: string | number = 1; const y = (x = "s"); return x; }
export function asgInObj() { let x: string | number = 1; const o = { a: (x = "s") }; return x; }
export function asgTernary(c: boolean) { let x: string | number = 1; const y = c ? (x = "s") : 0; return x; }
export function stmtTernary(c: boolean) { let x: string | number = 1; c ? (x = "s") : 0; return x; }
export function stmtBothArms(c: boolean) { let x: string | number | boolean = 1; c ? (x = "s") : (x = true); return x; }
export function stmtSeq() { let x: string | number = 1; (x = "s", 0); return x; }
export function stmtObj() { let x: string | number = 1; ({ a: (x = "s") }); return x; }
export function stmtArr() { let x: string | number = 1; [0, void (x = "s")]; return x; }
export function initSeqOrder() { let x: string | number = 1; const y = (x = "s", x = 2); return x; }
export function initNested(c: boolean) { let x: string | number | boolean = 1; const y = { a: c ? void (x = "s") : [x = true] }; return x; }
export function unselectedWrite(x: string | number) { const unused = (x = "s", 0); return x }
export function compoundParam(x: string | number) { x += "s"; return x; }
export function compoundLiteral() { let x: 1 | 2 = 1; x++; return x; }
export function compoundString() { let s = "a"; s += 1; return s; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`DISCARDED_WRITE_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const DISCARDED_WRITE_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string / string / string / string
    ("voidInit", "string", "string", "string", "string"),
    // string / string / string / string
    ("voidInObj", "string", "string", "string", "string"),
    // (string | undefined)[] / string[] / (string | undefined)[] / string[]
    (
        "voidArr",
        "(string | undefined)[]",
        "string[]",
        "(string | undefined)[]",
        "string[]",
    ),
    // string | number / string | number / string | number / string | number
    (
        "voidTernary",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // undefined / any / undefined / any
    ("voidValue", "undefined", "any", "undefined", "any"),
    // string / string / any / any
    ("voidLetEvolve", "string", "string", "any", "any"),
    // (string | undefined)[] / any[] / (string | undefined)[] / any[]
    (
        "voidSelected",
        "(string | undefined)[]",
        "any[]",
        "(string | undefined)[]",
        "any[]",
    ),
    // (string | { a: undefined; })[] / (string | { a: any; })[] / (string | { a: undefined; })[] / (string | { a: any; })[]
    (
        "voidSelectedObj",
        "(string | { a: undefined })[]",
        "(string | { a: any })[]",
        "(string | { a: undefined })[]",
        "(string | { a: any })[]",
    ),
    // (string | number | undefined)[] / (string | number)[] / (string | number | undefined)[] / (string | number)[]
    (
        "voidAssignTernary",
        "(number | string | undefined)[]",
        "(number | string)[]",
        "(number | string | undefined)[]",
        "(number | string)[]",
    ),
    // string / string / string / string
    ("asgInit", "string", "string", "string", "string"),
    // string / string / string / string
    ("asgInObj", "string", "string", "string", "string"),
    // string | number / string | number / string | number / string | number
    (
        "asgTernary",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number / string | number / string | number / string | number
    (
        "stmtTernary",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | true / string | true / string | true / string | true
    (
        "stmtBothArms",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string / string / string / string
    ("stmtSeq", "string", "string", "string", "string"),
    // string / string / string / string
    ("stmtObj", "string", "string", "string", "string"),
    // string / string / string / string
    ("stmtArr", "string", "string", "string", "string"),
    // number / number / number / number
    ("initSeqOrder", "number", "number", "number", "number"),
    // string | true / string | true / string | true / string | true
    (
        "initNested",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string / string / string / string
    ("unselectedWrite", "string", "string", "string", "string"),
    // string | number / string | number / string | number / string | number
    (
        "compoundParam",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // number / number / number / number
    ("compoundLiteral", "number", "number", "number", "number"),
    // string / string / string / string
    ("compoundString", "string", "string", "string", "string"),
];

/// A discarded value's whole-binding writes apply in evaluation order: the
/// write under `void`, in a conditional's arm (joined under its test), a
/// sequence operand, an object literal's member or an array literal's
/// element, whether the value is an expression statement or a declarator
/// initializer the demand did not select. A compound write retypes its
/// target to the base type of what it held. Each cell matches its own
/// project's TypeScript 7.0.2 answer.
#[test]
fn discarded_values_apply_their_writes_in_evaluation_order() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "discarded.ts",
        DISCARDED_WRITE_SOURCE,
        DISCARDED_WRITE_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Writes to a declared union holding `boolean`: a boolean literal, a `boolean`, a conditional, joined across branches.
const DECLARED_BOOLEAN_SOURCE: &str = r#"
export function asgBool() { let x: string | number | boolean = 1; x = true; return x; }
export function asgBoolIf(c: boolean) { let x: string | number | boolean = 1; if (c) { x = true; } else { x = "s"; } return x; }
export function asgBoolOneArm(c: boolean) { let x: string | boolean = "a"; if (c) { x = true; } return x; }
export function asgBoolBoth(c: boolean) { let x: number | boolean = 1; if (c) { x = true; } else { x = false; } return x; }
export function asgBoolArray(c: boolean) { let x: string | number | boolean = 1; if (c) { x = true; } else { x = "s"; } return [x]; }
export function asgBoolConst(c: boolean) { let x: string | number | boolean = 1; if (c) { x = true; } else { x = "s"; } const y = x; return y; }
export function asgBoolParam(c: boolean, s: string) { let x: string | number | boolean = 1; if (c) { x = true; } else { x = s; } return x; }
export function asgBoolFromBoolean(b: boolean) { let x: string | number | boolean = 1; x = b; return x; }
export function asgBoolAlone() { let x: boolean = false; x = true; return x; }
export function asgBoolInit() { let x: boolean = true; return x; }
export function asgBoolInitUnion() { let x: string | boolean = true; return x; }
export function asgTernaryUnion(c: boolean) { let x: string | number | boolean = 1; x = c ? "s" : 2; return x; }
export function asgTernaryTrue() { let x: string | number = 1; x = true ? "s" : 2; return x; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`DECLARED_BOOLEAN_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row; `false | true` is the lane's spelling of the checker's `boolean`, the one the oracle normalizer equates).
const DECLARED_BOOLEAN_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // boolean / boolean / boolean / boolean
    ("asgBool", "boolean", "boolean", "boolean", "boolean"),
    // string | true / string | true / string | true / string | true
    (
        "asgBoolIf",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true / string | true / string | true / string | true
    (
        "asgBoolOneArm",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // boolean / boolean / boolean / boolean
    ("asgBoolBoth", "boolean", "boolean", "boolean", "boolean"),
    // (string | boolean)[] / (string | boolean)[] / (string | boolean)[] / (string | boolean)[]
    (
        "asgBoolArray",
        "(boolean | string)[]",
        "(boolean | string)[]",
        "(boolean | string)[]",
        "(boolean | string)[]",
    ),
    // string | true / string | true / string | true / string | true
    (
        "asgBoolConst",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true / string | true / string | true / string | true
    (
        "asgBoolParam",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // boolean / boolean / boolean / boolean
    (
        "asgBoolFromBoolean",
        "boolean",
        "boolean",
        "boolean",
        "boolean",
    ),
    // boolean / boolean / boolean / boolean
    ("asgBoolAlone", "boolean", "boolean", "boolean", "boolean"),
    // boolean / boolean / boolean / boolean
    ("asgBoolInit", "boolean", "boolean", "boolean", "boolean"),
    // boolean / boolean / boolean / boolean
    (
        "asgBoolInitUnion",
        "boolean",
        "boolean",
        "boolean",
        "boolean",
    ),
    // string | number / string | number / string | number / string | number
    (
        "asgTernaryUnion",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number / string | number / string | number / string | number
    (
        "asgTernaryTrue",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
];

/// A declared union's reduction reads `boolean` as `true | false` and keeps
/// every constituent SOME member of the assigned value is assignable to
/// (`getAssignmentReducedType` over `typeMaybeAssignableTo`): `x = true`
/// keeps `true`, `x = c ? "s" : 2` keeps `string` and `number`. A fresh
/// boolean literal stays fresh, so a lone `true` read at a return widens to
/// `boolean`, while a join (`string | true`) keeps it. Each cell matches
/// its own project's TypeScript 7.0.2 answer.
#[test]
fn declared_boolean_unions_keep_the_assigned_constituent() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "declared-boolean.ts",
        DECLARED_BOOLEAN_SOURCE,
        DECLARED_BOOLEAN_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Array and tuple relations asked through a conditional type: covariant mutable arrays, `readonly` arrays, and tuples against arrays and tuples of fixed, optional, rest and `readonly` shape.
const ARRAY_TUPLE_RELATION_SOURCE: &str = r#"
declare const __v_arrCov: (string[]) extends ((string | number)[]) ? "y" : "n";
export function arrCov() { return __v_arrCov; }
declare const __v_arrContra: ((string | number)[]) extends (string[]) ? "y" : "n";
export function arrContra() { return __v_arrContra; }
declare const __v_arrLit: ("a"[]) extends (string[]) ? "y" : "n";
export function arrLit() { return __v_arrLit; }
declare const __v_arrNever: (never[]) extends (string[]) ? "y" : "n";
export function arrNever() { return __v_arrNever; }
declare const __v_arrToRo: (string[]) extends (readonly (string | number)[]) ? "y" : "n";
export function arrToRo() { return __v_arrToRo; }
declare const __v_arrRoToMut: (readonly string[]) extends (string[]) ? "y" : "n";
export function arrRoToMut() { return __v_arrRoToMut; }
declare const __v_arrRoRo: (readonly string[]) extends (readonly (string | number)[]) ? "y" : "n";
export function arrRoRo() { return __v_arrRoRo; }
declare const __v_arrObjSub: ({ a: string; b: number }[]) extends ({ a: string }[]) ? "y" : "n";
export function arrObjSub() { return __v_arrObjSub; }
declare const __v_arrObjSup: ({ a: string }[]) extends ({ a: string; b: number }[]) ? "y" : "n";
export function arrObjSup() { return __v_arrObjSup; }
declare const __v_arrNull: (null[]) extends (string[]) ? "y" : "n";
export function arrNull() { return __v_arrNull; }
declare const __v_arrAny: (any[]) extends (string[]) ? "y" : "n";
export function arrAny() { return __v_arrAny; }
declare const __v_arrToAny: (string[]) extends (any[]) ? "y" : "n";
export function arrToAny() { return __v_arrToAny; }
declare const __v_arrUnknown: (unknown[]) extends (string[]) ? "y" : "n";
export function arrUnknown() { return __v_arrUnknown; }
declare const __v_arrNested: (string[][]) extends ((string | number)[][]) ? "y" : "n";
export function arrNested() { return __v_arrNested; }
declare const __v_tupLonger: ([string, string]) extends ([string]) ? "y" : "n";
export function tupLonger() { return __v_tupLonger; }
declare const __v_tupShorter: ([string]) extends ([string, string]) ? "y" : "n";
export function tupShorter() { return __v_tupShorter; }
declare const __v_tupToOpt: ([string]) extends ([string, string?]) ? "y" : "n";
export function tupToOpt() { return __v_tupToOpt; }
declare const __v_tupOptToReq: ([string, string?]) extends ([string]) ? "y" : "n";
export function tupOptToReq() { return __v_tupOptToReq; }
declare const __v_tupOptOpt: ([string, string?]) extends ([string, string?]) ? "y" : "n";
export function tupOptOpt() { return __v_tupOptOpt; }
declare const __v_tupOptToReq2: ([string, string?]) extends ([string, string]) ? "y" : "n";
export function tupOptToReq2() { return __v_tupOptToReq2; }
declare const __v_tupFixedToRest: ([string, number]) extends ([string, ...number[]]) ? "y" : "n";
export function tupFixedToRest() { return __v_tupFixedToRest; }
declare const __v_tupFixedToRest2: ([string, number, number]) extends ([string, ...number[]]) ? "y" : "n";
export function tupFixedToRest2() { return __v_tupFixedToRest2; }
declare const __v_tupFixedToRestBad: ([string, string]) extends ([string, ...number[]]) ? "y" : "n";
export function tupFixedToRestBad() { return __v_tupFixedToRestBad; }
declare const __v_tupRestToFixed: ([string, ...number[]]) extends ([string, number]) ? "y" : "n";
export function tupRestToFixed() { return __v_tupRestToFixed; }
declare const __v_tupRestRest: ([string, ...number[]]) extends ([string, ...number[]]) ? "y" : "n";
export function tupRestRest() { return __v_tupRestRest; }
declare const __v_tupRestToArr: ([string, ...number[]]) extends ((string | number)[]) ? "y" : "n";
export function tupRestToArr() { return __v_tupRestToArr; }
declare const __v_tupRestToArrBad: ([string, ...number[]]) extends (string[]) ? "y" : "n";
export function tupRestToArrBad() { return __v_tupRestToArrBad; }
declare const __v_arrToTup1: (string[]) extends ([string]) ? "y" : "n";
export function arrToTup1() { return __v_arrToTup1; }
declare const __v_arrToTupRest: (string[]) extends ([...string[]]) ? "y" : "n";
export function arrToTupRest() { return __v_arrToTupRest; }
declare const __v_arrToTupReqRest: (string[]) extends ([string, ...string[]]) ? "y" : "n";
export function arrToTupReqRest() { return __v_arrToTupReqRest; }
declare const __v_arrToTupRestBad: (number[]) extends ([...string[]]) ? "y" : "n";
export function arrToTupRestBad() { return __v_arrToTupRestBad; }
declare const __v_tupEmpty: ([]) extends ([]) ? "y" : "n";
export function tupEmpty() { return __v_tupEmpty; }
declare const __v_tupEmptyToOpt: ([]) extends ([string?]) ? "y" : "n";
export function tupEmptyToOpt() { return __v_tupEmptyToOpt; }
declare const __v_tupOptToEmpty: ([string?]) extends ([]) ? "y" : "n";
export function tupOptToEmpty() { return __v_tupOptToEmpty; }
declare const __v_tupRoToMut: (readonly [string]) extends ([string]) ? "y" : "n";
export function tupRoToMut() { return __v_tupRoToMut; }
declare const __v_tupToRo: ([string]) extends (readonly [string]) ? "y" : "n";
export function tupToRo() { return __v_tupToRo; }
declare const __v_tupRoToRoArr: (readonly [string]) extends (readonly string[]) ? "y" : "n";
export function tupRoToRoArr() { return __v_tupRoToRoArr; }
declare const __v_tupRoToArr: (readonly [string]) extends (string[]) ? "y" : "n";
export function tupRoToArr() { return __v_tupRoToArr; }
declare const __v_tupToArr: ([string, number]) extends ((string | number)[]) ? "y" : "n";
export function tupToArr() { return __v_tupToArr; }
declare const __v_tupOptToArr: ([string, number?]) extends ((string | number)[]) ? "y" : "n";
export function tupOptToArr() { return __v_tupOptToArr; }
declare const __v_tupOptToArrU: ([string, number?]) extends ((string | number | undefined)[]) ? "y" : "n";
export function tupOptToArrU() { return __v_tupOptToArrU; }
declare const __v_tupToArrBad: ([string, number]) extends (string[]) ? "y" : "n";
export function tupToArrBad() { return __v_tupToArrBad; }
declare const __v_tupElemCov: (["a", 1]) extends ([string, number]) ? "y" : "n";
export function tupElemCov() { return __v_tupElemCov; }
declare const __v_tupElemBad: ([string, number]) extends ([string, string]) ? "y" : "n";
export function tupElemBad() { return __v_tupElemBad; }
declare const __v_tupEmptyToArr: ([]) extends (string[]) ? "y" : "n";
export function tupEmptyToArr() { return __v_tupEmptyToArr; }
declare const __v_tupLeadingRest: ([...string[], number]) extends ([...string[], number]) ? "y" : "n";
export function tupLeadingRest() { return __v_tupLeadingRest; }
declare const __v_tupLeadingRestBad: ([...string[], number]) extends ([string, number]) ? "y" : "n";
export function tupLeadingRestBad() { return __v_tupLeadingRestBad; }
declare const __v_tupMidRest: ([string, ...number[], boolean]) extends ([string, ...(number | boolean)[]]) ? "y" : "n";
export function tupMidRest() { return __v_tupMidRest; }
declare const __v_tupOptRest: ([string?, ...number[]]) extends ([string?, ...number[]]) ? "y" : "n";
export function tupOptRest() { return __v_tupOptRest; }
declare const __v_tupToOptRest: ([string, number]) extends ([string?, ...number[]]) ? "y" : "n";
export function tupToOptRest() { return __v_tupToOptRest; }
type FS = (x: string) => void;
type FSN = (x: string | number) => void;
declare const __v_arrFnContra: (FS[]) extends (FSN[]) ? "y" : "n";
export function arrFnContra() { return __v_arrFnContra; }
declare const __v_arrFnCov: (FSN[]) extends (FS[]) ? "y" : "n";
export function arrFnCov() { return __v_arrFnCov; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`ARRAY_TUPLE_RELATION_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const ARRAY_TUPLE_RELATION_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "y" / "y" / "y" / "y"
    ("arrCov", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("arrContra", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("arrLit", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("arrNever", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("arrToRo", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("arrRoToMut", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("arrRoRo", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("arrObjSub", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("arrObjSup", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "n" / "y" / "n" / "y"
    ("arrNull", "\"n\"", "\"y\"", "\"n\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("arrAny", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("arrToAny", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("arrUnknown", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("arrNested", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("tupLonger", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "n" / "n" / "n" / "n"
    ("tupShorter", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("tupToOpt", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("tupOptToReq", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("tupOptOpt", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("tupOptToReq2", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("tupFixedToRest", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("tupFixedToRest2", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("tupFixedToRestBad", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "n" / "n" / "n" / "n"
    ("tupRestToFixed", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("tupRestRest", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("tupRestToArr", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("tupRestToArrBad", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "n" / "n" / "n" / "n"
    ("arrToTup1", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("arrToTupRest", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("arrToTupReqRest", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "n" / "n" / "n" / "n"
    ("arrToTupRestBad", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("tupEmpty", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("tupEmptyToOpt", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("tupOptToEmpty", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "n" / "n" / "n" / "n"
    ("tupRoToMut", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("tupToRo", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("tupRoToRoArr", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("tupRoToArr", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("tupToArr", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "y" / "n" / "y"
    ("tupOptToArr", "\"n\"", "\"y\"", "\"n\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("tupOptToArrU", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("tupToArrBad", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("tupElemCov", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("tupElemBad", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("tupEmptyToArr", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("tupLeadingRest", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("tupLeadingRestBad", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("tupMidRest", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("tupOptRest", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("tupToOptRest", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("arrFnContra", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("arrFnCov", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
];

/// Array relations over aliased and interface element types.
const ALIASED_ARRAY_RELATION_SOURCE: &str = r#"
type FS = (x: string) => void;
type FSN = (x: string | number) => void;
interface IA { a: string }
declare const __v_fnAlias: (FS) extends (FSN) ? "y" : "n";
export function fnAlias() { return __v_fnAlias; }
declare const __v_fnAliasRev: (FSN) extends (FS) ? "y" : "n";
export function fnAliasRev() { return __v_fnAliasRev; }
declare const __v_fnArrAlias: (FS[]) extends (FSN[]) ? "y" : "n";
export function fnArrAlias() { return __v_fnArrAlias; }
declare const __v_ifaceArr: (IA[]) extends ({ a: string }[]) ? "y" : "n";
export function ifaceArr() { return __v_ifaceArr; }
declare const __v_ifaceArr2: ({ a: string; b: number }[]) extends (IA[]) ? "y" : "n";
export function ifaceArr2() { return __v_ifaceArr2; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`ALIASED_ARRAY_RELATION_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const ALIASED_ARRAY_RELATION_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "n" / "n" / "n" / "n"
    ("fnAlias", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("fnAliasRev", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("fnArrAlias", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("ifaceArr", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("ifaceArr2", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
];

/// Array and tuple relations over interface element types.
const INTERFACE_ELEMENT_RELATION_SOURCE: &str = r#"
interface IA { a: string }
declare const __v_ifaceBare: (IA) extends ({ a: string }) ? "y" : "n";
export function ifaceBare() { return __v_ifaceBare; }
declare const __v_ifaceTup: ([IA]) extends ([{ a: string }]) ? "y" : "n";
export function ifaceTup() { return __v_ifaceTup; }
declare const __v_ifaceArr: (IA[]) extends ({ a: string }[]) ? "y" : "n";
export function ifaceArr() { return __v_ifaceArr; }
declare const __v_ifaceRo: (readonly IA[]) extends (readonly { a: string }[]) ? "y" : "n";
export function ifaceRo() { return __v_ifaceRo; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`INTERFACE_ELEMENT_RELATION_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const INTERFACE_ELEMENT_RELATION_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "y" / "y" / "y" / "y"
    ("ifaceBare", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("ifaceTup", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("ifaceArr", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("ifaceRo", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
];

/// A mutable array relates COVARIANTLY in its element, as the checker's
/// `Array<T>` does (`string[]` is assignable to `(string | number)[]`), and
/// a tuple relates by the checker's arity rules: a fixed tuple to a fixed
/// one of the same length, optional and rest elements by position, a tuple
/// to an array through every element, an array to a tuple never (unless the
/// tuple is all rest), and a `readonly` source never to a mutable target.
/// Each cell matches its own project's TypeScript 7.0.2 answer.
#[test]
fn array_and_tuple_relations_follow_the_checker() {
    let host = four_policy_host();
    let mut mismatches = Vec::new();
    mismatches.extend(matrix_mismatches(
        &host,
        "array-tuple-relations.ts",
        ARRAY_TUPLE_RELATION_SOURCE,
        ARRAY_TUPLE_RELATION_TABLE,
    ));
    mismatches.extend(matrix_mismatches(
        &host,
        "aliased-array-relations.ts",
        ALIASED_ARRAY_RELATION_SOURCE,
        ALIASED_ARRAY_RELATION_TABLE,
    ));
    mismatches.extend(matrix_mismatches(
        &host,
        "interface-element-relations.ts",
        INTERFACE_ELEMENT_RELATION_SOURCE,
        INTERFACE_ELEMENT_RELATION_TABLE,
    ));
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Return joins whose arms the checker's strict subtype relation orders: `any` and `unknown` members, optional and `readonly` members, signatures of different arity, derived interfaces and classes, arrays, tuples, empty objects and index signatures.
const STRICT_SUBTYPE_JOIN_SOURCE: &str = r#"
interface Generator<T, TReturn, TNext> {}
interface A1 { x: string }
interface B1 { x: string }
interface D1 extends A1 { y: number }
class CA { x = 1 }
class CB { x = 1 }
class CD extends CA { y = 2 }
export function retAnyObj(c: boolean, a: { x: any }, b: { x: string }) { if (c) return a; return b; }
export function retObjAny(c: boolean, a: { x: string }, b: { x: any }) { if (c) return a; return b; }
export function retUnknownObj(c: boolean, a: { x: unknown }, b: { x: string }) { if (c) return a; return b; }
export function retOptDecl(c: boolean, a: { x: string }, b: { x: string; y?: number }) { if (c) return a; return b; }
export function retOptDeclRev(c: boolean, a: { x: string; y?: number }, b: { x: string }) { if (c) return a; return b; }
export function retOptLit(c: boolean, b: { x: string; y?: number }) { if (c) return { x: "s" }; return b; }
export function retLitOpt(c: boolean, a: { x: string; y?: number }) { if (c) return a; return { x: "s" }; }
export function retRoProp(c: boolean, a: { readonly x: string }, b: { x: string }) { if (c) return a; return b; }
export function retRoPropRev(c: boolean, a: { x: string }, b: { readonly x: string }) { if (c) return a; return b; }
export function retFnOpt(c: boolean, a: (s?: string) => number, b: () => number) { if (c) return a; return b; }
export function retFnOptRev(c: boolean, a: () => number, b: (s?: string) => number) { if (c) return a; return b; }
export function retFnRest(c: boolean, a: (...s: string[]) => number, b: () => number) { if (c) return a; return b; }
export function retSameIface(c: boolean, a: A1, b: B1) { if (c) return a; return b; }
export function retSameIfaceRev(c: boolean, a: B1, b: A1) { if (c) return a; return b; }
export function retDerivedIface(c: boolean, a: A1, b: D1) { if (c) return a; return b; }
export function retDerivedIfaceRev(c: boolean, a: D1, b: A1) { if (c) return a; return b; }
export function retClassSame(c: boolean, a: CA, b: CB) { if (c) return a; return b; }
export function retClassDerived(c: boolean, a: CA, b: CD) { if (c) return a; return b; }
export function retClassDerivedRev(c: boolean, a: CD, b: CA) { if (c) return a; return b; }
export function retArrSubtype(c: boolean, a: string[], b: (string | number)[]) { if (c) return a; return b; }
export function retArrSupertypeFirst(c: boolean, a: (string | number)[], b: string[]) { if (c) return a; return b; }
export function retRoArr(c: boolean, a: readonly string[], b: string[]) { if (c) return a; return b; }
export function retTupArr(c: boolean, a: [string], b: string[]) { if (c) return a; return b; }
export function retTupLonger(c: boolean, a: [string, string], b: [string]) { if (c) return a; return b; }
export function retNullArr(c: boolean) { if (c) return [null]; return ["s"]; }
export function retEmptyArr(c: boolean) { if (c) return []; return ["s"]; }
export function retNullArrDecl(c: boolean, b: string[]) { if (c) return [null]; return b; }
export function arrNullArr() { return [[null], ["s"]]; }
export function retEmptyObj(c: boolean, a: {}, b: { x: string }) { if (c) return a; return b; }
export function retEmptyObjPrim(c: boolean, a: {}, b: string) { if (c) return a; return b; }
export function retObjectPrim(c: boolean, a: object, b: { x: string }) { if (c) return a; return b; }
export function retUnknownAnyArr(c: boolean, a: any[], b: unknown[]) { if (c) return a; return b; }
export function retIndexSig(c: boolean, a: { [k: string]: string }, b: { x: string }) { if (c) return a; return b; }
export function retIndexSigAny(c: boolean, a: { [k: string]: any }, b: { x: string }) { if (c) return a; return b; }
export function* genAnyObj(a: { x: any }, b: { x: string }) { yield a; yield b; }
export function arrAnyObj(a: { x: any }, b: { x: string }) { return [a, b]; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`STRICT_SUBTYPE_JOIN_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const STRICT_SUBTYPE_JOIN_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // { x: any; } / { x: any; } / { x: any; } / { x: any; }
    (
        "retAnyObj",
        "{ x: any }",
        "{ x: any }",
        "{ x: any }",
        "{ x: any }",
    ),
    // { x: any; } / { x: any; } / { x: any; } / { x: any; }
    (
        "retObjAny",
        "{ x: any }",
        "{ x: any }",
        "{ x: any }",
        "{ x: any }",
    ),
    // { x: unknown; } / { x: unknown; } / { x: unknown; } / { x: unknown; }
    (
        "retUnknownObj",
        "{ x: unknown }",
        "{ x: unknown }",
        "{ x: unknown }",
        "{ x: unknown }",
    ),
    // { x: string; } / { x: string; } / { x: string; } / { x: string; }
    (
        "retOptDecl",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
    ),
    // { x: string; } / { x: string; } / { x: string; } / { x: string; }
    (
        "retOptDeclRev",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
    ),
    // { x: string; y?: number | undefined; } / { x: string; y?: number; } / { x: string; y?: number | undefined; } / { x: string; y?: number; }
    (
        "retOptLit",
        "{ x: string; y?: number }",
        "{ x: string; y?: number }",
        "{ x: string; y?: number }",
        "{ x: string; y?: number }",
    ),
    // { x: string; y?: number | undefined; } / { x: string; y?: number; } / { x: string; y?: number | undefined; } / { x: string; y?: number; }
    (
        "retLitOpt",
        "{ x: string; y?: number }",
        "{ x: string; y?: number }",
        "{ x: string; y?: number }",
        "{ x: string; y?: number }",
    ),
    // { readonly x: string; } / { readonly x: string; } / { readonly x: string; } / { readonly x: string; }
    (
        "retRoProp",
        "{ readonly x: string }",
        "{ readonly x: string }",
        "{ readonly x: string }",
        "{ readonly x: string }",
    ),
    // { readonly x: string; } / { readonly x: string; } / { readonly x: string; } / { readonly x: string; }
    (
        "retRoPropRev",
        "{ readonly x: string }",
        "{ readonly x: string }",
        "{ readonly x: string }",
        "{ readonly x: string }",
    ),
    // (s?: string | undefined) => number / (s?: string) => number / (s?: string | undefined) => number / (s?: string) => number
    (
        "retFnOpt",
        "(s?: string) => number",
        "(s?: string) => number",
        "(s?: string) => number",
        "(s?: string) => number",
    ),
    // (s?: string | undefined) => number / (s?: string) => number / (s?: string | undefined) => number / (s?: string) => number
    (
        "retFnOptRev",
        "(s?: string) => number",
        "(s?: string) => number",
        "(s?: string) => number",
        "(s?: string) => number",
    ),
    // (...s: string[]) => number / (...s: string[]) => number / (...s: string[]) => number / (...s: string[]) => number
    (
        "retFnRest",
        "(...s: string[]) => number",
        "(...s: string[]) => number",
        "(...s: string[]) => number",
        "(...s: string[]) => number",
    ),
    // A1 / A1 / A1 / A1
    ("retDerivedIface", "A1", "A1", "A1", "A1"),
    // A1 / A1 / A1 / A1
    ("retDerivedIfaceRev", "A1", "A1", "A1", "A1"),
    // CA | CB / CA | CB / CA | CB / CA | CB
    ("retClassSame", "CA | CB", "CA | CB", "CA | CB", "CA | CB"),
    // CA / CA / CA / CA
    ("retClassDerived", "CA", "CA", "CA", "CA"),
    // CA / CA / CA / CA
    ("retClassDerivedRev", "CA", "CA", "CA", "CA"),
    // (string | number)[] / (string | number)[] / (string | number)[] / (string | number)[]
    (
        "retArrSubtype",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // (string | number)[] / (string | number)[] / (string | number)[] / (string | number)[]
    (
        "retArrSupertypeFirst",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // readonly string[] / readonly string[] / readonly string[] / readonly string[]
    (
        "retRoArr",
        "readonly string[]",
        "readonly string[]",
        "readonly string[]",
        "readonly string[]",
    ),
    // string[] / string[] / string[] / string[]
    ("retTupArr", "string[]", "string[]", "string[]", "string[]"),
    // [string] | [string, string] / [string] | [string, string] / [string] | [string, string] / [string] | [string, string]
    (
        "retTupLonger",
        "[string, string] | [string]",
        "[string, string] | [string]",
        "[string, string] | [string]",
        "[string, string] | [string]",
    ),
    // null[] | string[] / string[] / null[] | string[] / string[]
    (
        "retNullArr",
        "null[] | string[]",
        "string[]",
        "null[] | string[]",
        "string[]",
    ),
    // string[] / string[] / string[] / string[]
    (
        "retEmptyArr",
        "string[]",
        "string[]",
        "string[]",
        "string[]",
    ),
    // null[] | string[] / string[] / null[] | string[] / string[]
    (
        "retNullArrDecl",
        "null[] | string[]",
        "string[]",
        "null[] | string[]",
        "string[]",
    ),
    // (null[] | string[])[] / string[][] / (null[] | string[])[] / string[][]
    (
        "arrNullArr",
        "(null[] | string[])[]",
        "string[][]",
        "(null[] | string[])[]",
        "string[][]",
    ),
    // {} / {} / {} / {}
    ("retEmptyObj", "{  }", "{  }", "{  }", "{  }"),
    // {} / {} / {} / {}
    ("retEmptyObjPrim", "{  }", "{  }", "{  }", "{  }"),
    // object / object / object / object
    ("retObjectPrim", "object", "object", "object", "object"),
    // any[] / any[] / any[] / any[]
    ("retUnknownAnyArr", "any[]", "any[]", "any[]", "any[]"),
    // { [k: string]: string; } | { x: string; } / { [k: string]: string; } | { x: string; } / { [k: string]: string; } | { x: string; } / { [k: string]: string; } | { x: string; }
    (
        "retIndexSig",
        "{ [k: string]: string } | { x: string }",
        "{ [k: string]: string } | { x: string }",
        "{ [k: string]: string } | { x: string }",
        "{ [k: string]: string } | { x: string }",
    ),
    // { [k: string]: any; } | { x: string; } / { [k: string]: any; } | { x: string; } / { [k: string]: any; } | { x: string; } / { [k: string]: any; } | { x: string; }
    (
        "retIndexSigAny",
        "{ [k: string]: any } | { x: string }",
        "{ [k: string]: any } | { x: string }",
        "{ [k: string]: any } | { x: string }",
        "{ [k: string]: any } | { x: string }",
    ),
    // Generator<{ x: any; }, void, unknown> / Generator<{ x: any; }, void, unknown> / Generator<{ x: any; }, void, unknown> / Generator<{ x: any; }, void, unknown>
    (
        "genAnyObj",
        "Generator<{ x: any }, void, unknown>",
        "Generator<{ x: any }, void, unknown>",
        "Generator<{ x: any }, void, unknown>",
        "Generator<{ x: any }, void, unknown>",
    ),
    // { x: any; }[] / { x: any; }[] / { x: any; }[] / { x: any; }[]
    (
        "arrAnyObj",
        "{ x: any }[]",
        "{ x: any }[]",
        "{ x: any }[]",
        "{ x: any }[]",
    ),
];

/// Return and array joins over interface-typed members, elements and tuple positions.
const DECLARED_MEMBER_JOIN_SOURCE: &str = r#"
interface A1 { x: string }
interface D1 extends A1 { y: number }
interface P1 { v: A1 }
interface P2 { v: D1 }
export function retIfaceArr(c: boolean, a: A1[], b: D1[]) { if (c) return a; return b; }
export function retIfaceMember(c: boolean, a: P1, b: P2) { if (c) return a; return b; }
export function retIfaceTuple(c: boolean, a: [A1], b: [D1]) { if (c) return a; return b; }
export function retObjWithIface(c: boolean, a: { v: A1 }, b: { v: D1 }) { if (c) return a; return b; }
export function arrIfaceElems(a: A1, b: D1) { return [a, b]; }
export function retIface(c: boolean, a: A1, b: D1) { if (c) return a; return b; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`DECLARED_MEMBER_JOIN_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const DECLARED_MEMBER_JOIN_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // A1[] / A1[] / A1[] / A1[]
    ("retIfaceArr", "A1[]", "A1[]", "A1[]", "A1[]"),
    // P1 / P1 / P1 / P1
    ("retIfaceMember", "P1", "P1", "P1", "P1"),
    // [A1] / [A1] / [A1] / [A1]
    ("retIfaceTuple", "[A1]", "[A1]", "[A1]", "[A1]"),
    // { v: A1; } / { v: A1; } / { v: A1; } / { v: A1; }
    (
        "retObjWithIface",
        "{ v: A1 }",
        "{ v: A1 }",
        "{ v: A1 }",
        "{ v: A1 }",
    ),
    // A1[] / A1[] / A1[] / A1[]
    ("arrIfaceElems", "A1[]", "A1[]", "A1[]", "A1[]"),
    // A1 / A1 / A1 / A1
    ("retIface", "A1", "A1", "A1", "A1"),
];

/// A join's arms reduce by the checker's STRICT subtype relation
/// (`isTypeStrictSubtypeOf` in `removeSubtypes`): `any` is never below
/// `unknown`, a `readonly` member never below a mutable one, a signature
/// never below one taking fewer parameters, an optional member never below
/// a required one, an index signature is inferred only for an object
/// literal, and structurally identical classes reduce only through
/// derivation. `if (c) return [null]; return ["s"]` is `string[]` with
/// `strictNullChecks` off, the covariant array absorbing `null[]`. Each
/// cell matches its own project's TypeScript 7.0.2 answer.
#[test]
fn joins_reduce_by_the_checkers_strict_subtype_relation() {
    let host = four_policy_host();
    let mut mismatches = Vec::new();
    mismatches.extend(matrix_mismatches(
        &host,
        "strict-subtype-joins.ts",
        STRICT_SUBTYPE_JOIN_SOURCE,
        STRICT_SUBTYPE_JOIN_TABLE,
    ));
    mismatches.extend(matrix_mismatches(
        &host,
        "declared-member-joins.ts",
        DECLARED_MEMBER_JOIN_SOURCE,
        DECLARED_MEMBER_JOIN_TABLE,
    ));
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Conditional expressions returned, bound, nested and read as members or elements.
const CONDITIONAL_EXPRESSION_SOURCE: &str = r#"
interface A1 { x: string }
interface B1 { x: string }
export function ternDeclSubtype(c: boolean, a: { x: string }, b: { x: string; y: number }) { return c ? a : b; }
export function ternDeclSubtypeLocal(c: boolean, a: { x: string }, b: { x: string; y: number }) { const r = c ? a : b; return r; }
export function ternNullObj(c: boolean) { return c ? { a: null } : { a: "s" }; }
export function ternNullArr(c: boolean) { return c ? [null] : ["s"]; }
export function ternAnyObj(c: boolean, a: { x: any }, b: { x: string }) { return c ? a : b; }
export function ternArr(c: boolean, a: string[], b: (string | number)[]) { return c ? a : b; }
export function ternLitSubtype(c: boolean) { return c ? "a" : "b" as string; }
export function ternLitKeep(c: boolean) { return c ? "a" : "b"; }
export function ternLitNum(c: boolean) { return c ? 1 : 2; }
export function ternMixed(c: boolean, s: string) { return c ? "a" : s; }
export function ternObjLits(c: boolean) { return c ? { a: 1, b: 2 } : { a: 1 }; }
export function ternSameIface(c: boolean, a: A1, b: B1) { return c ? a : b; }
export function ternNested(c: boolean, d: boolean, a: { x: string }, b: { x: string; y: number }, e: { x: string; z: 1 }) { return c ? a : d ? b : e; }
export function ternInArr(c: boolean, a: { x: string }, b: { x: string; y: number }) { return [c ? a : b]; }
export function ternInObj(c: boolean, a: { x: string }, b: { x: string; y: number }) { return { v: c ? a : b }; }
export function ternLetLocal(c: boolean, a: { x: string }, b: { x: string; y: number }) { let r = c ? a : b; return r; }
export function ternArg(c: boolean, a: { x: string }, b: { x: string; y: number }) { return id(c ? a : b); }
export function* genTern(c: boolean, a: { x: string }, b: { x: string; y: number }) { yield c ? a : b; }
export function ternFnOpt(c: boolean, a: (s?: string) => number, b: () => number) { return c ? a : b; }
export function ternNullLit(c: boolean) { return c ? null : "s"; }
export function ternUndefObj(c: boolean, a: { x: string }) { return c ? undefined : a; }
declare function id<T>(v: T): T;
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`CONDITIONAL_EXPRESSION_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const CONDITIONAL_EXPRESSION_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // { x: string; } / { x: string; } / { x: string; } / { x: string; }
    (
        "ternDeclSubtype",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
    ),
    // { x: string; } / { x: string; } / { x: string; } / { x: string; }
    (
        "ternDeclSubtypeLocal",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
    ),
    // { a: null; } | { a: string; } / { a: string; } / { a: null; } | { a: string; } / { a: string; }
    (
        "ternNullObj",
        "{ a: null } | { a: string }",
        "{ a: string }",
        "{ a: null } | { a: string }",
        "{ a: string }",
    ),
    // null[] | string[] / string[] / null[] | string[] / string[]
    (
        "ternNullArr",
        "null[] | string[]",
        "string[]",
        "null[] | string[]",
        "string[]",
    ),
    // { x: any; } / { x: any; } / { x: any; } / { x: any; }
    (
        "ternAnyObj",
        "{ x: any }",
        "{ x: any }",
        "{ x: any }",
        "{ x: any }",
    ),
    // (string | number)[] / (string | number)[] / (string | number)[] / (string | number)[]
    (
        "ternArr",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // string / string / string / string
    ("ternLitSubtype", "string", "string", "string", "string"),
    // "a" | "b" / "a" | "b" / "a" | "b" / "a" | "b"
    (
        "ternLitKeep",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // 1 | 2 / 1 | 2 / 1 | 2 / 1 | 2
    ("ternLitNum", "1 | 2", "1 | 2", "1 | 2", "1 | 2"),
    // string / string / string / string
    ("ternMixed", "string", "string", "string", "string"),
    // { x: string; } / { x: string; } / { x: string; } / { x: string; }
    (
        "ternNested",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
    ),
    // { x: string; }[] / { x: string; }[] / { x: string; }[] / { x: string; }[]
    (
        "ternInArr",
        "{ x: string }[]",
        "{ x: string }[]",
        "{ x: string }[]",
        "{ x: string }[]",
    ),
    // { v: { x: string; }; } / { v: { x: string; }; } / { v: { x: string; }; } / { v: { x: string; }; }
    (
        "ternInObj",
        "{ v: { x: string } }",
        "{ v: { x: string } }",
        "{ v: { x: string } }",
        "{ v: { x: string } }",
    ),
    // { x: string; } / { x: string; } / { x: string; } / { x: string; }
    (
        "ternLetLocal",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
    ),
    // (s?: string | undefined) => number / (s?: string) => number / (s?: string | undefined) => number / (s?: string) => number
    (
        "ternFnOpt",
        "(s?: string) => number",
        "(s?: string) => number",
        "(s?: string) => number",
        "(s?: string) => number",
    ),
    // "s" | null / string / "s" | null / string
    (
        "ternNullLit",
        "\"s\" | null",
        "string",
        "\"s\" | null",
        "string",
    ),
    // { x: string; } | undefined / { x: string; } / { x: string; } | undefined / { x: string; }
    (
        "ternUndefObj",
        "undefined | { x: string }",
        "{ x: string }",
        "undefined | { x: string }",
        "{ x: string }",
    ),
];

/// A conditional expression's arms reduce like a return join's
/// (`checkConditionalExpression` unions its arms with subtype reduction):
/// `c ? a : b` over `a: { x: string }` and `b: { x: string; y: number }`
/// is `{ x: string }` wherever the value lands. Each cell matches its own
/// project's TypeScript 7.0.2 answer.
#[test]
fn conditional_expressions_reduce_like_return_joins() {
    let host = four_policy_host();
    let mut mismatches = Vec::new();
    mismatches.extend(matrix_mismatches(
        &host,
        "conditional-expressions.ts",
        CONDITIONAL_EXPRESSION_SOURCE,
        CONDITIONAL_EXPRESSION_TABLE,
    ));
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Object, array and tuple literals under `satisfies` targets of every shape.
const SATISFIES_SOURCE: &str = r#"
interface IK { k: "a" | "b" }
export function satWiden() { return { label: "x", n: 1 } satisfies { label: string; n: number }; }
export function satRecord() { return { label: "x", n: 1 } satisfies Record<string, string | number>; }
export function satLitUnion() { return { label: "x", n: 1 } satisfies { label: "x" | "y"; n: 1 | 2 }; }
export function satObject() { return { n: 1 } satisfies object; }
export function satUnknown() { return { n: 1 } satisfies unknown; }
export function satNestedKeep() { return { a: { b: "x" } } satisfies { a: { b: "x" | "y" } }; }
export function satNestedWiden() { return { a: { b: "x" } } satisfies { a: { b: string } }; }
export function satArrWiden() { return ["a"] satisfies string[]; }
export function satArrKeep() { return ["a"] satisfies ("a" | "b")[]; }
export function satTuple() { return ["a", 1] satisfies [string, number]; }
export function satTupleLit() { return ["a", 1] satisfies ["a", 1]; }
export function satTupleRo() { return ["a", 1] satisfies readonly [string, number]; }
export function satArrUnknown() { return [1] satisfies unknown; }
export function satStr() { return "a" satisfies string; }
export function satStrLit() { return "a" satisfies "a" | "b"; }
export function satUnionTarget() { return { a: 1 } satisfies { a: number } | { b: string }; }
export function satKind() { return { kind: "a" } satisfies { kind: "a" | "b" }; }
export function satIndexLit() { return { n: 1 } satisfies { [k: string]: 1 | 2 }; }
export function satArrObj() { return [{ k: "a" }] satisfies { k: "a" | "b" }[]; }
export function satObjArr() { return { xs: ["a"] } satisfies { xs: string[] }; }
export function satObjTuple() { return { xs: ["a", 1] } satisfies { xs: [string, number] }; }
export function satIface() { return { k: "a" } satisfies IK; }
export function satOptional() { return { f: 1 } satisfies { f?: 1 }; }
export function satTemplate() { return { a: "x" } satisfies { a: `x${string}` }; }
export function satLocal() { const o = { k: "a" } satisfies IK; return o; }
export function satLetLocal() { let o = { k: "a" } satisfies IK; return o; }
export function satBool() { return { b: true } satisfies { b: boolean }; }
export function satBoolLit() { return { b: true } satisfies { b: true }; }
export function satTupleSpread(t: [number]) { return ["a", ...t] satisfies [string, number]; }
export function satTupleOpt() { return ["a"] satisfies [string, number?]; }
export function satEmptyTuple() { return [] satisfies []; }
export function satAny() { return { n: 1 } satisfies any; }
export function satNumIndex() { return [1, 2] satisfies { [i: number]: 1 | 2 }; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`SATISFIES_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const SATISFIES_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // { label: string; n: number; } / { label: string; n: number; } / { label: string; n: number; } / { label: string; n: number; }
    (
        "satWiden",
        "{ label: string; n: number }",
        "{ label: string; n: number }",
        "{ label: string; n: number }",
        "{ label: string; n: number }",
    ),
    // { label: string; n: number; } / { label: string; n: number; } / { label: string; n: number; } / { label: string; n: number; }
    (
        "satRecord",
        "{ label: string; n: number }",
        "{ label: string; n: number }",
        "{ label: string; n: number }",
        "{ label: string; n: number }",
    ),
    // { label: "x"; n: 1; } / { label: "x"; n: 1; } / { label: "x"; n: 1; } / { label: "x"; n: 1; }
    (
        "satLitUnion",
        "{ label: \"x\"; n: 1 }",
        "{ label: \"x\"; n: 1 }",
        "{ label: \"x\"; n: 1 }",
        "{ label: \"x\"; n: 1 }",
    ),
    // { n: number; } / { n: number; } / { n: number; } / { n: number; }
    (
        "satObject",
        "{ n: number }",
        "{ n: number }",
        "{ n: number }",
        "{ n: number }",
    ),
    // { n: number; } / { n: number; } / { n: number; } / { n: number; }
    (
        "satUnknown",
        "{ n: number }",
        "{ n: number }",
        "{ n: number }",
        "{ n: number }",
    ),
    // { a: { b: "x"; }; } / { a: { b: "x"; }; } / { a: { b: "x"; }; } / { a: { b: "x"; }; }
    (
        "satNestedKeep",
        "{ a: { b: \"x\" } }",
        "{ a: { b: \"x\" } }",
        "{ a: { b: \"x\" } }",
        "{ a: { b: \"x\" } }",
    ),
    // { a: { b: string; }; } / { a: { b: string; }; } / { a: { b: string; }; } / { a: { b: string; }; }
    (
        "satNestedWiden",
        "{ a: { b: string } }",
        "{ a: { b: string } }",
        "{ a: { b: string } }",
        "{ a: { b: string } }",
    ),
    // string[] / string[] / string[] / string[]
    (
        "satArrWiden",
        "string[]",
        "string[]",
        "string[]",
        "string[]",
    ),
    // "a"[] / "a"[] / "a"[] / "a"[]
    ("satArrKeep", "\"a\"[]", "\"a\"[]", "\"a\"[]", "\"a\"[]"),
    // [string, number] / [string, number] / [string, number] / [string, number]
    (
        "satTuple",
        "[string, number]",
        "[string, number]",
        "[string, number]",
        "[string, number]",
    ),
    // ["a", 1] / ["a", 1] / ["a", 1] / ["a", 1]
    (
        "satTupleLit",
        "[\"a\", 1]",
        "[\"a\", 1]",
        "[\"a\", 1]",
        "[\"a\", 1]",
    ),
    // [string, number] / [string, number] / [string, number] / [string, number]
    (
        "satTupleRo",
        "[string, number]",
        "[string, number]",
        "[string, number]",
        "[string, number]",
    ),
    // number[] / number[] / number[] / number[]
    (
        "satArrUnknown",
        "number[]",
        "number[]",
        "number[]",
        "number[]",
    ),
    // string / string / string / string
    ("satStr", "string", "string", "string", "string"),
    // string / string / string / string
    ("satStrLit", "string", "string", "string", "string"),
    // { a: number; } / { a: number; } / { a: number; } / { a: number; }
    (
        "satUnionTarget",
        "{ a: number }",
        "{ a: number }",
        "{ a: number }",
        "{ a: number }",
    ),
    // { kind: "a"; } / { kind: "a"; } / { kind: "a"; } / { kind: "a"; }
    (
        "satKind",
        "{ kind: \"a\" }",
        "{ kind: \"a\" }",
        "{ kind: \"a\" }",
        "{ kind: \"a\" }",
    ),
    // { n: 1; } / { n: 1; } / { n: 1; } / { n: 1; }
    (
        "satIndexLit",
        "{ n: 1 }",
        "{ n: 1 }",
        "{ n: 1 }",
        "{ n: 1 }",
    ),
    // { k: "a"; }[] / { k: "a"; }[] / { k: "a"; }[] / { k: "a"; }[]
    (
        "satArrObj",
        "{ k: \"a\" }[]",
        "{ k: \"a\" }[]",
        "{ k: \"a\" }[]",
        "{ k: \"a\" }[]",
    ),
    // { xs: string[]; } / { xs: string[]; } / { xs: string[]; } / { xs: string[]; }
    (
        "satObjArr",
        "{ xs: string[] }",
        "{ xs: string[] }",
        "{ xs: string[] }",
        "{ xs: string[] }",
    ),
    // { xs: [string, number]; } / { xs: [string, number]; } / { xs: [string, number]; } / { xs: [string, number]; }
    (
        "satObjTuple",
        "{ xs: [string, number] }",
        "{ xs: [string, number] }",
        "{ xs: [string, number] }",
        "{ xs: [string, number] }",
    ),
    // { k: "a"; } / { k: "a"; } / { k: "a"; } / { k: "a"; }
    (
        "satIface",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
    ),
    // { f: 1; } / { f: 1; } / { f: 1; } / { f: 1; }
    (
        "satOptional",
        "{ f: 1 }",
        "{ f: 1 }",
        "{ f: 1 }",
        "{ f: 1 }",
    ),
    // { a: "x"; } / { a: "x"; } / { a: "x"; } / { a: "x"; }
    (
        "satTemplate",
        "{ a: \"x\" }",
        "{ a: \"x\" }",
        "{ a: \"x\" }",
        "{ a: \"x\" }",
    ),
    // { k: "a"; } / { k: "a"; } / { k: "a"; } / { k: "a"; }
    (
        "satLocal",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
    ),
    // { k: "a"; } / { k: "a"; } / { k: "a"; } / { k: "a"; }
    (
        "satLetLocal",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
    ),
    // { b: true; } / { b: true; } / { b: true; } / { b: true; }
    (
        "satBool",
        "{ b: true }",
        "{ b: true }",
        "{ b: true }",
        "{ b: true }",
    ),
    // { b: true; } / { b: true; } / { b: true; } / { b: true; }
    (
        "satBoolLit",
        "{ b: true }",
        "{ b: true }",
        "{ b: true }",
        "{ b: true }",
    ),
    // [string, number] / [string, number] / [string, number] / [string, number]
    (
        "satTupleSpread",
        "[string, number]",
        "[string, number]",
        "[string, number]",
        "[string, number]",
    ),
    // [string] / [string] / [string] / [string]
    (
        "satTupleOpt",
        "[string]",
        "[string]",
        "[string]",
        "[string]",
    ),
    // [] / [] / [] / []
    ("satEmptyTuple", "[]", "[]", "[]", "[]"),
    // { n: number; } / { n: number; } / { n: number; } / { n: number; }
    (
        "satAny",
        "{ n: number }",
        "{ n: number }",
        "{ n: number }",
        "{ n: number }",
    ),
    // (1 | 2)[] / (1 | 2)[] / (1 | 2)[] / (1 | 2)[]
    (
        "satNumIndex",
        "(1 | 2)[]",
        "(1 | 2)[]",
        "(1 | 2)[]",
        "(1 | 2)[]",
    ),
];

/// A `satisfies` literal bound to a `let` and read.
const SATISFIES_LOCAL_SOURCE: &str = r#"
interface IK { k: "a" | "b" }
export function letInline() { let o = { k: "a" } satisfies { k: "a" | "b" }; return o; }
export function letIface() { let o = { k: "a" } satisfies IK; return o; }
export function letParen() { let o = ({ k: "a" }) satisfies IK; return o; }
export function letRead() { let o = { k: "a" } satisfies IK; const p = o; return p; }
export function letAsConst() { let o = { k: "a" } as const; return o; }
export function letArr() { let o = ["a"] satisfies ("a" | "b")[]; return o; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`SATISFIES_LOCAL_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const SATISFIES_LOCAL_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // { k: "a"; } / { k: "a"; } / { k: "a"; } / { k: "a"; }
    (
        "letInline",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
    ),
    // { k: "a"; } / { k: "a"; } / { k: "a"; } / { k: "a"; }
    (
        "letIface",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
    ),
    // { k: "a"; } / { k: "a"; } / { k: "a"; } / { k: "a"; }
    (
        "letParen",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
    ),
    // { k: "a"; } / { k: "a"; } / { k: "a"; } / { k: "a"; }
    (
        "letRead",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
        "{ k: \"a\" }",
    ),
    // { readonly k: "a"; } / { readonly k: "a"; } / { readonly k: "a"; } / { readonly k: "a"; }
    (
        "letAsConst",
        "{ readonly k: \"a\" }",
        "{ readonly k: \"a\" }",
        "{ readonly k: \"a\" }",
        "{ readonly k: \"a\" }",
    ),
    // "a"[] / "a"[] / "a"[] / "a"[]
    ("letArr", "\"a\"[]", "\"a\"[]", "\"a\"[]", "\"a\"[]"),
];

/// `satisfies` contextually types its literal operand the way the checker
/// does: a fresh member or element literal keeps its literal type exactly
/// where the target's matching position is a literal context (a union of
/// literals, a literal, a type parameter constrained by one), an array
/// literal in a tuple context is a tuple, and every other position widens
/// as an unannotated literal does. Each cell matches its own project's
/// TypeScript 7.0.2 answer.
#[test]
fn satisfies_contextually_types_its_literal() {
    let host = four_policy_host();
    let mut mismatches = Vec::new();
    mismatches.extend(matrix_mismatches(
        &host,
        "satisfies.ts",
        SATISFIES_SOURCE,
        SATISFIES_TABLE,
    ));
    mismatches.extend(matrix_mismatches(
        &host,
        "satisfies-local.ts",
        SATISFIES_LOCAL_SOURCE,
        SATISFIES_LOCAL_TABLE,
    ));
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Equality tests against `null` and `undefined`.
const NULLISH_EQUALITY_SOURCE: &str = r#"
export function eqNullThen(x: string | null) { if (x === null) return x; return 0; }
export function eqNullElse(x: string | null) { if (x === null) { return 1; } return x; }
export function neNullThen(x: string | null) { if (x !== null) return x; return 0; }
export function eqUndefThen(x: string | undefined) { if (x === undefined) return x; return 0; }
export function eqNullObj(x: { a: string } | null) { if (x === null) return x; return 0; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`NULLISH_EQUALITY_SOURCE`], each the lane's
/// spelling of the checker's TypeScript 7.0.2 answer (the checker's prints
/// follow each row).
const NULLISH_EQUALITY_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // 0 | null / string | 0 / 0 | null / string | 0
    (
        "eqNullThen",
        "0 | null",
        "0 | string",
        "0 | null",
        "0 | string",
    ),
    // string | 1 / string | 1 / string | 1 / string | 1
    (
        "eqNullElse",
        "1 | string",
        "1 | string",
        "1 | string",
        "1 | string",
    ),
    // string | 0 / string | 0 / string | 0 / string | 0
    (
        "neNullThen",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // 0 | undefined / string | 0 / 0 | undefined / string | 0
    (
        "eqUndefThen",
        "0 | undefined",
        "0 | string",
        "0 | undefined",
        "0 | string",
    ),
    // 0 | null / 0 | { a: string; } / 0 | null / 0 | { a: string; }
    (
        "eqNullObj",
        "0 | null",
        "0 | { a: string }",
        "0 | null",
        "0 | { a: string }",
    ),
];

/// With `strictNullChecks` off an equality with `null` or `undefined`
/// narrows nothing on either edge (`narrowTypeByEquality` returns the type
/// unchanged): `if (x === null) return x` over `x: string | null` returns
/// `string`, never an empty narrow that drops the arm. Each cell matches its
/// own project's TypeScript 7.0.2 answer.
#[test]
fn strict_null_checks_off_equality_with_nullish_narrows_nothing() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "nullish-equality.ts",
        NULLISH_EQUALITY_SOURCE,
        NULLISH_EQUALITY_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:
{}",
        mismatches.join(
            "
"
        )
    );
}

/// `if (c) return { a: null }; return { a: undefined }` with
/// `strictNullChecks` off: the checker prints `{ a: any; } | { a: any; }` —
/// two fresh object identities of one structure, which the discriminant
/// shortcut keeps apart before widening — and the lane answers the one
/// object `{ a: any }` the canonical union collapses them to
/// (`T | T = T`; the discarded arm's member spans stay recoverable through
/// the normalization origin edge). Nothing but that print tells the two
/// apart. Measured on TypeScript 7.0.2 over the checker's own answer `R`:
/// `keyof R` is `"a"`, `R["a"]` is `any`, `R` is neither callable nor
/// constructable, `R` and `{ a: any }` are assignable each to the other,
/// and a consumer joining the value again reduces the twins to one
/// (`c ? twin(c) : { a: 1 }` is `{ a: any; }`). The lane's answer carries
/// the same effects: one member `a` of type `any`, no call, construct or
/// index signature, assignable each way with the lane's own `{ a: any }`,
/// and the same rejoined answer.
#[test]
fn an_object_join_of_structural_twins_differs_only_in_presentation() {
    const SOURCE: &str = r#"
export function twin(c: boolean) { if (c) return { a: null }; return { a: undefined }; }
export function single(v: any) { return { a: v }; }
export function twinTernary(c: boolean) { return c ? twin(c) : { a: 1 }; }
"#;
    let host = two_policy_host();
    let canonical = format!("{LOOSE_ROOT}/twins.ts");
    upsert(&host, &canonical, SOURCE);
    // { a: any; } | { a: any; }
    assert_eq!(observe(&host, &canonical, "twin"), "{ a: any }");
    // { a: any; }
    assert_eq!(observe(&host, &canonical, "twinTernary"), "{ a: any }");
    let node_of = |symbol: &str| {
        host.get_flow_return_type_with_audit(
            &identity(&canonical, symbol),
            ReturnProjectionDemand::whole_return(),
        )
        .as_result()
        .map(|result| result.return_type())
        .unwrap_or_else(|_| panic!("`{symbol}` answers a value"))
    };
    let twin = node_of("twin");
    let single = node_of("single");
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    let graph = dispatch.graph();
    let Some(SemanticNodeData::Object(view)) = graph.node_data(twin).as_deref().cloned() else {
        panic!("the twins collapse to one object");
    };
    assert!(
        view.call_signatures.is_empty()
            && view.construct_signatures.is_empty()
            && view.index_signatures.is_empty(),
        "neither twin is callable, constructable or indexed"
    );
    let [member] = view.positive_members() else {
        panic!("one member: {:?}", view.positive_members());
    };
    assert_eq!(member.key.as_string(), Some("a"));
    assert!(matches!(
        graph.node_data(member.value).as_deref(),
        Some(SemanticNodeData::Primitive(PrimitiveKind::Any))
    ));
    for (source, target) in [(twin, single), (single, twin)] {
        assert!(
            matches!(
                dispatch.execute_relate_pair(source, target),
                super::dispatch_txn::RelationStep::Assignable { .. }
            ),
            "the twins' object and `{{ a: any }}` are assignable each to the other"
        );
    }
}

/// `if (c) return a; return b` (and `c ? a : b`) over two structurally
/// identical interfaces `A1` and `B1`: each is a strict subtype of the
/// other, so the checker's `removeSubtypes` keeps the one its own type
/// order visits first and prints `A1` (TypeScript 7.0.2, with and without
/// `strictNullChecks`). The lane visits the operands in the union's
/// `VerterStableV1` order and keeps the one that order puts first, which
/// the stable key decides per project: in the `strictNullChecks`-off
/// project at `/off/p.ts` it is `B1`. The counterfactual isolates the order
/// as the only cause: a fresh store with ONLY its union order reversed
/// answers the checker's `A1`, and a fresh store under the stable order
/// answers `B1` again.
#[test]
fn a_structural_twin_survivor_follows_the_stable_union_order() {
    const SOURCE: &str = r#"
interface A1 { x: string }
interface B1 { x: string }
export function retSameIface(c: boolean, a: A1, b: B1) { if (c) return a; return b; }
export function ternSameIface(c: boolean, a: A1, b: B1) { return c ? a : b; }
"#;
    let survivors = |reversed: bool| {
        let host = VerterHost::new_standalone_with_tsconfig_projects(
            HostConfig::default(),
            &[(
                "/off",
                r#"{ "compilerOptions": { "strict": true, "strictNullChecks": false } }"#,
            )],
        );
        if reversed {
            host.project_type_store()
                .semantic_graph()
                .reverse_union_order_for_tests();
        }
        upsert(&host, "/off/p.ts", SOURCE);
        ["retSameIface", "ternSameIface"].map(|symbol| observe(&host, "/off/p.ts", symbol))
    };
    // A1 / A1 on the checker.
    assert_eq!(
        survivors(false),
        ["B1", "B1"],
        "the stable order's survivors"
    );
    assert_eq!(
        survivors(true),
        ["A1", "A1"],
        "reversing ONLY the union order answers the checker's survivor"
    );
    assert_eq!(
        survivors(false),
        ["B1", "B1"],
        "the stable order recovers the recorded survivor"
    );
}

/// Loose and strict literal equality, `unknown` and empty-object arms, and
/// `case` clauses.
const EQUALITY_NARROWING_SOURCE: &str = r#"
interface Foo { a: string }
export function looseNull(x: string | undefined) { if (x == null) return x; return 0; }
export function looseNotNull(x: string | null | undefined) { if (x != null) return x; return 0; }
export function looseUndef(x: string | null) { if (x == undefined) return x; return 0; }
export function looseNullRev(x: number | null) { if (null == x) return 1; return x; }
export function looseUnknown(x: unknown) { if (x == null) return x; return 0; }
export function looseUnknownNot(x: unknown) { if (x != null) return x; return 0; }
export function looseAny(x: any) { if (x == null) return x; return 0; }
export function looseObj(x: Foo | null) { if (x != null) return x.a; return 0; }
export function looseDisc(o: { k: string | null; v: number }) { if (o.k == null) return 1; return o.k; }
export function looseTernary(x: string | null) { return x == null ? 0 : x; }
export function looseStr(x: string | number) { if (x == "1") return x; return true; }
export function u1(x: unknown) { if (x !== null) return x; return 0; }
export function u2(x: unknown) { if (x !== undefined) return x; return 0; }
export function u3(x: unknown) { if (x !== null && x !== undefined) return x; return 0; }
export function u4(x: unknown) { if (x === null) return 1; return x; }
export function u5(x: unknown) { if (x) return x; return 0; }
export function u6(x: unknown) { if (x != undefined) return x; return 0; }
export function u7(x: unknown) { if (x === null) return x; return 0; }
export function u8(o: { k: string | null | undefined }) { if (o.k !== undefined && o.k !== null) return o.k; return 0; }
export function u9(o: { k: string | null }) { if (o.k === "a") return o.k; return 0; }
export function u10(o: { k: unknown }) { if (o.k != null) return o.k; return 0; }
export function u11(x: unknown) { if (!x) return x; return 0; }
export function u12(o: { k: unknown }) { if (o.k) return o.k; return 0; }
export function s1(x: unknown) { if (x === "1") return x; return 0; }
export function s2(x: unknown) { if (x == "1") return x; return 0; }
export function s3(x: {} | number) { if (x === "1") return x; return false; }
export function s4(x: {} | number) { if (x == "1") return x; return false; }
export function s5(x: string | number) { if (x != "1") return x; return true; }
export function s6(x: boolean | string) { if (x == true) return x; return 0; }
export function s7(x: number | string) { if (x == 1) return x; return true; }
export function s8(o: { k: string | number }) { if (o.k == "a") return o.k; return 0; }
export function s9(x: "a" | "b" | 1) { if (x == "a") return x; return true; }
export function s10(x: "a" | "b" | 1) { if (x != "a") return x; return true; }
export function s11(x: any) { if (x == "a") return x; return true; }
export function s12(x: {}) { if (x == "1") return x; return false; }
export function s13(x: {}) { if (x === "1") return x; return false; }
export function s14(x: string | number) { if (typeof x == "string") return x; return true; }
export function s15(x: string | number) { if (typeof x != "string") return x; return true; }
export function sw1(x: unknown) { switch (x) { case "a": return x; } return 0; }
export function sw2(x: {} | number) { switch (x) { case "a": return x; } return false; }
export function sw3(x: unknown) { switch (x) { case "a": return x; default: return 0; } }
export function sw4(x: unknown) { switch (x) { case "a": default: return x; } }
export function sw5(x: {}) { switch (x) { case "a": return x; } return false; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`EQUALITY_NARROWING_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const EQUALITY_NARROWING_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // 0 | undefined / string | 0 / 0 | undefined / string | 0
    (
        "looseNull",
        "0 | undefined",
        "0 | string",
        "0 | undefined",
        "0 | string",
    ),
    // string | 0 / string | 0 / string | 0 / string | 0
    (
        "looseNotNull",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // 0 | null / string | 0 / 0 | null / string | 0
    (
        "looseUndef",
        "0 | null",
        "0 | string",
        "0 | null",
        "0 | string",
    ),
    // number / number / number / number
    ("looseNullRev", "number", "number", "number", "number"),
    // 0 | null | undefined / unknown / 0 | null | undefined / unknown
    (
        "looseUnknown",
        "0 | null | undefined",
        "unknown",
        "0 | null | undefined",
        "unknown",
    ),
    // {} / unknown / {} / unknown
    ("looseUnknownNot", "{  }", "unknown", "{  }", "unknown"),
    // any / any / any / any
    ("looseAny", "any", "any", "any", "any"),
    // string | 0 / string | 0 / string | 0 / string | 0
    (
        "looseObj",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // string | 1 / string | 1 / string | 1 / string | 1
    (
        "looseDisc",
        "1 | string",
        "1 | string",
        "1 | string",
        "1 | string",
    ),
    // string | 0 / string | 0 / string | 0 / string | 0
    (
        "looseTernary",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // "1" | true / "1" | true / "1" | true / "1" | true
    (
        "looseStr",
        "\"1\" | true",
        "\"1\" | true",
        "\"1\" | true",
        "\"1\" | true",
    ),
    // {} | undefined / unknown / {} | undefined / unknown
    (
        "u1",
        "undefined | {  }",
        "unknown",
        "undefined | {  }",
        "unknown",
    ),
    // {} | null / unknown / {} | null / unknown
    ("u2", "null | {  }", "unknown", "null | {  }", "unknown"),
    // {} / unknown / {} / unknown
    ("u3", "{  }", "unknown", "{  }", "unknown"),
    // {} | undefined / unknown / {} | undefined / unknown
    (
        "u4",
        "undefined | {  }",
        "unknown",
        "undefined | {  }",
        "unknown",
    ),
    // {} / unknown / {} / unknown
    ("u5", "{  }", "unknown", "{  }", "unknown"),
    // {} / unknown / {} / unknown
    ("u6", "{  }", "unknown", "{  }", "unknown"),
    // 0 | null / unknown / 0 | null / unknown
    ("u7", "0 | null", "unknown", "0 | null", "unknown"),
    // string | 0 / string | 0 / string | 0 / string | 0
    ("u8", "0 | string", "0 | string", "0 | string", "0 | string"),
    // "a" | 0 / "a" | 0 / "a" | 0 / "a" | 0
    ("u9", "\"a\" | 0", "\"a\" | 0", "\"a\" | 0", "\"a\" | 0"),
    // {} / unknown / {} / unknown
    ("u10", "{  }", "unknown", "{  }", "unknown"),
    // unknown / unknown / unknown / unknown
    ("u11", "unknown", "unknown", "unknown", "unknown"),
    // {} / unknown / {} / unknown
    ("u12", "{  }", "unknown", "{  }", "unknown"),
    // "1" | 0 / "1" | 0 / "1" | 0 / "1" | 0
    ("s1", "\"1\" | 0", "\"1\" | 0", "\"1\" | 0", "\"1\" | 0"),
    // unknown / unknown / unknown / unknown
    ("s2", "unknown", "unknown", "unknown", "unknown"),
    // "1" | false / "1" | false / "1" | false / "1" | false
    (
        "s3",
        "\"1\" | false",
        "\"1\" | false",
        "\"1\" | false",
        "\"1\" | false",
    ),
    // {} / {} / {} / {}
    ("s4", "{  }", "{  }", "{  }", "{  }"),
    // string | number | true / string | number | true / string | number | true / string | number | true
    (
        "s5",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // 0 | true / 0 | true / 0 | true / 0 | true
    ("s6", "0 | true", "0 | true", "0 | true", "0 | true"),
    // 1 | true / 1 | true / 1 | true / 1 | true
    ("s7", "1 | true", "1 | true", "1 | true", "1 | true"),
    // "a" | 0 / "a" | 0 / "a" | 0 / "a" | 0
    ("s8", "\"a\" | 0", "\"a\" | 0", "\"a\" | 0", "\"a\" | 0"),
    // "a" | true / "a" | true / "a" | true / "a" | true
    (
        "s9",
        "\"a\" | true",
        "\"a\" | true",
        "\"a\" | true",
        "\"a\" | true",
    ),
    // "b" | 1 | true / "b" | 1 | true / "b" | 1 | true / "b" | 1 | true
    (
        "s10",
        "\"b\" | 1 | true",
        "\"b\" | 1 | true",
        "\"b\" | 1 | true",
        "\"b\" | 1 | true",
    ),
    // any / any / any / any
    ("s11", "any", "any", "any", "any"),
    // {} / {} / {} / {}
    ("s12", "{  }", "{  }", "{  }", "{  }"),
    // "1" | false / "1" | false / "1" | false / "1" | false
    (
        "s13",
        "\"1\" | false",
        "\"1\" | false",
        "\"1\" | false",
        "\"1\" | false",
    ),
    // string | true / string | true / string | true / string | true
    (
        "s14",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // number | true / number | true / number | true / number | true
    (
        "s15",
        "number | true",
        "number | true",
        "number | true",
        "number | true",
    ),
    // "a" | 0 / "a" | 0 / "a" | 0 / "a" | 0
    ("sw1", "\"a\" | 0", "\"a\" | 0", "\"a\" | 0", "\"a\" | 0"),
    // {} / {} / {} / {}
    ("sw2", "{  }", "{  }", "{  }", "{  }"),
    // "a" | 0 / "a" | 0 / "a" | 0 / "a" | 0
    ("sw3", "\"a\" | 0", "\"a\" | 0", "\"a\" | 0", "\"a\" | 0"),
    // unknown / unknown / unknown / unknown
    ("sw4", "unknown", "unknown", "unknown", "unknown"),
    // {} / {} / {} / {}
    ("sw5", "{  }", "{  }", "{  }", "{  }"),
];

/// Every row is measured on TypeScript 7.0.2 in the four
/// `strictNullChecks` × `noImplicitAny` projects:
///
/// - `x == null` selects both nullish members and `x != null` removes both
///   (`looseNull`, `looseNotNull`, `looseUndef`), on a member path too
///   (`looseDisc`: `o.k == null` narrows `o.k` itself, `string | 1`);
/// - with `strictNullChecks`, `unknown` reads as `{} | null | undefined`:
///   `x !== null` is `{} | undefined`, `x !== undefined` is `{} | null`,
///   both together and a truthy test are `{}`, and the return join absorbs
///   a primitive beside `{}` (`u1`: `{} | undefined`, the `0` gone);
/// - `==` against a non-nullish literal narrows as `===` does
///   (`looseStr`: `"1" | true`) except over `unknown` and an empty object
///   arm, which it only filters (`s2`: `unknown`, `s4`: `{}`), where
///   `===` reads the literal (`s1`, `s3`: `"1" | false`);
/// - a `case` clause reads the literal over `unknown` and filters over
///   `{}` (`sw1`: `"a" | 0`, `sw2`: `{}`);
/// - `typeof x == "string"` narrows as the strict spelling (`s14`, `s15`).
#[test]
fn equality_narrows_by_the_checkers_literal_comparison_rules() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "equality-narrowing.ts",
        EQUALITY_NARROWING_SOURCE,
        EQUALITY_NARROWING_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Literal equalities whose narrow takes the compared literal from the
/// compared value.
const FRESH_EQUALITY_LITERAL_SOURCE: &str = r#"
export function f1(x: string) { if (x === "s") return x; throw 0; }
export function f2(x: string) { if (x === "s") { let y = x; return y; } throw 0; }
export function f3(x: string) { if (x === "s") { const y = x; return y; } throw 0; }
export function f4(x: string) { if (x === "s") return [x]; throw 0; }
export function f5(x: string) { if (x === "s") return { v: x }; throw 0; }
export function f6(x: string) { switch (x) { case "s": return x; } throw 0; }
export function f7(x: string | number) { if (x === 1) return x; throw 0; }
export function f8(x: string) { return x === "s" ? x : "s"; }
export function f9(x: boolean) { if (x === true) return x; throw 0; }
export function f10(x: string) { if (x !== "s") throw 0; return x; }
export function f11(x: string) { if (x === "s" || x === "t") return x; throw 0; }
export function f12(o: { k: string }) { if (o.k === "s") return o.k; throw 0; }
export function f13(x: string) { if (x == "s") return x; throw 0; }
export function f14(x: string) { if (x === "s") return x; return x; }
export function f15(x: string, c: boolean) { if (x === "s") return c ? x : x; throw 0; }
export function f16(x: unknown) { if (x === "s") return x; throw 0; }
export function f17(x: unknown) { if (x === true) return x; throw 0; }
export function f18(x: "s" | number) { if (x === "s") return x; throw 0; }
export function f19(x: string) { if (x === "s") { const y = x; let z = y; return z; } throw 0; }
export function f21(x: string | number) { if (x === "s") return [x]; throw 0; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`FRESH_EQUALITY_LITERAL_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const FRESH_EQUALITY_LITERAL_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string / string / string / string
    ("f1", "string", "string", "string", "string"),
    // string / string / string / string
    ("f2", "string", "string", "string", "string"),
    // string / string / string / string
    ("f3", "string", "string", "string", "string"),
    // string[] / string[] / string[] / string[]
    ("f4", "string[]", "string[]", "string[]", "string[]"),
    // { v: string; } / { v: string; } / { v: string; } / { v: string; }
    (
        "f5",
        "{ v: string }",
        "{ v: string }",
        "{ v: string }",
        "{ v: string }",
    ),
    // "s" / "s" / "s" / "s"
    ("f6", "\"s\"", "\"s\"", "\"s\"", "\"s\""),
    // number / number / number / number
    ("f7", "number", "number", "number", "number"),
    // string / string / string / string
    ("f8", "string", "string", "string", "string"),
    // true / true / true / true
    ("f9", "true", "true", "true", "true"),
    // string / string / string / string
    ("f10", "string", "string", "string", "string"),
    // "s" | "t" / "s" | "t" / "s" | "t" / "s" | "t"
    (
        "f11",
        "\"s\" | \"t\"",
        "\"s\" | \"t\"",
        "\"s\" | \"t\"",
        "\"s\" | \"t\"",
    ),
    // string / string / string / string
    ("f12", "string", "string", "string", "string"),
    // string / string / string / string
    ("f13", "string", "string", "string", "string"),
    // string / string / string / string
    ("f14", "string", "string", "string", "string"),
    // string / string / string / string
    ("f15", "string", "string", "string", "string"),
    // string / string / string / string
    ("f16", "string", "string", "string", "string"),
    // boolean / boolean / boolean / boolean
    ("f17", "boolean", "boolean", "boolean", "boolean"),
    // "s" / "s" / "s" / "s"
    ("f18", "\"s\"", "\"s\"", "\"s\"", "\"s\""),
    // string / string / string / string
    ("f19", "string", "string", "string", "string"),
    // string[] / string[] / string[] / string[]
    ("f21", "string[]", "string[]", "string[]", "string[]"),
];

/// A narrow that replaces a primitive with the compared literal reads the
/// checker's FRESH literal type, which widens where a bare literal would
/// (TypeScript 7.0.2, every project): alone as a return (`f1`, `f10`,
/// `f12` over a member path, `f16` over `unknown`, `f17`: `boolean`), at a
/// mutable declaration or through a `const` (`f2`, `f3`, `f19`), in an
/// array or object literal (`f4`: `string[]`, `f5`: `{ v: string; }`,
/// `f21`), and as both arms of a conditional (`f8`, `f15`). A literal the
/// reference already declared is not fresh (`f18`: `"s"`, `f9`: `true`),
/// a `case` clause's literal is not either (`f6`: `"s"`), and a union of
/// literals is never widened (`f11`: `"s" | "t"`).
#[test]
fn an_equality_narrow_from_the_compared_value_is_a_fresh_literal() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "fresh-equality-literal.ts",
        FRESH_EQUALITY_LITERAL_SOURCE,
        FRESH_EQUALITY_LITERAL_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Index-signature sources against named members, and `any` / `unknown` /
/// `never` against every kind of target.
const INDEX_AND_ANY_RELATION_SOURCE: &str = r#"
declare const __v_idxToReq: ({ [k: string]: string }) extends ({ a: string }) ? "y" : "n";
export function idxToReq() { return __v_idxToReq; }
declare const __v_idxToOpt: ({ [k: string]: string }) extends ({ a?: string }) ? "y" : "n";
export function idxToOpt() { return __v_idxToOpt; }
declare const __v_idxToOptBad: ({ [k: string]: number }) extends ({ a?: string }) ? "y" : "n";
export function idxToOptBad() { return __v_idxToOptBad; }
declare const __v_idxNumToReq: ({ [k: number]: string }) extends ({ 0: string }) ? "y" : "n";
export function idxNumToReq() { return __v_idxNumToReq; }
declare const __v_idxToReqAny: ({ [k: string]: any }) extends ({ a: string }) ? "y" : "n";
export function idxToReqAny() { return __v_idxToReqAny; }
declare const __v_idxToEmpty: ({ [k: string]: string }) extends ({}) ? "y" : "n";
export function idxToEmpty() { return __v_idxToEmpty; }
declare const __v_idxAndPropToReq: ({ [k: string]: string; a: string }) extends ({ a: string; b: string }) ? "y" : "n";
export function idxAndPropToReq() { return __v_idxAndPropToReq; }
declare const __v_tmplIdxToReq: ({ [k: `a${string}`]: string }) extends ({ ab: string }) ? "y" : "n";
export function tmplIdxToReq() { return __v_tmplIdxToReq; }
declare const __v_anyToUnknown: (any) extends (unknown) ? "y" : "n";
export function anyToUnknown() { return __v_anyToUnknown; }
declare const __v_anyToAny: (any) extends (any) ? "y" : "n";
export function anyToAny() { return __v_anyToAny; }
declare const __v_anyToStr: (any) extends (string) ? "y" : "n";
export function anyToStr() { return __v_anyToStr; }
declare const __v_anyToNever: (any) extends (never) ? "y" : "n";
export function anyToNever() { return __v_anyToNever; }
declare const __v_neverToStr: (never) extends (string) ? "y" : "n";
export function neverToStr() { return __v_neverToStr; }
declare const __v_unknownToAny: (unknown) extends (any) ? "y" : "n";
export function unknownToAny() { return __v_unknownToAny; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`INDEX_AND_ANY_RELATION_SOURCE`], each the lane's spelling of
/// the checker's TypeScript 7.0.2 answer (the checker's prints follow each
/// row).
const INDEX_AND_ANY_RELATION_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "n" / "n" / "n" / "n"
    ("idxToReq", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("idxToOpt", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("idxToOptBad", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("idxNumToReq", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "n" / "n" / "n" / "n"
    ("idxToReqAny", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("idxToEmpty", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("idxAndPropToReq", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "n" / "n" / "n" / "n"
    ("tmplIdxToReq", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("anyToUnknown", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("anyToAny", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" | "y" / "n" | "y" / "n" | "y" / "n" | "y"
    (
        "anyToStr",
        "\"n\" | \"y\"",
        "\"n\" | \"y\"",
        "\"n\" | \"y\"",
        "\"n\" | \"y\"",
    ),
    // "n" | "y" / "n" | "y" / "n" | "y" / "n" | "y"
    (
        "anyToNever",
        "\"n\" | \"y\"",
        "\"n\" | \"y\"",
        "\"n\" | \"y\"",
        "\"n\" | \"y\"",
    ),
    // "y" / "y" / "y" / "y"
    ("neverToStr", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("unknownToAny", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
];

/// Measured as `S extends T ? "y" : "n"` on TypeScript 7.0.2, every
/// project: an index signature never satisfies a REQUIRED named member
/// (`idxToReq`, `idxNumToReq`, `idxToReqAny`, `idxAndPropToReq`,
/// `tmplIdxToReq`: `"n"`), while an OPTIONAL one is satisfied without
/// relating the index value (`idxToOpt` and `idxToOptBad`: `"y"`); an `any`
/// check type takes the true branch alone against `any` or `unknown`
/// (`anyToUnknown`, `anyToAny`: `"y"`) and both branches against anything
/// else (`anyToStr`, `anyToNever`: `"n" | "y"`).
#[test]
fn index_signatures_and_any_relate_by_the_checkers_rules() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "index-and-any-relations.ts",
        INDEX_AND_ANY_RELATION_SOURCE,
        INDEX_AND_ANY_RELATION_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Returns no path reaches: after a never-returning call (a statement or a
/// comma operand, the return's own included), a `throw`, a loop that never
/// exits, or another `return`.
const UNREACHABLE_RETURN_SOURCE: &str = r#"
function fail(): never { throw 0; }
export function callStmt(x: string | number) { fail(); return x; }
export function callComma(x: string | number) { return (fail(), x); }
export function commaStmt(x: string | number) { (0, fail()); return x; }
export function throwStmt(x: string | number) { throw 0; return x; }
export function whileTrue(x: string | number) { while (true) {} return x; }
export function forEver(x: string | number) { for (;;) {} return x; }
export function afterReturn(x: string | number) { return 1; return x; }
export function afterFailMixed(x: string | number, c: boolean) { if (c) return true; fail(); return x; }
export function narrowedBefore(x: string | number) { if (typeof x === "string") { fail(); return x; } return 0; }
export function literalUnreach() { fail(); return "lit"; }
export function nullUnreach() { fail(); return null; }
export function letInit() { let y: string | undefined = "s"; fail(); return y; }
export function emptyReturn(x: string | number, c: boolean) { if (c) return x; fail(); return; }
export function emptyReturnOnly() { fail(); return; }
export function letEvolving() { let y; y = 1; fail(); return y; }
export function letEvolvingNull() { let y = null; y = 1; fail(); return y; }
export function varEvolving() { var y; y = "s"; throw 0; return y; }
export function letNumber() { let y = 1; y = 2; fail(); return y; }
"#;

/// `(symbol, strict, strictNullChecks off, noImplicitAny off, both off)`
/// for [`UNREACHABLE_RETURN_SOURCE`], each the lane's spelling of that
/// project's TypeScript 7.0.2 `.d.ts` answer.
const UNREACHABLE_RETURN_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    (
        "callStmt",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "callComma",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "commaStmt",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "throwStmt",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "whileTrue",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "forEver",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "afterReturn",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "afterFailMixed",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    (
        "narrowedBefore",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    ("literalUnreach", "string", "string", "string", "string"),
    ("nullUnreach", "null", "any", "null", "any"),
    (
        "letInit",
        "string | undefined",
        "string",
        "string | undefined",
        "string",
    ),
    (
        "emptyReturn",
        "number | string | undefined",
        "number | string",
        "number | string | undefined",
        "number | string",
    ),
    ("emptyReturnOnly", "void", "void", "void", "void"),
    ("letEvolving", "any", "any", "any", "any"),
    ("letEvolvingNull", "any", "any", "null", "any"),
    ("varEvolving", "any", "any", "any", "any"),
    ("letNumber", "number", "number", "number", "number"),
];

/// The checker infers a return type from EVERY `return` of the body,
/// reachable or not (`checkAndAggregateReturnExpressionTypes` walks them
/// all), and a reference no path reaches reads its declared type: a
/// parameter its annotation, never a narrow (`narrowedBefore`); an
/// annotated local its annotation (`letInit`); an auto-typed local — an
/// unannotated `let` / `var` with no initializer or a `null` / `undefined`
/// one under `noImplicitAny` — `any` (`letEvolving`, `varEvolving`,
/// `letEvolvingNull`, which without `noImplicitAny` is declared `null`).
/// An unreachable bare `return;` adds `undefined` beside other returns under
/// `strictNullChecks` and is `void` alone. Each cell matches its own
/// project's TypeScript 7.0.2 answer.
#[test]
fn unreachable_returns_contribute_their_declared_reads() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "unreachable.ts",
        UNREACHABLE_RETURN_SOURCE,
        UNREACHABLE_RETURN_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}
