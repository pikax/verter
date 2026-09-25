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
        SemanticNodeData::Literal(LiteralValue::BigInt(value)) => format!("{value}n"),
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

/// Four projects over the `catch`-variable axis, under the four root
/// names of [`four_policy_host`]: `strict`; `strict` off; `strict` with
/// `useUnknownInCatchVariables` off; `strict` off with
/// `useUnknownInCatchVariables` on.
fn catch_policy_host() -> VerterHost {
    VerterHost::new_standalone_with_tsconfig_projects(
        HostConfig::default(),
        &[
            (STRICT_ROOT, r#"{ "compilerOptions": { "strict": true } }"#),
            (LOOSE_ROOT, r#"{ "compilerOptions": { "strict": false } }"#),
            (
                STRICT_IMPLICIT_ROOT,
                r#"{ "compilerOptions": { "strict": true, "useUnknownInCatchVariables": false } }"#,
            ),
            (
                LOOSE_IMPLICIT_ROOT,
                r#"{ "compilerOptions": { "strict": false, "useUnknownInCatchVariables": true } }"#,
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

/// User aliases whose body intersects its parameter with `{}` or `unknown`,
/// applied to unions with `null` and to single types.
const USER_INTERSECTION_ALIAS_SOURCE: &str = r#"
type NN<T> = T & {};
type NU<T> = T & unknown;
declare function nn<T>(v: T): NN<T>;
export function nnParam(v: NN<string | null>) { return v; }
export function nnUndef(v: NN<string | undefined | null>) { return v; }
export function nnNum(v: NN<number | string | null>) { return v; }
export function nnObj(v: NN<{ a: string } | null>) { return v; }
export function nnLit(v: NN<"a" | null>) { return v; }
export function nnNull(v: NN<null>) { return v; }
export function nnUnknown(v: NN<unknown>) { return v; }
export function nnAny(v: NN<any>) { return v; }
export function nuParam(v: NU<string | null>) { return v; }
export function nnCall(v: string | null) { return nn(v); }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`USER_INTERSECTION_ALIAS_SOURCE`], read at the
/// checker's print altitude; the TypeScript 7.0.2 `.d.ts` print follows
/// each row.
const USER_INTERSECTION_ALIAS_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string / string / string / string
    ("nnParam", "string", "string", "string", "string"),
    // string / string / string / string
    ("nnUndef", "string", "string", "string", "string"),
    // NN<string | number | null> / NN<string | number> (twice each)
    (
        "nnNum",
        "NN<null | number | string>",
        "NN<number | string>",
        "NN<null | number | string>",
        "NN<number | string>",
    ),
    // { a: string; } / { a: string; } / { a: string; } / { a: string; }
    (
        "nnObj",
        "{ a: string }",
        "{ a: string }",
        "{ a: string }",
        "{ a: string }",
    ),
    // "a" / "a" / "a" / "a"
    ("nnLit", "\"a\"", "\"a\"", "\"a\"", "\"a\""),
    // never / never / never / never
    ("nnNull", "never", "never", "never", "never"),
    // {} / {} / {} / {}
    ("nnUnknown", "{  }", "{  }", "{  }", "{  }"),
    // any / any / any / any
    ("nnAny", "any", "any", "any", "any"),
    // string | null / string / string | null / string
    (
        "nuParam",
        "null | string",
        "string",
        "null | string",
        "string",
    ),
    // string / string / string / string
    ("nnCall", "string", "string", "string", "string"),
];

/// An application of a user alias whose body is an intersection is the
/// intersection the checker constructs from the substituted arms: a union
/// argument distributes, `null & {}` and `undefined & {}` are `never`, and
/// `{}` beside a type that is never `null` or `undefined` drops — the
/// written `string & {}` the checker keeps is the authored pair, not an
/// instantiated one. `NN<string | null>` (`type NN<T> = T & {}`) is
/// therefore `string` with and without `strictNullChecks`, and the checker
/// names the alias only for an intersection or distributed union it
/// constructs (`NN<string | number | null>`), printing a reduced
/// constituent (`{ a: string; }`) or an argument returned as it is
/// (`string | null` for `type NU<T> = T & unknown`) as itself. Each cell
/// is its project's TypeScript 7.0.2 answer at the print altitude.
#[test]
fn user_intersection_aliases_reduce_their_applications() {
    let host = four_policy_host();
    let roots = [
        STRICT_ROOT,
        LOOSE_ROOT,
        STRICT_IMPLICIT_ROOT,
        LOOSE_IMPLICIT_ROOT,
    ];
    for root in roots {
        upsert(
            &host,
            &format!("{root}/user-aliases.ts"),
            USER_INTERSECTION_ALIAS_SOURCE,
        );
    }
    let mut mismatches = Vec::new();
    for (symbol, strict, loose, strict_implicit, loose_implicit) in USER_INTERSECTION_ALIAS_TABLE {
        for (root, expected) in
            roots
                .into_iter()
                .zip([strict, loose, strict_implicit, loose_implicit])
        {
            let canonical = format!("{root}/user-aliases.ts");
            let observed = observe_at_checker_altitude(&host, &canonical, symbol);
            if observed != *expected {
                mismatches.push(format!(
                    "{canonical} `{symbol}`: expected `{expected}`, observed `{observed}`"
                ));
            }
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

/// Mapped types over a key domain holding an index key (`Record<string,
/// V>`, `{ [K in string]: K }`, `Record<"a" | number, V>`) against named
/// members, index signatures and a record.
const MAPPED_INDEX_RELATION_SOURCE: &str = r#"
declare const __v_recToReq: (Record<string, string>) extends ({ a: string }) ? "y" : "n";
export function recToReq() { return __v_recToReq; }
declare const __v_recToOpt: (Record<string, string>) extends ({ a?: string }) ? "y" : "n";
export function recToOpt() { return __v_recToOpt; }
declare const __v_recToOptBad: (Record<string, number>) extends ({ a?: string }) ? "y" : "n";
export function recToOptBad() { return __v_recToOptBad; }
declare const __v_recNumToReq: (Record<number, string>) extends ({ 0: string }) ? "y" : "n";
export function recNumToReq() { return __v_recNumToReq; }
declare const __v_recToIndex: (Record<string, string>) extends ({ [k: string]: string }) ? "y" : "n";
export function recToIndex() { return __v_recToIndex; }
declare const __v_recToIndexBad: (Record<string, number>) extends ({ [k: string]: string }) ? "y" : "n";
export function recToIndexBad() { return __v_recToIndexBad; }
declare const __v_recToRecord: (Record<string, string>) extends (Record<string, unknown>) ? "y" : "n";
export function recToRecord() { return __v_recToRecord; }
declare const __v_recMixedToReq: (Record<"a" | number, string>) extends ({ a: string }) ? "y" : "n";
export function recMixedToReq() { return __v_recMixedToReq; }
declare const __v_recMixedToMissing: (Record<"a" | number, string>) extends ({ b: string }) ? "y" : "n";
export function recMixedToMissing() { return __v_recMixedToMissing; }
declare const __v_recMixedBadValue: (Record<"a" | number, number>) extends ({ a: string }) ? "y" : "n";
export function recMixedBadValue() { return __v_recMixedBadValue; }
declare const __v_mappedStrToReq: ({ [K in string]: K }) extends ({ a: string }) ? "y" : "n";
export function mappedStrToReq() { return __v_mappedStrToReq; }
declare const __v_mappedStrToIndex: ({ [K in string]: K }) extends ({ [k: string]: string }) ? "y" : "n";
export function mappedStrToIndex() { return __v_mappedStrToIndex; }
declare const __v_recToEmpty: (Record<string, string>) extends ({}) ? "y" : "n";
export function recToEmpty() { return __v_recToEmpty; }
declare const __v_optMappedToReq: ({ [K in string]?: string }) extends ({ a: string }) ? "y" : "n";
export function optMappedToReq() { return __v_optMappedToReq; }
declare const __v_optMappedToIndex: ({ [K in string]?: number }) extends ({ [k: string]: number }) ? "y" : "n";
export function optMappedToIndex() { return __v_optMappedToIndex; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`MAPPED_INDEX_RELATION_SOURCE`], each the lane's
/// spelling of the checker's TypeScript 7.0.2 answer (the checker's
/// prints follow each row).
const MAPPED_INDEX_RELATION_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "n" / "n" / "n" / "n"
    ("recToReq", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("recToOpt", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("recToOptBad", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("recNumToReq", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("recToIndex", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("recToIndexBad", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("recToRecord", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("recMixedToReq", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("recMixedToMissing", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "n" / "n" / "n" / "n"
    ("recMixedBadValue", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "n" / "n" / "n" / "n"
    ("mappedStrToReq", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "y" / "y" / "y" / "y"
    ("mappedStrToIndex", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "y" / "y" / "y" / "y"
    ("recToEmpty", "\"y\"", "\"y\"", "\"y\"", "\"y\""),
    // "n" / "n" / "n" / "n"
    ("optMappedToReq", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // "n" / "y" / "n" / "y"
    ("optMappedToIndex", "\"n\"", "\"y\"", "\"n\"", "\"y\""),
];

/// A mapped type whose key domain holds an index key relates as the
/// object the checker resolves it to (`resolveMappedTypeMembers`): a
/// string literal key a property, `string` / `number` an index signature,
/// each valued by the template at that key. So its index signature never
/// satisfies a REQUIRED named target member (`Record<string, string>
/// extends { a: string }` is `"n"`), an OPTIONAL one is satisfied without
/// relating the index value (`Record<string, number> extends { a?: string
/// }` is `"y"`), and a `?` modifier makes the index value `undefined`-able
/// under `strictNullChecks` alone (`{ [K in string]?: number }` against `{
/// [k: string]: number }` is `"n"` strict, `"y"` without it). Measured as
/// `S extends T ? "y" : "n"` on TypeScript 7.0.2 in the four projects.
#[test]
fn mapped_sources_relate_through_their_index_signatures() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "mapped-index-relations.ts",
        MAPPED_INDEX_RELATION_SOURCE,
        MAPPED_INDEX_RELATION_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Entered `asserts` calls in a conditional's arms — a ternary's, or a
/// `&&` / `||` right operand — and the statement `if` twins.
const CONDITIONAL_JOIN_SOURCE: &str = r#"
function isStr(x: unknown): asserts x is string { if (typeof x !== "string") throw 0; }
function isNum(x: unknown): asserts x is number { if (typeof x !== "number") throw 0; }
function notStr(x: unknown): asserts x is number | boolean { if (typeof x === "string") throw 0; }
export function sameArms(x: string | number | boolean, c: boolean) { c ? (0, isStr(x)) : (0, isStr(x)); return x; }
export function diffArms(x: string | number | boolean, c: boolean) { c ? (0, isStr(x)) : (0, isNum(x)); return x; }
export function overlapArms(x: string | number | boolean, c: boolean) { c ? (0, isStr(x)) : (0, notStr(x)); return x; }
export function oneArm(x: string | number | boolean, c: boolean) { c ? (0, isStr(x)) : 0; return x; }
export function twoRefs(x: string | number | boolean, y: string | number | boolean, c: boolean) { c ? (0, isStr(x), isStr(y)) : (0, isNum(x), isNum(y)); return { x, y }; }
export function crossRefs(x: string | number | boolean, y: string | number | boolean, c: boolean) { c ? (0, isStr(x)) : (0, isNum(y)); return { x, y }; }
export function guardedDiff(x: string | number | boolean) { typeof x === "string" ? (0, isNum(x)) : (0, isStr(x)); return x; }
export function guardedOne(x: string | number | boolean) { typeof x === "boolean" ? (0, isStr(x)) : 0; return x; }
export function andArm(x: string | number | boolean) { typeof x === "boolean" && (0, isStr(x)); return x; }
export function orArm(x: string | number | boolean) { typeof x !== "boolean" || (0, isStr(x)); return x; }
export function nestedDiff(x: string | number | boolean, c: boolean, d: boolean) { c ? (d ? (0, isStr(x)) : (0, isNum(x))) : (0, isStr(x)); return x; }
export function inSequence(x: string | number | boolean, c: boolean) { const y = (c ? (0, isStr(x)) : (0, isNum(x)), x); return y; }
export function inInitializer(x: string | number | boolean, c: boolean) { const u = c ? (0, isStr(x)) : (0, isNum(x)); return x; }
export function ifDiff(x: string | number | boolean, c: boolean) { if (c) { isStr(x); } else { isNum(x); } return x; }
export function ifGuardedOne(x: string | number | boolean) { if (typeof x === "boolean") { isStr(x); } return x; }
export function nullableArms(x: string | null | undefined, c: boolean) { c ? (0, isStr(x)) : 0; return x; }
export function guardedInInitializer(x: string | number | boolean) { const u = typeof x === "string" ? (0, isNum(x)) : (0, isStr(x)); return x; }
export function andInInitializer(x: string | number | boolean) { const u = typeof x === "boolean" && (0, isStr(x)); return x; }
export function guardedInSequence(x: string | number | boolean) { const y = (typeof x === "string" ? (0, isNum(x)) : (0, isStr(x)), x); return y; }
"#;

/// `(symbol, strict, strictNullChecks off, noImplicitAny off, both off)`
/// for [`CONDITIONAL_JOIN_SOURCE`], each the lane's spelling of that
/// project's TypeScript 7.0.2 `.d.ts` answer.
const CONDITIONAL_JOIN_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    ("sameArms", "string", "string", "string", "string"),
    (
        "diffArms",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "overlapArms",
        "boolean | number | string",
        "boolean | number | string",
        "boolean | number | string",
        "boolean | number | string",
    ),
    (
        "oneArm",
        "boolean | number | string",
        "boolean | number | string",
        "boolean | number | string",
        "boolean | number | string",
    ),
    (
        "twoRefs",
        "{ x: number | string; y: number | string }",
        "{ x: number | string; y: number | string }",
        "{ x: number | string; y: number | string }",
        "{ x: number | string; y: number | string }",
    ),
    (
        "crossRefs",
        "{ x: boolean | number | string; y: boolean | number | string }",
        "{ x: boolean | number | string; y: boolean | number | string }",
        "{ x: boolean | number | string; y: boolean | number | string }",
        "{ x: boolean | number | string; y: boolean | number | string }",
    ),
    ("guardedDiff", "never", "never", "never", "never"),
    (
        "guardedOne",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "andArm",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "orArm",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "nestedDiff",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "inSequence",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "inInitializer",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "ifDiff",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "ifGuardedOne",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    (
        "nullableArms",
        "null | string | undefined",
        "string",
        "null | string | undefined",
        "string",
    ),
    ("guardedInInitializer", "never", "never", "never", "never"),
    (
        "andInInitializer",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    ("guardedInSequence", "never", "never", "never", "never"),
];

/// The checker's flow graph joins a conditional's arms: past it, each
/// reference reads the union of its narrowed types at the ends of the
/// arms, each arm under its reading of the test. The same targets narrow
/// to that target (`sameArms`), different ones to their union
/// (`diffArms`, `nestedDiff`, in a sequence or an initializer too), an
/// overlapping pair or an arm that asserts nothing to what their union
/// covers (`overlapArms`, `oneArm`), and each reference joins on its own
/// (`twoRefs`, `crossRefs`). The test's reading applies inside each arm
/// (`guardedDiff` is `never` in every position, `guardedOne`, `andArm` and `orArm` drop the
/// asserted-away arm), exactly as for an `if`. Each cell matches its own
/// project's TypeScript 7.0.2 answer.
#[test]
fn a_conditional_join_unions_its_arms_narrowed_types() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "conditional-join.ts",
        CONDITIONAL_JOIN_SOURCE,
        CONDITIONAL_JOIN_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Unions of object literals joined by returns, conditional expressions,
/// arrays, yields and bindings, nested literals at each depth, and a
/// declared type beside a literal.
const OBJECT_LITERAL_NORMALIZATION_SOURCE: &str = r#"
interface Generator<T, TReturn, TNext> {}
export function ternRet(c: boolean) { return c ? { a: 1 } : { a: 1, b: 2 }; }
export function ifRet(c: boolean) { if (c) return { a: 1 }; return { a: 1, b: 2 }; }
export function constDecl(c: boolean) { const x = c ? { a: 1 } : { a: 1, b: 2 }; return x; }
export function three(c: number) { return c === 0 ? { a: 1 } : c === 1 ? { b: "s" } : { c: true }; }
export function ternInTern(c: boolean, d: boolean) { return c ? { k: 1 } : d ? { a: 1 } : { a: 1, b: 2 }; }
export function arr(c: boolean) { return [{ a: 1 }, { a: 1, b: 2 }]; }
export function* gen() { yield { a: 1 }; yield { b: 2 }; }
export function nested(c: boolean) { return { o: c ? { a: 1 } : { a: 1, b: 2 } }; }
export function nestedDeep(c: boolean) { return c ? { o: { a: 1 } } : { o: { a: 1, b: 2 } }; }
export function deep3(c: boolean) { return c ? { o: { p: { a: 1 } } } : { o: { p: { a: 1, b: 2 } } }; }
export function mixedDeep(c: boolean) { return c ? { o: { a: 1 } } : { o: { b: 1 }, q: 2 }; }
export function arrDeep(c: boolean) { return [{ o: { a: 1 } }, { o: { b: 2 } }]; }
export function joinNested(c: boolean, d: boolean) { if (c) return { o: d ? { a: 1 } : { a: 1, b: 2 } }; return { o: d ? { a: 1 } : { a: 1, b: 2 } }; }
export function joinSibling(c: boolean, d: boolean) { if (c) return { o: { z: 1 } }; return { o: d ? { a: 1 } : { a: 1, b: 2 } }; }
export function depthTwo(c: boolean) { return { o: { p: c ? { a: 1 } : { a: 1, b: 2 } } }; }
export function arrayResets(c: boolean) { return { o: [c ? { a: 1 } : { a: 1, b: 2 }] }; }
export function methodArm(c: boolean) { return c ? { m() { return 1; } } : { a: 1 }; }
export function asConst(c: boolean) { return c ? ({ a: 1 } as const) : ({ a: 1, b: 2 } as const); }
export function nullArm(c: boolean) { return c ? { a: null } : { b: 1 }; }
export function vsDecl(c: boolean, d: { a: number; b: number }) { return c ? { a: 1 } : d; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`OBJECT_LITERAL_NORMALIZATION_SOURCE`], each the
/// lane's spelling of the checker's TypeScript 7.0.2 answer (read from a
/// `ReturnType<typeof f>` message; the checker's print follows each row,
/// once when the four projects agree).
const OBJECT_LITERAL_NORMALIZATION_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // { a: number; b?: undefined; } | { a: number; b: number; }
    (
        "ternRet",
        "{ a: number; b: number } | { a: number; b?: undefined }",
        "{ a: number; b: number } | { a: number; b?: undefined }",
        "{ a: number; b: number } | { a: number; b?: undefined }",
        "{ a: number; b: number } | { a: number; b?: undefined }",
    ),
    // { b?: undefined; a: number; } | { a: number; b: number; }
    (
        "ifRet",
        "{ a: number; b: number } | { a: number; b?: undefined }",
        "{ a: number; b: number } | { a: number; b?: undefined }",
        "{ a: number; b: number } | { a: number; b?: undefined }",
        "{ a: number; b: number } | { a: number; b?: undefined }",
    ),
    // { b?: undefined; a: number; } | { a: number; b: number; }
    (
        "constDecl",
        "{ a: number; b: number } | { a: number; b?: undefined }",
        "{ a: number; b: number } | { a: number; b?: undefined }",
        "{ a: number; b: number } | { a: number; b?: undefined }",
        "{ a: number; b: number } | { a: number; b?: undefined }",
    ),
    // { b?: undefined; a: number; c?: undefined; } | { a?: undefined; b: string; c?: undefined; } | { b?: undefined; a?: undefined; c: boolean; }
    (
        "three",
        "{ a: number; c?: undefined; b?: undefined } | { b: string; c?: undefined; a?: undefined } | { c: boolean; a?: undefined; b?: undefined }",
        "{ a: number; c?: undefined; b?: undefined } | { b: string; c?: undefined; a?: undefined } | { c: boolean; a?: undefined; b?: undefined }",
        "{ a: number; c?: undefined; b?: undefined } | { b: string; c?: undefined; a?: undefined } | { c: boolean; a?: undefined; b?: undefined }",
        "{ a: number; c?: undefined; b?: undefined } | { b: string; c?: undefined; a?: undefined } | { c: boolean; a?: undefined; b?: undefined }",
    ),
    // { b?: undefined; a?: undefined; k: number; } | { b?: undefined; k?: undefined; a: number; } | { k?: undefined; a: number; b: number; }
    (
        "ternInTern",
        "{ a: number; b: number; k?: undefined } | { a: number; b?: undefined; k?: undefined } | { k: number; a?: undefined; b?: undefined }",
        "{ a: number; b: number; k?: undefined } | { a: number; b?: undefined; k?: undefined } | { k: number; a?: undefined; b?: undefined }",
        "{ a: number; b: number; k?: undefined } | { a: number; b?: undefined; k?: undefined } | { k: number; a?: undefined; b?: undefined }",
        "{ a: number; b: number; k?: undefined } | { a: number; b?: undefined; k?: undefined } | { k: number; a?: undefined; b?: undefined }",
    ),
    // ({ b?: undefined; a: number; } | { a: number; b: number; })[]
    (
        "arr",
        "({ a: number; b: number } | { a: number; b?: undefined })[]",
        "({ a: number; b: number } | { a: number; b?: undefined })[]",
        "({ a: number; b: number } | { a: number; b?: undefined })[]",
        "({ a: number; b: number } | { a: number; b?: undefined })[]",
    ),
    // Generator<{ b?: undefined; a: number; } | { a?: undefined; b: number; }, void, unknown>
    (
        "gen",
        "Generator<{ a: number; b?: undefined } | { b: number; a?: undefined }, void, unknown>",
        "Generator<{ a: number; b?: undefined } | { b: number; a?: undefined }, void, unknown>",
        "Generator<{ a: number; b?: undefined } | { b: number; a?: undefined }, void, unknown>",
        "Generator<{ a: number; b?: undefined } | { b: number; a?: undefined }, void, unknown>",
    ),
    // { o: { a: number; b?: undefined; } | { a: number; b: number; }; }
    (
        "nested",
        "{ o: { a: number; b: number } | { a: number; b?: undefined } }",
        "{ o: { a: number; b: number } | { a: number; b?: undefined } }",
        "{ o: { a: number; b: number } | { a: number; b?: undefined } }",
        "{ o: { a: number; b: number } | { a: number; b?: undefined } }",
    ),
    // { o: { b?: undefined; a: number; }; } | { o: { a: number; b: number; }; }
    (
        "nestedDeep",
        "{ o: { a: number; b: number } } | { o: { a: number; b?: undefined } }",
        "{ o: { a: number; b: number } } | { o: { a: number; b?: undefined } }",
        "{ o: { a: number; b: number } } | { o: { a: number; b?: undefined } }",
        "{ o: { a: number; b: number } } | { o: { a: number; b?: undefined } }",
    ),
    // { o: { p: { b?: undefined; a: number; }; }; } | { o: { p: { a: number; b: number; }; }; }
    (
        "deep3",
        "{ o: { p: { a: number; b: number } } } | { o: { p: { a: number; b?: undefined } } }",
        "{ o: { p: { a: number; b: number } } } | { o: { p: { a: number; b?: undefined } } }",
        "{ o: { p: { a: number; b: number } } } | { o: { p: { a: number; b?: undefined } } }",
        "{ o: { p: { a: number; b: number } } } | { o: { p: { a: number; b?: undefined } } }",
    ),
    // { o: { b?: undefined; a: number; }; q?: undefined; } | { o: { a?: undefined; b: number; }; q: number; }
    (
        "mixedDeep",
        "{ o: { a: number; b?: undefined }; q?: undefined } | { o: { b: number; a?: undefined }; q: number }",
        "{ o: { a: number; b?: undefined }; q?: undefined } | { o: { b: number; a?: undefined }; q: number }",
        "{ o: { a: number; b?: undefined }; q?: undefined } | { o: { b: number; a?: undefined }; q: number }",
        "{ o: { a: number; b?: undefined }; q?: undefined } | { o: { b: number; a?: undefined }; q: number }",
    ),
    // ({ o: { b?: undefined; a: number; }; } | { o: { a?: undefined; b: number; }; })[]
    (
        "arrDeep",
        "({ o: { a: number; b?: undefined } } | { o: { b: number; a?: undefined } })[]",
        "({ o: { a: number; b?: undefined } } | { o: { b: number; a?: undefined } })[]",
        "({ o: { a: number; b?: undefined } } | { o: { b: number; a?: undefined } })[]",
        "({ o: { a: number; b?: undefined } } | { o: { b: number; a?: undefined } })[]",
    ),
    // { o: { b?: undefined; a: number; } | { a: number; b: number; }; }
    (
        "joinNested",
        "{ o: { a: number; b: number } | { a: number; b?: undefined } }",
        "{ o: { a: number; b: number } | { a: number; b?: undefined } }",
        "{ o: { a: number; b: number } | { a: number; b?: undefined } }",
        "{ o: { a: number; b: number } | { a: number; b?: undefined } }",
    ),
    // { o: { b?: undefined; z: number; a?: undefined; }; } | { o: { b?: undefined; z?: undefined; a: number; } | { z?: undefined; a: number; b: number; }; }
    (
        "joinSibling",
        "{ o: { a: number; b: number; z?: undefined } | { a: number; z?: undefined; b?: undefined } } | { o: { z: number; a?: undefined; b?: undefined } }",
        "{ o: { a: number; b: number; z?: undefined } | { a: number; z?: undefined; b?: undefined } } | { o: { z: number; a?: undefined; b?: undefined } }",
        "{ o: { a: number; b: number; z?: undefined } | { a: number; z?: undefined; b?: undefined } } | { o: { z: number; a?: undefined; b?: undefined } }",
        "{ o: { a: number; b: number; z?: undefined } | { a: number; z?: undefined; b?: undefined } } | { o: { z: number; a?: undefined; b?: undefined } }",
    ),
    // { o: { p: { a: number; } | { a: number; b: number; }; }; }
    (
        "depthTwo",
        "{ o: { p: { a: number } | { a: number; b: number } } }",
        "{ o: { p: { a: number } | { a: number; b: number } } }",
        "{ o: { p: { a: number } | { a: number; b: number } } }",
        "{ o: { p: { a: number } | { a: number; b: number } } }",
    ),
    // { o: ({ b?: undefined; a: number; } | { a: number; b: number; })[]; }
    (
        "arrayResets",
        "{ o: ({ a: number; b: number } | { a: number; b?: undefined })[] }",
        "{ o: ({ a: number; b: number } | { a: number; b?: undefined })[] }",
        "{ o: ({ a: number; b: number } | { a: number; b?: undefined })[] }",
        "{ o: ({ a: number; b: number } | { a: number; b?: undefined })[] }",
    ),
    // { a?: undefined; m(): number; } | { m?: undefined; a: number; }
    (
        "methodArm",
        "{ a: number; m?: undefined } | { m: () => number; a?: undefined }",
        "{ a: number; m?: undefined } | { m: () => number; a?: undefined }",
        "{ a: number; m?: undefined } | { m: () => number; a?: undefined }",
        "{ a: number; m?: undefined } | { m: () => number; a?: undefined }",
    ),
    // { b?: undefined; readonly a: 1; } | { readonly a: 1; readonly b: 2; }
    (
        "asConst",
        "{ readonly a: 1; b?: undefined } | { readonly a: 1; readonly b: 2 }",
        "{ readonly a: 1; b?: undefined } | { readonly a: 1; readonly b: 2 }",
        "{ readonly a: 1; b?: undefined } | { readonly a: 1; readonly b: 2 }",
        "{ readonly a: 1; b?: undefined } | { readonly a: 1; readonly b: 2 }",
    ),
    // { a: null; b?: undefined; } | { a?: undefined; b: number; } / { a: any; b?: undefined; } | { a?: undefined; b: number; } (twice each)
    (
        "nullArm",
        "{ a: null; b?: undefined } | { b: number; a?: undefined }",
        "{ a: any; b?: undefined } | { b: number; a?: undefined }",
        "{ a: null; b?: undefined } | { b: number; a?: undefined }",
        "{ a: any; b?: undefined } | { b: number; a?: undefined }",
    ),
    // { a: number; b: number; } | { a: number; }
    (
        "vsDecl",
        "{ a: number } | { a: number; b: number }",
        "{ a: number } | { a: number; b: number }",
        "{ a: number } | { a: number; b: number }",
        "{ a: number } | { a: number; b: number }",
    ),
];

/// The checker widens the value a return, a yield, a binding and an
/// array literal hold, and widening a union of object literals gives each
/// literal the properties its object-literal siblings name and it lacks,
/// as optional `undefined` members (`getWidenedTypeWithContext`): `c ? {
/// a: 1 } : { a: 1, b: 2 }` is `{ a: number; b?: undefined } | { a:
/// number; b: number }`. A literal widened in such a context widens each
/// property in the context of the same property of its siblings, so the
/// normalisation reaches nested literals (`nestedDeep`, `deep3`,
/// `mixedDeep`, `arrDeep`, `joinSibling`); a lone literal normalises a
/// union-valued property (`nested`, `joinNested`) but, on TypeScript
/// 7.0.2, not a union two literal properties down (`depthTwo`), and an
/// array widens its element as a whole (`arrayResets`). A declared object
/// type neither names nor receives a property (`vsDecl`), and the added
/// member is `undefined` without `strictNullChecks` too (`nullArm`). The
/// nested literals relate by the checker's subtype rule for an object
/// literal target (a source property the literal lacks is not below it
/// unless `undefined`), so `{ o: { a: 1, b: 2 } }` is not absorbed into
/// `{ o: { a: 1 } }`. The rows spell members in the lane's order (own
/// members, then the added ones in sibling order); the checker's print
/// order is its property table's, which is not stable even across one
/// construction (`ternRet` and `ifRet` print `b?` last and first). Each
/// cell matches its own project's TypeScript 7.0.2 answer.
#[test]
fn widened_object_literal_unions_take_their_siblings_properties() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "object-literal-normalization.ts",
        OBJECT_LITERAL_NORMALIZATION_SOURCE,
        OBJECT_LITERAL_NORMALIZATION_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// The normalised join and the declared union the checker prints for it
/// are one type: each is assignable to the other through the relation
/// authority, in every project. A spread-built literal receives the
/// properties it lacks too (`c ? { a: 1, q: 1 } : { ...s, a: 2 }` gives
/// the spread arm `q?: undefined` on TypeScript 7.0.2).
#[test]
fn a_normalized_object_literal_join_is_the_checkers_type() {
    const SOURCE: &str = r#"
export function ternRet(c: boolean) { return c ? { a: 1 } : { a: 1, b: 2 }; }
export function declared(v: { b?: undefined; a: number } | { a: number; b: number }) { return v; }
export function spreadArm(c: boolean, s: { z: number }) { return c ? { a: 1, q: 1 } : { ...s, a: 2 }; }
"#;
    let host = four_policy_host();
    for root in [
        STRICT_ROOT,
        LOOSE_ROOT,
        STRICT_IMPLICIT_ROOT,
        LOOSE_IMPLICIT_ROOT,
    ] {
        let canonical = format!("{root}/normalized-join.ts");
        upsert(&host, &canonical, SOURCE);
        let node_of = |symbol: &str| {
            host.get_flow_return_type_with_audit(
                &identity(&canonical, symbol),
                ReturnProjectionDemand::whole_return(),
            )
            .as_result()
            .map(|result| result.return_type())
            .unwrap_or_else(|_| panic!("`{symbol}` answers a value"))
        };
        let join = node_of("ternRet");
        let declared = node_of("declared");
        let spread = node_of("spreadArm");
        let store_view = host.resolver_store_view_read().into_owned_view();
        let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
        let host_ctx = crate::resolver_core::HostResolverContext::new(&host, &store_view, overlay);
        let dispatch = ProjectSemanticDispatch::new(&host_ctx);
        for (source, target) in [(join, declared), (declared, join)] {
            assert!(
                matches!(
                    dispatch.execute_relate_pair(source, target),
                    super::dispatch_txn::RelationStep::Assignable { .. }
                ),
                "{root}: the normalised join and the checker's declared union are assignable \
                 each to the other"
            );
        }
        let graph = dispatch.graph();
        let Some(SemanticNodeData::Union(arms)) = graph.node_data(spread).as_deref().cloned()
        else {
            panic!("{root}: the spread join keeps both arms");
        };
        let program = arms
            .iter()
            .copied()
            .find(|arm| {
                matches!(
                    graph.node_data(*arm).as_deref(),
                    Some(SemanticNodeData::ObjectSpreadProgram(_))
                )
            })
            .unwrap_or_else(|| panic!("{root}: one arm is the spread-built literal"));
        let surface = dispatch
            .resolve_typeinfo_surface_view(
                program,
                crate::semantic_query::ProjectionReductionContext::structural_transit(),
            )
            .unwrap_or_else(|| panic!("{root}: the spread arm has a surface"));
        let q = surface
            .positive_members()
            .iter()
            .find(|member| member.key.as_string() == Some("q"))
            .unwrap_or_else(|| panic!("{root}: the spread arm receives `q`"));
        assert!(q.optional, "{root}: the received `q` is optional");
        assert!(
            matches!(
                graph.node_data(q.value).as_deref(),
                Some(SemanticNodeData::Primitive(PrimitiveKind::Undefined))
            ),
            "{root}: the received `q` is `undefined`"
        );
    }
}

/// Values a generic or overloaded call consumes, computed in the frame:
/// conditional arguments over parameters, literals and calls, calls of a
/// flow-inferred function passed to an overload set and a generic
/// callee, member reads of such calls, and `in` tests of their values.
const CALL_CONSUMER_SOURCE: &str = r#"
declare function id<T>(v: T): T;
declare function first<T>(xs: T[]): T;
declare function pick(v: { a: string }): "s";
declare function pick(v: { a: number }): "n";
declare function pick(v: unknown): "u";
export function twin(c: boolean) { if (c) return { a: null }; return { a: undefined }; }
export function one(c: boolean) { if (c) return { a: 1 }; return { a: 2 }; }
function local(c: boolean) { if (c) return { a: null }; return { a: undefined }; }
export function ternArg(c: boolean, a: { x: string }, b: { x: string; y: number }) { return id(c ? a : b); }
export function ternArgRev(c: boolean, a: { x: string; y: number }, b: { x: string }) { return id(c ? a : b); }
export function ternArgLit(c: boolean) { return id(c ? "a" : "b"); }
export function ternArgNull(c: boolean, a: { x: string }) { return id(c ? a : null); }
export function ternArgUnrelated(c: boolean, a: { x: string }, b: { y: number }) { return id(c ? a : b); }
export function ternArgObjLit(c: boolean) { return id(c ? { a: 1 } : { a: 1, b: 2 }); }
export function ternArgSameShape(c: boolean) { return id(c ? { a: 1 } : { a: 2 }); }
export function arrArgLit() { return id([{ a: 1 }, { a: 1, b: 2 }]); }
export function memberRead(c: boolean) { return twin(c).a; }
export function memberReadOne(c: boolean) { return one(c).a; }
export function memberReadLocal(c: boolean) { return local(c).a; }
export function viaPick(c: boolean) { return pick(twin(c)); }
export function viaPickOne(c: boolean) { return pick(one(c)); }
export function viaFirst(c: boolean) { return first([twin(c)]); }
export function viaFirstOne(c: boolean) { return first([one(c)]); }
export function viaIn(c: boolean) { const v = twin(c); if ("a" in v) return v; return 0; }
export function viaInOne(c: boolean) { const v = one(c); if ("a" in v) return v; return 0; }
export function viaInLocal(c: boolean) { const v = local(c); if ("a" in v) return v; return 0; }
export function viaInDirectLocal(c: boolean) { if ("a" in local(c)) return 1; return "x"; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`CALL_CONSUMER_SOURCE`], each the lane's spelling
/// of the checker's TypeScript 7.0.2 answer (the checker's print follows
/// each row, once when the four projects agree).
const CALL_CONSUMER_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // { x: string; }
    (
        "ternArg",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
    ),
    // { x: string; }
    (
        "ternArgRev",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
        "{ x: string }",
    ),
    // "a" | "b"
    (
        "ternArgLit",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // { x: string; } | null / { x: string; } (twice each)
    (
        "ternArgNull",
        "null | { x: string }",
        "{ x: string }",
        "null | { x: string }",
        "{ x: string }",
    ),
    // { x: string; } | { y: number; }
    (
        "ternArgUnrelated",
        "{ x: string } | { y: number }",
        "{ x: string } | { y: number }",
        "{ x: string } | { y: number }",
        "{ x: string } | { y: number }",
    ),
    // { a: number; b?: undefined; } | { a: number; b: number; }
    (
        "ternArgObjLit",
        "{ a: number; b: number } | { a: number; b?: undefined }",
        "{ a: number; b: number } | { a: number; b?: undefined }",
        "{ a: number; b: number } | { a: number; b?: undefined }",
        "{ a: number; b: number } | { a: number; b?: undefined }",
    ),
    // { a: number; }
    (
        "ternArgSameShape",
        "{ a: number }",
        "{ a: number }",
        "{ a: number }",
        "{ a: number }",
    ),
    // ({ b?: undefined; a: number; } | { a: number; b: number; })[]
    (
        "arrArgLit",
        "({ a: number; b: number } | { a: number; b?: undefined })[]",
        "({ a: number; b: number } | { a: number; b?: undefined })[]",
        "({ a: number; b: number } | { a: number; b?: undefined })[]",
        "({ a: number; b: number } | { a: number; b?: undefined })[]",
    ),
    // null | undefined / any (twice each)
    (
        "memberRead",
        "null | undefined",
        "any",
        "null | undefined",
        "any",
    ),
    // number
    ("memberReadOne", "number", "number", "number", "number"),
    // null | undefined / any (twice each)
    (
        "memberReadLocal",
        "null | undefined",
        "any",
        "null | undefined",
        "any",
    ),
    // "u"
    ("viaPick", "\"u\"", "\"u\"", "\"u\"", "\"u\""),
    // "n"
    ("viaPickOne", "\"n\"", "\"n\"", "\"n\"", "\"n\""),
    // { a: null; } | { a: undefined; } / { a: any; } | { a: any; } (twice each)
    (
        "viaFirst",
        "{ a: null } | { a: undefined }",
        "{ a: any }",
        "{ a: null } | { a: undefined }",
        "{ a: any }",
    ),
    // { a: number; }
    (
        "viaFirstOne",
        "{ a: number }",
        "{ a: number }",
        "{ a: number }",
        "{ a: number }",
    ),
    // 0 | { a: null; } | { a: undefined; } / 0 | { a: any; } (twice each)
    (
        "viaIn",
        "0 | { a: null } | { a: undefined }",
        "0 | { a: any }",
        "0 | { a: null } | { a: undefined }",
        "0 | { a: any }",
    ),
    // 0 | { a: number; }
    (
        "viaInOne",
        "0 | { a: number }",
        "0 | { a: number }",
        "0 | { a: number }",
        "0 | { a: number }",
    ),
    // 0 | { a: null; } | { a: undefined; } / 0 | { a: any; } (twice each)
    (
        "viaInLocal",
        "0 | { a: null } | { a: undefined }",
        "0 | { a: any }",
        "0 | { a: null } | { a: undefined }",
        "0 | { a: any }",
    ),
    // "x" | 1
    (
        "viaInDirectLocal",
        "\"x\" | 1",
        "\"x\" | 1",
        "\"x\" | 1",
        "\"x\" | 1",
    ),
];

/// A call's arguments are values of the calling frame: `id(c ? a : b)`
/// over `declare function id<T>(v: T): T` infers `T` from the
/// subtype-reduced conditional (`{ x: string }`), a flow-inferred call
/// passes its return (`pick(twin(c))`, `first([twin(c)])`), and the
/// conditional's arms are each a fresh literal (`id(c ? { a: 1 } : { a:
/// 2 })` is `{ a: number }`). An overloaded callee is tried under the
/// SUBTYPE relation before assignability, as the checker's `resolveCall`
/// does: `twin(c)` is `{ a: any }` without `strictNullChecks`, which is
/// only assignable to `(v: { a: string })`, so `pick` selects `(v:
/// unknown)` and answers `"u"`. A static member read off a call's value
/// reads the member of its return (`twin(c).a`), and `"a" in v` narrows
/// the call's value held by a `const` — the checker narrows only the
/// reference an `in` test names, so the local initialised from a call
/// carries no fact the test leaves unmentioned — and an `in` operand that
/// is a call of a closed same-file function is no predicate position.
/// Every row is clean; each cell matches its own project's TypeScript
/// 7.0.2 answer (`viaFirst` without `strictNullChecks` prints `{ a: any;
/// } | { a: any; }` on the checker, the structural-twin presentation
/// `an_object_join_of_structural_twins_differs_only_in_presentation`
/// pins, and is the one object `{ a: any }` here).
#[test]
fn call_consumers_read_the_frames_values() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "call-consumers.ts",
        CALL_CONSUMER_SOURCE,
        CALL_CONSUMER_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

const OPERATOR_SOURCE: &str = r#"
export function loopCounter(c: boolean) { let i = 0; while (c) { const n = i; i = n + 1; } return i; }
export function addLocal(a: number) { const n = a; return n + 1; }
export function addParam(a: number) { return a + 1; }
export function addStrParam(a: string) { return a + 1; }
export function addStrLocal(a: string) { const s = a; return s + 1; }
export function addMixed(a: number, b: string) { return a + b; }
export function addLits(a: 1 | 2) { return a + a; }
export function addBig(a: bigint) { return a + 1n; }
export function subParam(a: number) { return a - 1; }
export function subAny(a: any) { return a - 1; }
export function addAny(a: any) { return a + 1; }
export function addAnyStr(a: any) { return a + "s"; }
export function addAnyAny(a: any, b: any) { return a + b; }
export function mulBig(a: bigint, b: bigint) { return a * b; }
export function subUnion(a: number | bigint) { return -a; }
export function negParam(a: number) { return -a; }
export function negBig(a: bigint) { return -a; }
export function plusStr(a: string) { return +a; }
export function bitNot(a: number) { return ~a; }
export function postInc(c: boolean) { let i = 0; if (c) i++; return i; }
export function postIncVal() { let i = 0; return i++; }
export function preIncVal() { let i = 0; return ++i; }
export function incBig(b: bigint) { let i = b; return i++; }
export function compound(a: number) { let i = a; i += 1; return i; }
export function elemParamIdx(xs: string[], i: number) { return xs[i]; }
export function elemLocalIdx(xs: string[]) { let i = 0; return xs[i]; }
export function elemLitIdx(xs: string[]) { return xs[0]; }
export function elemTupleLit(t: [number, string]) { return t[1]; }
export function elemTupleIdx(t: [number, string], i: number) { return t[i]; }
export function elemTupleLocalLit(t: [number, string]) { const i = 1; return t[i]; }
export function elemLoop(xs: string[]) { for (let i = 0; i < xs.length; i++) { if (xs[i]) return xs[i]; } return 0; }
export function obj(a: number) { const n = a; return { v: n * 2 }; }
export function ltCompare(a: number) { const n = a; return n < 1; }
export function modLoop(c: boolean) { let i = 0; while (c) { i = (i + 1) % 3; } return i; }
export function strLoop(c: boolean) { let s = ""; while (c) { s = s + "x"; } return s; }
export function tmpl(a: number) { const n = a; return `${n + 1}`; }
export function enumAdd(a: E) { return a + 1; }
export enum E { A, B }
export function strEnumAdd(a: S) { return a + 1; }
export enum S { A = "a" }
export function nullAdd(a: number | null) { return a! + 1; }
export function undefAdd(a: number | undefined) { return (a ?? 0) + 1; }
export function nonNullAlone(a: number | null) { return a!; }
export function anyIdx(xs: string[], i: any) { return xs[i]; }
export function strIdx(s: string, i: number) { return s[i]; }
export function objAnyIdx(o: any, k: string) { return o[k]; }
export function unionArrIdx(xs: string[] | number[], i: number) { return xs[i]; }
export function tupleOptIdx(t: [number, string?], i: number) { return t[i]; }
export function tupleRestIdx(t: [number, ...string[]]) { return t[1]; }
export function negLit() { return -1; }
export function negLocalLit() { const n = 1; return -n; }
export function addConstLits() { const a = 1; const b = 2; return a + b; }
export function strConcatLit() { const a = "x"; return a + "y"; }
export function templateAdd(a: `x${string}`) { return a + 1; }
export function unknownAdd(a: unknown, b: unknown) { return (a as number) + (b as number); }
export function typeParamAdd<T extends number>(a: T) { return a + 1; }
export function typeParamBig<T extends bigint>(a: T) { return a * a; }
export function typeParamNeg<T extends number | bigint>(a: T) { return -a; }
export function numLitUnionNeg(a: 1 | 2) { return -a; }
export function updateValueOld() { let i: number | string = 0; const j = i++; return [i, j]; }
class C { m() { return 1; } p = "s"; static make() { return new C(); } }
class G<T> { constructor(public v: T) {} get() { return this.v; } }
const cc = 5;
export function newCall() { return new C().m(); }
export function newProp() { return new C().p; }
export function newGenericProp() { return new G(1).v; }
export function notZero() { const v = !0; return v; }
export function notZeroRet() { return !0; }
export function andLits() { const v = true && 1; return v; }
export function orLits() { const v = false || "s"; return v; }
export function nonNullLit() { const v = 1!; return v; }
export function letNot() { let v = !1; return v; }
export function captureAdd() { return cc + 1; }
export function paramAdd(c: number) { return c + 1; }
export function fnVarDestr(t: [number, string]) { var [a, b] = t; return b; }
export function fnVarDestrObj(o: { x: number }) { var { x } = o; return x; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`OPERATOR_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const OPERATOR_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number
    ("loopCounter", "number", "number", "number", "number"),
    // number
    ("addLocal", "number", "number", "number", "number"),
    // number
    ("addParam", "number", "number", "number", "number"),
    // string
    ("addStrParam", "string", "string", "string", "string"),
    // string
    ("addStrLocal", "string", "string", "string", "string"),
    // string
    ("addMixed", "string", "string", "string", "string"),
    // number
    ("addLits", "number", "number", "number", "number"),
    // bigint
    ("addBig", "bigint", "bigint", "bigint", "bigint"),
    // number
    ("subParam", "number", "number", "number", "number"),
    // number
    ("subAny", "number", "number", "number", "number"),
    // any
    ("addAny", "any", "any", "any", "any"),
    // string
    ("addAnyStr", "string", "string", "string", "string"),
    // any
    ("addAnyAny", "any", "any", "any", "any"),
    // bigint
    ("mulBig", "bigint", "bigint", "bigint", "bigint"),
    // number | bigint
    (
        "subUnion",
        "bigint | number",
        "bigint | number",
        "bigint | number",
        "bigint | number",
    ),
    // number
    ("negParam", "number", "number", "number", "number"),
    // bigint
    ("negBig", "bigint", "bigint", "bigint", "bigint"),
    // number
    ("plusStr", "number", "number", "number", "number"),
    // number
    ("bitNot", "number", "number", "number", "number"),
    // number
    ("postInc", "number", "number", "number", "number"),
    // number
    ("postIncVal", "number", "number", "number", "number"),
    // number
    ("preIncVal", "number", "number", "number", "number"),
    // bigint
    ("incBig", "bigint", "bigint", "bigint", "bigint"),
    // number
    ("compound", "number", "number", "number", "number"),
    // string
    ("elemParamIdx", "string", "string", "string", "string"),
    // string
    ("elemLocalIdx", "string", "string", "string", "string"),
    // string
    ("elemLitIdx", "string", "string", "string", "string"),
    // string
    ("elemTupleLit", "string", "string", "string", "string"),
    // string | number
    (
        "elemTupleIdx",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string
    ("elemTupleLocalLit", "string", "string", "string", "string"),
    // string | 0
    (
        "elemLoop",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // { v: number; }
    (
        "obj",
        "{ v: number }",
        "{ v: number }",
        "{ v: number }",
        "{ v: number }",
    ),
    // boolean
    ("ltCompare", "boolean", "boolean", "boolean", "boolean"),
    // number
    ("modLoop", "number", "number", "number", "number"),
    // string
    ("strLoop", "string", "string", "string", "string"),
    // string
    ("tmpl", "string", "string", "string", "string"),
    // number
    ("enumAdd", "number", "number", "number", "number"),
    // string
    ("strEnumAdd", "string", "string", "string", "string"),
    // number
    ("nullAdd", "number", "number", "number", "number"),
    // number
    ("undefAdd", "number", "number", "number", "number"),
    // number
    ("nonNullAlone", "number", "number", "number", "number"),
    // string
    ("anyIdx", "string", "string", "string", "string"),
    // string
    ("strIdx", "string", "string", "string", "string"),
    // any
    ("objAnyIdx", "any", "any", "any", "any"),
    // string | number
    (
        "unionArrIdx",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number | undefined / string | number / string | number | undefined / string | number
    (
        "tupleOptIdx",
        "number | string | undefined",
        "number | string",
        "number | string | undefined",
        "number | string",
    ),
    // string
    ("tupleRestIdx", "string", "string", "string", "string"),
    // number
    ("negLit", "number", "number", "number", "number"),
    // number
    ("negLocalLit", "number", "number", "number", "number"),
    // number
    ("addConstLits", "number", "number", "number", "number"),
    // string
    ("strConcatLit", "string", "string", "string", "string"),
    // string
    ("templateAdd", "string", "string", "string", "string"),
    // number
    ("unknownAdd", "number", "number", "number", "number"),
    // number
    ("typeParamAdd", "number", "number", "number", "number"),
    // number
    ("typeParamBig", "number", "number", "number", "number"),
    // number
    ("typeParamNeg", "number", "number", "number", "number"),
    // number
    ("numLitUnionNeg", "number", "number", "number", "number"),
    // number[]
    (
        "updateValueOld",
        "number[]",
        "number[]",
        "number[]",
        "number[]",
    ),
    // number
    ("newCall", "number", "number", "number", "number"),
    // string
    ("newProp", "string", "string", "string", "string"),
    // number
    ("newGenericProp", "number", "number", "number", "number"),
    // boolean
    ("notZero", "boolean", "boolean", "boolean", "boolean"),
    // boolean
    ("notZeroRet", "boolean", "boolean", "boolean", "boolean"),
    // number / 0 | 1 / number / 0 | 1
    ("andLits", "number", "0 | 1", "number", "0 | 1"),
    // string
    ("orLits", "string", "string", "string", "string"),
    // number
    ("nonNullLit", "number", "number", "number", "number"),
    // boolean
    ("letNot", "boolean", "boolean", "boolean", "boolean"),
    // number
    ("captureAdd", "number", "number", "number", "number"),
    // number
    ("paramAdd", "number", "number", "number", "number"),
    // string
    ("fnVarDestr", "string", "string", "string", "string"),
    // number
    ("fnVarDestrObj", "number", "number", "number", "number"),
];

const DESTRUCTURE_SOURCE: &str = r#"
export function forOfArr(xs: [number, string][]) { for (const [a, b] of xs) { if (a) return b; } return 0; }
export function forOfArrA(xs: [number, string][]) { for (const [a] of xs) { return a; } return "z"; }
export function forOfObj(xs: { a: number; b: string }[]) { for (const { a, b } of xs) { if (a) return b; } return 0; }
export function forOfObjRename(xs: { a: number; b: string }[]) { for (const { b: c } of xs) { return c; } return 0; }
export function forOfDefault(xs: { a?: number }[]) { for (const { a = "d" } of xs) { return a; } return true; }
export function forOfRest(xs: [number, string, boolean][]) { for (const [a, ...rest] of xs) { return rest; } return 0; }
export function forOfObjRest(xs: { a: number; b: string; c: boolean }[]) { for (const { a, ...rest } of xs) { return rest; } return 0; }
export function forOfNested(xs: { p: [number, { q: string }] }[]) { for (const { p: [n, { q }] } of xs) { return q; } return 0; }
export function forOfLet(xs: [number, string][]) { for (let [a, b] of xs) { a = 2; return a; } return "z"; }
export function forOfHole(xs: [number, string][]) { for (const [, b] of xs) { return b; } return 0; }
export function constArr(t: [number, string]) { const [a, b] = t; return b; }
export function constObj(o: { a: number; b: string }) { const { a, b } = o; return a; }
export function constDefault(o: { a?: number }) { const { a = "d" } = o; return a; }
export function constDefaultUndef(o: { a: number | undefined }) { const { a = 1 } = o; return a; }
export function constArrDefault(t: [number?]) { const [a = "d"] = t; return a; }
export function constRestArr(t: [number, string, boolean]) { const [a, ...r] = t; return r; }
export function constRestArrOfArr(t: string[]) { const [a, ...r] = t; return r; }
export function constRestObj(o: { a: number; b: string; c?: boolean }) { const { a, ...r } = o; return r; }
export function constNested(o: { p: { q: [string, number] } }) { const { p: { q: [s] } } = o; return s; }
export function constArrOfArr(xs: string[]) { const [a, b] = xs; return a; }
export function letNarrow(o: { a: string | number }) { let { a } = o; if (typeof a === "string") return a; return 0; }
export function letAssign(o: { a: string | number }) { let { a } = o; a = 1; return a; }
export function paramDestr({ a, b }: { a: number; b: string }) { return b; }
export function paramArrDestr([a, b]: [number, string]) { return b; }
export function constStrLit() { const { a } = { a: "x" }; return a; }
export function constArrLit() { const [a, b] = [1, "s"]; return b; }
export function constArrLitConst() { const [a, b] = [1, "s"] as const; return b; }
export function constObjLitFresh() { const { a } = { a: "x" as const }; return a; }
export function computedKey(o: { a: number; b: string }) { const k = "b"; const { [k]: v } = o; return v; }
export function strDestr(s: string) { const [c] = s; return c; }
export function anyDestr(x: any) { const { a } = x; return a; }
export function anyArrDestr(x: any) { const [a] = x; return a; }
export function assignDestr(o: { a: number }) { let a: number | string = "s"; ({ a } = o); return a; }
export function assignArrDestr(t: [number]) { let a: number | string = "s"; [a] = t; return a; }
export function letConstLit() { let { a } = { a: "x" as const }; return a; }
export function letConstLitArr() { let [a] = ["x"] as const; return a; }
export function letDefaultLit(o: { a?: number }) { let { a = "d" } = o; return a; }
export function constDefaultLitArr(o: { a?: number }) { const { a = "d" } = o; return [a]; }
export function letDefaultRet(o: { a?: number }) { let { a = "d" } = o; return [a]; }
export function arrLitTuple() { const [a, b] = [1, "s"]; return [a, b]; }
export function arrLitTupleConst() { const [a, b] = [1, "s"] as const; return [a, b]; }
export function arrDefaultUndef(t: [number | undefined]) { const [a = "d"] = t; return a; }
export function objDefaultUndefUnion(o: { a: number | undefined }) { const { a = 5 } = o; return a; }
export function nestedDefault(o: { p?: { q: string } }) { const { p: { q } = { q: "z" } } = o; return q; }
export function arrRestRo(t: readonly string[]) { const [a, ...r] = t; return r; }
export function tupleOptRest(t: [number, string?, ...boolean[]]) { const [a, ...r] = t; return r; }
export function objRestRo(o: { readonly a: number; readonly b: string }) { const { a, ...r } = o; return r; }
export function objRestUnion(o: { a: 1; b: string } | { a: 2; c: number }) { const { a, ...r } = o; return r; }
export function objIndexSig(o: Record<string, number>) { const { a } = o; return a; }
export function objNumericKey(o: { 0: string }) { const { 0: z } = o; return z; }
export function objStringKey(o: { "a-b": string }) { const { "a-b": z } = o; return z; }
export function letNarrowLater(o: { a: string | number }) { let { a } = o; if (typeof a === "string") { return a; } return a; }
export function paramArrNested([a, [b]]: [number, [string]]) { return b; }
export function paramObjNested({ p: { q } }: { p: { q: string } }) { return q; }
export function paramObjRest({ a, ...r }: { a: number; b: string }) { return r; }
export function paramArrRest([a, ...r]: [number, string, boolean]) { return r; }
export function forOfLetReassign(xs: [number, string][]) { for (let [a, b] of xs) { b = "z"; return b; } return 0; }
export function destrAssignObj(o: { a: string; b: number }) { let a: string | number = 1, b: string | number = "x"; ({ a, b } = o); return [a, b]; }
export function destrAssignDefault(t: [string?]) { let a: string | number = 1; [a = 5] = t; return a; }
var [ta, tb] = [1, "s"] as const;
var { tx } = { tx: 2 };
var [tc, td]: [number, string] = [1, "s"];
const [ca, cb] = [1, "s"];
let { la = "d" } = { la: 1 as number | undefined };
const { ob: { oc } } = { ob: { oc: true } };
export function topVarA() { return ta; }
export function topVarB() { return tb; }
export function topVarX() { return tx; }
export function topVarAnn() { return td; }
export function topConstArr() { return cb; }
export function topDefault() { return la; }
export function topNested() { return oc; }
const { cl } = { cl: "x" as const };
let [lz = "d"] = [] as (number | undefined)[];
export function topConstLit() { return cl; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`DESTRUCTURE_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const DESTRUCTURE_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string | 0
    (
        "forOfArr",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // number | "z"
    (
        "forOfArrA",
        "\"z\" | number",
        "\"z\" | number",
        "\"z\" | number",
        "\"z\" | number",
    ),
    // string | 0
    (
        "forOfObj",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // string | 0
    (
        "forOfObjRename",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // number | "d" | true
    (
        "forOfDefault",
        "\"d\" | number | true",
        "\"d\" | number | true",
        "\"d\" | number | true",
        "\"d\" | number | true",
    ),
    // 0 | [string, boolean]
    (
        "forOfRest",
        "0 | [string, boolean]",
        "0 | [string, boolean]",
        "0 | [string, boolean]",
        "0 | [string, boolean]",
    ),
    // 0 | { b: string; c: boolean; }
    (
        "forOfObjRest",
        "0 | { b: string; c: boolean }",
        "0 | { b: string; c: boolean }",
        "0 | { b: string; c: boolean }",
        "0 | { b: string; c: boolean }",
    ),
    // string | 0
    (
        "forOfNested",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // number | "z"
    (
        "forOfLet",
        "\"z\" | number",
        "\"z\" | number",
        "\"z\" | number",
        "\"z\" | number",
    ),
    // string | 0
    (
        "forOfHole",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // string
    ("constArr", "string", "string", "string", "string"),
    // number
    ("constObj", "number", "number", "number", "number"),
    // number | "d"
    (
        "constDefault",
        "\"d\" | number",
        "\"d\" | number",
        "\"d\" | number",
        "\"d\" | number",
    ),
    // number
    ("constDefaultUndef", "number", "number", "number", "number"),
    // number | "d"
    (
        "constArrDefault",
        "\"d\" | number",
        "\"d\" | number",
        "\"d\" | number",
        "\"d\" | number",
    ),
    // [string, boolean]
    (
        "constRestArr",
        "[string, boolean]",
        "[string, boolean]",
        "[string, boolean]",
        "[string, boolean]",
    ),
    // string[]
    (
        "constRestArrOfArr",
        "string[]",
        "string[]",
        "string[]",
        "string[]",
    ),
    // { b: string; c?: boolean; }
    (
        "constRestObj",
        "{ b: string; c?: boolean }",
        "{ b: string; c?: boolean }",
        "{ b: string; c?: boolean }",
        "{ b: string; c?: boolean }",
    ),
    // string
    ("constNested", "string", "string", "string", "string"),
    // string
    ("constArrOfArr", "string", "string", "string", "string"),
    // string | 0
    (
        "letNarrow",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // number
    ("letAssign", "number", "number", "number", "number"),
    // string
    ("paramDestr", "string", "string", "string", "string"),
    // string
    ("paramArrDestr", "string", "string", "string", "string"),
    // string
    ("constStrLit", "string", "string", "string", "string"),
    // string
    ("constArrLit", "string", "string", "string", "string"),
    // "s"
    ("constArrLitConst", "\"s\"", "\"s\"", "\"s\"", "\"s\""),
    // "x"
    ("constObjLitFresh", "\"x\"", "\"x\"", "\"x\"", "\"x\""),
    // string
    ("computedKey", "string", "string", "string", "string"),
    // string
    ("strDestr", "string", "string", "string", "string"),
    // any
    ("anyDestr", "any", "any", "any", "any"),
    // any
    ("anyArrDestr", "any", "any", "any", "any"),
    // number
    ("assignDestr", "number", "number", "number", "number"),
    // number
    ("assignArrDestr", "number", "number", "number", "number"),
    // "x"
    ("letConstLit", "\"x\"", "\"x\"", "\"x\"", "\"x\""),
    // "x"
    ("letConstLitArr", "\"x\"", "\"x\"", "\"x\"", "\"x\""),
    // string | number
    (
        "letDefaultLit",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // (string | number)[]
    (
        "constDefaultLitArr",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // (string | number)[]
    (
        "letDefaultRet",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // (string | number)[]
    (
        "arrLitTuple",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // ("s" | 1)[]
    (
        "arrLitTupleConst",
        "(\"s\" | 1)[]",
        "(\"s\" | 1)[]",
        "(\"s\" | 1)[]",
        "(\"s\" | 1)[]",
    ),
    // number | "d"
    (
        "arrDefaultUndef",
        "\"d\" | number",
        "\"d\" | number",
        "\"d\" | number",
        "\"d\" | number",
    ),
    // number
    (
        "objDefaultUndefUnion",
        "number",
        "number",
        "number",
        "number",
    ),
    // string
    ("nestedDefault", "string", "string", "string", "string"),
    // string[]
    ("arrRestRo", "string[]", "string[]", "string[]", "string[]"),
    // [(string | undefined)?, ...boolean[]] / [string?, ...boolean[]] / [(string | undefined)?, ...boolean[]] / [string?, ...boolean[]]
    (
        "tupleOptRest",
        "[(string | undefined)?, ...boolean[]]",
        "[string?, ...boolean[]]",
        "[(string | undefined)?, ...boolean[]]",
        "[string?, ...boolean[]]",
    ),
    // { b: string; }
    (
        "objRestRo",
        "{ b: string }",
        "{ b: string }",
        "{ b: string }",
        "{ b: string }",
    ),
    // { b: string; } | { c: number; }
    (
        "objRestUnion",
        "{ b: string } | { c: number }",
        "{ b: string } | { c: number }",
        "{ b: string } | { c: number }",
        "{ b: string } | { c: number }",
    ),
    // number
    ("objIndexSig", "number", "number", "number", "number"),
    // string
    ("objNumericKey", "string", "string", "string", "string"),
    // string
    ("objStringKey", "string", "string", "string", "string"),
    // string | number
    (
        "letNarrowLater",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string
    ("paramArrNested", "string", "string", "string", "string"),
    // string
    ("paramObjNested", "string", "string", "string", "string"),
    // { b: string; }
    (
        "paramObjRest",
        "{ b: string }",
        "{ b: string }",
        "{ b: string }",
        "{ b: string }",
    ),
    // [string, boolean]
    (
        "paramArrRest",
        "[string, boolean]",
        "[string, boolean]",
        "[string, boolean]",
        "[string, boolean]",
    ),
    // string | 0
    (
        "forOfLetReassign",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // (string | number)[]
    (
        "destrAssignObj",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // string | number
    (
        "destrAssignDefault",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // 1
    ("topVarA", "1", "1", "1", "1"),
    // "s"
    ("topVarB", "\"s\"", "\"s\"", "\"s\"", "\"s\""),
    // number
    ("topVarX", "number", "number", "number", "number"),
    // string
    ("topVarAnn", "string", "string", "string", "string"),
    // string
    ("topConstArr", "string", "string", "string", "string"),
    // string | number
    (
        "topDefault",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // boolean
    ("topNested", "boolean", "boolean", "boolean", "boolean"),
    // "x"
    ("topConstLit", "\"x\"", "\"x\"", "\"x\"", "\"x\""),
];

const FOR_OF_SOURCE: &str = r#"
export function forOfStr(s: string) { for (const c of s) return c; return 0; }
export function forOfTuple(t: [number, string]) { for (const x of t) return x; return true; }
export function forOfRoArr(xs: readonly boolean[]) { for (const x of xs) return x; return 0; }
export function forOfUnionElem(xs: (string | null)[]) { for (const x of xs) { if (x) return x; } return 0; }
export function forOfLitUnion(s: "ab" | "cd") { for (const c of s) return c; return 0; }
export function forOfLetElem(xs: number[]) { for (let x of xs) { x = 1; return x; } return "z"; }
export function forOfArrUnion(xs: string[] | number[]) { for (const x of xs) return x; return true; }
export function forOfLocal() { const xs = [1, 2]; for (const x of xs) return x; return "z"; }
export function forOfAny(xs: any) { for (const x of xs) return x; return 0; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`FOR_OF_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const FOR_OF_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string | 0
    (
        "forOfStr",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // string | number | true
    (
        "forOfTuple",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // 0 | boolean
    (
        "forOfRoArr",
        "0 | boolean",
        "0 | boolean",
        "0 | boolean",
        "0 | boolean",
    ),
    // string | 0
    (
        "forOfUnionElem",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // string | 0
    (
        "forOfLitUnion",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // number | "z"
    (
        "forOfLetElem",
        "\"z\" | number",
        "\"z\" | number",
        "\"z\" | number",
        "\"z\" | number",
    ),
    // string | number | true
    (
        "forOfArrUnion",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // number | "z"
    (
        "forOfLocal",
        "\"z\" | number",
        "\"z\" | number",
        "\"z\" | number",
        "\"z\" | number",
    ),
    // any
    ("forOfAny", "any", "any", "any", "any"),
];

const ITERATOR_SOURCE: &str = r#"
export function forOfIterable(it: Iterable<string>) { for (const x of it) return x; return 0; }
export function forOfCustom(it: { [Symbol.iterator](): Iterator<boolean> }) { for (const x of it) return x; return 0; }
export function forOfIterObj(it: IteratorObject<number>) { for (const x of it) return x; return "n"; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`ITERATOR_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const ITERATOR_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string | 0
    (
        "forOfIterable",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // 0 | boolean
    (
        "forOfCustom",
        "0 | boolean",
        "0 | boolean",
        "0 | boolean",
        "0 | boolean",
    ),
    // number | "n"
    (
        "forOfIterObj",
        "\"n\" | number",
        "\"n\" | number",
        "\"n\" | number",
        "\"n\" | number",
    ),
];

const MEMBER_WRITE_SOURCE: &str = r#"
type O = { x: 0 | 1; y: "a" | "b" };
type N = { p: { q: "a" | "b"; r: number }; s: string | number };
type A = { 0: "a" | "b"; k: "a" | "b"; "s-t": "a" | "b" };
declare function touch(): void;
declare function touchO(o: O): void;
export function memberParam(o: O) { o.y = "b"; return o.y; }
export function memberLocal() { const o: O = { x: 0, y: "a" }; o.y = "b"; return o.y; }
export function memberLet() { let o: O = { x: 0, y: "a" }; o.y = "b"; return o.y; }
export function memberOther(o: O) { o.x = 1; return o.y; }
export function memberSibling(o: O) { o.y = "b"; return o.x; }
export function memberNested(n: N) { n.p.q = "b"; return n.p.q; }
export function memberNestedParent(n: N) { n.p.q = "b"; return n.p; }
export function memberWhole(o: O) { o.y = "b"; return o; }
export function memberElemLit(a: A) { a["k"] = "b"; return a.k; }
export function memberElemLitRead(a: A) { a.k = "b"; return a["k"]; }
export function memberElemNum(a: A) { a[0] = "b"; return a[0]; }
export function memberElemDash(a: A) { a["s-t"] = "b"; return a["s-t"]; }
export function memberCallBetween(o: O) { o.y = "b"; touch(); return o.y; }
export function memberCallArg(o: O) { o.y = "b"; touchO(o); return o.y; }
export function memberPrefix(n: N, p: N["p"]) { n.p.q = "b"; n.p = p; return n.p.q; }
export function memberRootAssign(o: O, o2: O) { o.y = "b"; o = o2; return o.y; }
export function memberUnionProp(n: N) { n.s = 1; return n.s; }
export function memberWiden(n: N) { n.s = "lit"; return n.s; }
export function memberCond(o: O, c: boolean) { if (c) o.y = "b"; return o.y; }
export function memberBothArms(o: O, c: boolean) { if (c) o.y = "b"; else o.y = "a"; return o.y; }
export function memberLoop(o: O, c: boolean) { while (c) { o.y = "b"; } return o.y; }
export function memberNarrowThenAssign(o: O) { if (o.y === "a") { o.y = "b"; return o.y; } return o.y; }
export function memberInObj(o: O) { o.y = "b"; return { v: o.y }; }
export function memberThroughAlias(o: O) { const p = o; p.y = "b"; return o.y; }
export function memberCompound(n: N) { n.p.r += 1; return n.p.r; }
export function memberDeclaredAny(o: { y: any }) { o.y = "b"; return o.y; }
export function memberOptional(o: { y?: "a" | "b" }) { o.y = "b"; return o.y; }
export function memberElemComputedVarRead(a: A) { const k = "k"; a.k = "b"; return a[k]; }
export function memberStringKeyed(r: Record<string, string | number>) { r.a = 1; return r.a; }
export function memberArrElem(xs: (string | number)[]) { xs[0] = 1; return xs[0]; }
export function memberAwaitBetween(o: O) { return (async () => { o.y = "b"; await 0; return o.y; })(); }
export function memberNewBetween(o: O) { o.y = "b"; new Date(); return o.y; }
export function memberBool(o: { f: boolean }) { o.f = true; return o.f; }
export function memberBoolUnion(o: { f: true | "x" }) { o.f = true; return o.f; }
export function memberThreeArms(o: { y: "a" | "b" | "c" }, c: boolean) { if (c) o.y = "a"; else o.y = "b"; return o.y; }
export function memberThreeLoop(o: { y: "a" | "b" | "c" }, c: boolean) { o.y = "a"; while (c) { o.y = "b"; } return o.y; }
export function memberValuePos(o: O) { const v = (o.y = "b"); return o.y; }
export function memberTernaryStmt(o: O, c: boolean) { c ? (o.y = "b") : (o.y = "b"); return o.y; }
export function memberIncr(o: { n: 0 | 1 }) { o.n++; return o.n; }
export function memberGuardThenWriteRoot(o: O, p: O) { o.y = "b"; o = p; return o.y; }
export function memberNestedWritePrefixThenRead(n: N) { n.p.q = "b"; n.p = { q: "a", r: 1 }; return n.p.q; }
export function memberDeepAfterParentWrite(n: N) { n.p = { q: "b", r: 1 }; return n.p.q; }
export function memberOptChainRead(o: { y?: "a" | "b" }) { o.y = "b"; return o?.y; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`MEMBER_WRITE_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const MEMBER_WRITE_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "b"
    ("memberParam", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("memberLocal", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("memberLet", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "a" | "b"
    (
        "memberOther",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // 0 | 1
    ("memberSibling", "0 | 1", "0 | 1", "0 | 1", "0 | 1"),
    // "b"
    ("memberNested", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // { q: "a" | "b"; r: number; }
    (
        "memberNestedParent",
        "{ q: \"a\" | \"b\"; r: number }",
        "{ q: \"a\" | \"b\"; r: number }",
        "{ q: \"a\" | \"b\"; r: number }",
        "{ q: \"a\" | \"b\"; r: number }",
    ),
    // O
    ("memberWhole", "O", "O", "O", "O"),
    // "b"
    ("memberElemLit", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("memberElemLitRead", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("memberElemNum", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("memberElemDash", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("memberCallBetween", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("memberCallArg", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "a" | "b"
    (
        "memberPrefix",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "a" | "b"
    (
        "memberRootAssign",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // number
    ("memberUnionProp", "number", "number", "number", "number"),
    // string
    ("memberWiden", "string", "string", "string", "string"),
    // "a" | "b"
    (
        "memberCond",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "a" | "b"
    (
        "memberBothArms",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "a" | "b"
    (
        "memberLoop",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "b"
    ("memberNarrowThenAssign", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // { v: "b"; }
    (
        "memberInObj",
        "{ v: \"b\" }",
        "{ v: \"b\" }",
        "{ v: \"b\" }",
        "{ v: \"b\" }",
    ),
    // "a" | "b"
    (
        "memberThroughAlias",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // number
    ("memberCompound", "number", "number", "number", "number"),
    // any
    ("memberDeclaredAny", "any", "any", "any", "any"),
    // "b"
    ("memberOptional", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    (
        "memberElemComputedVarRead",
        "\"b\"",
        "\"b\"",
        "\"b\"",
        "\"b\"",
    ),
    // number
    ("memberStringKeyed", "number", "number", "number", "number"),
    // number
    ("memberArrElem", "number", "number", "number", "number"),
    // Promise<"b">
    (
        "memberAwaitBetween",
        "Promise<\"b\">",
        "Promise<\"b\">",
        "Promise<\"b\">",
        "Promise<\"b\">",
    ),
    // "b"
    ("memberNewBetween", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // boolean
    ("memberBool", "boolean", "boolean", "boolean", "boolean"),
    // boolean
    (
        "memberBoolUnion",
        "boolean",
        "boolean",
        "boolean",
        "boolean",
    ),
    // "a" | "b"
    (
        "memberThreeArms",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "a" | "b"
    (
        "memberThreeLoop",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "b"
    ("memberValuePos", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("memberTernaryStmt", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // number
    ("memberIncr", "number", "number", "number", "number"),
    // "a" | "b"
    (
        "memberGuardThenWriteRoot",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "a" | "b"
    (
        "memberNestedWritePrefixThenRead",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "a" | "b"
    (
        "memberDeepAfterParentWrite",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "b"
    ("memberOptChainRead", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
];

const LOGICAL_SOURCE: &str = r#"
export function andWrite(c: boolean) { let x: string | number = 1; c && (x = "s"); return x; }
export function orWrite(c: boolean) { let x: string | number = 1; c || (x = "s"); return x; }
export function nullishWrite(c: string | null) { let x: string | number = 1; c ?? (x = "s"); return x; }
export function andWriteLeft(c: boolean) { let x: string | number = 1; (x = "s") && c; return x; }
export function andWriteValue(c: boolean) { let x: string | number = 1; const v = c && (x = "s"); return [v, x]; }
export function andWriteReturn(c: boolean) { let x: string | number = 1; return c && (x = "s"); }
export function andWriteBoth(c: boolean, d: boolean) { let x: string | number | boolean = 1; c && (x = "s") && d && (x = true); return x; }
export function andWriteNarrow(y: string | number) { let x: string | number = 1; typeof y === "string" && (x = y); return x; }
export function andAssignSelf(c: string | number) { let x: string | number = c; x &&= "s"; return x; }
export function orAssignSelf(c: string | number) { let x: string | number = c; x ||= "s"; return x; }
export function nullishAssignSelf(c: string | null) { let x = c; x ??= "s"; return x; }
export function nullishAssignSelfN(c: number | null) { let x = c; x ??= 1; return x; }
export function nestedLogical(c: boolean, d: boolean) { let x: string | number | boolean = 1; (c && (x = "s")) || (x = true); return x; }
export function andWriteTernary(c: boolean) { let x: string | number = 1; c && (x = "s"); return c ? x : 0; }
export function andWriteExprStmtNarrow(c: string | number) { let x: string | number = c; x === 1 || (x = "s"); return x; }
export function andWriteNestedObj(c: boolean) { let x: string | number = 1; const o = { v: c && (x = "s") }; return x; }
export function andWriteTrue() { let x: string | number = 1; true && (x = "s"); return x; }
export function andWriteFalse() { let x: string | number = 1; false && (x = "s"); return x; }
export function evolvingLogical(c: boolean) { let x; return c && (x = "s"); }
export function andVal(c: boolean, s: string) { return c && s; }
export function orVal(a: string, b: number) { return a || b; }
export function nullishVal(a: string | null, b: number) { return a ?? b; }
export function nullishUndef(a: number | undefined) { return (a ?? 0) + 1; }
export function andObj(o: { a: number } | null) { return o && o.a; }
export function orDefault(s: string | undefined) { return s || "d"; }
export function andNum(n: number, s: "x") { return n && s; }
export function orBool(b: boolean) { return b || "no"; }
export function andLit(c: boolean) { return c && "s"; }
export function nullishLit(a: "x" | null) { return a ?? "y"; }
export function andUnion(a: string | number, b: boolean) { return a && b; }
export function nullishNonNull(a: string) { return a ?? 1; }
export function orAny(a: any) { return a || 1; }
export function andBig(a: bigint, s: string) { return a && s; }
export function andNarrow(a: string | number) { return typeof a === "string" && a; }
export function orNarrowRight(a: string | null) { return a === null || a; }
export function nestedNullish(a: string | null, b: number | null) { return a ?? b ?? true; }
export function nRight(c: string | null) { let r; c ?? (r = c); return r; }
export function nShort(c: string | null) { let r; c || (r = c); return r; }
export function nAfter(c: string | null) { c ?? (c = "s"); return c; }
export function nAnd(c: string | null) { let r; c && (r = c); return r; }
export function nZero(c: number | null) { let r; c ?? (r = c); return r; }
export function ifElseNarrowWrite(x: 1 | 2 | 3) { if (x === 1) {} else { x = 2; } return x; }
export function orNarrowWrite(x: 1 | 2 | 3) { x === 1 || (x = 2); return x; }
export function shortPath(c: "" | "a" | null) { c ?? 0; return c; }
export function shortPathWrite(c: "" | "a" | null) { let r: unknown = 0; c ?? (r = 1); return [c, r]; }
export function orShort(c: "" | "a" | null) { c || 0; return c; }
export function rightPathOr(c: "" | "a" | null) { let r; c || (r = c); return r; }
export function rightPathNullish(c: "" | "a" | null) { let r; c ?? (r = c); return r; }
export function shortPathAnd(c: "" | "a" | null) { let r; c && (r = c); return r; }
export function letAnd(c: boolean) { let v = c && "s"; return v; }
export function constAnd(c: boolean) { const v = c && "s"; return v; }
export function constAndArr(c: boolean) { const v = c && "s"; return [v]; }
export function objAnd(c: boolean) { return { v: c && "s" }; }
export function arrAnd(c: boolean) { return [c && "s"]; }
export function letOr(s: string) { let v = s || "d"; return v; }
export function objOrLit(s: "" | "a") { return { v: s || "d" }; }
export function letNullish(a: "x" | null) { let v = a ?? "y"; return v; }
export function objNullish(a: "x" | null) { return { v: a ?? "y" }; }
export function retNot(c: boolean) { return !c; }
export function objNot(c: string) { return { v: !c }; }
export function retNotObj(o: { a: 1 }) { return !o; }
export function retNotNull(o: null) { return !o; }
export function nonNullLet(a: 1 | null) { let v = a!; return v; }
export function retAndTrue() { return true && "s"; }
export function objTrueAnd() { return { v: true && "s" }; }
export function falsyLit(x: "a" | "") { if (!x) return x; return 0; }
export function falsyObj(x: { a: 1 } | 0) { if (!x) return x; return "t"; }
export function falsyTrue(x: true | 0) { if (!x) return x; return "t"; }
export function truthyLit(x: "a" | "") { if (x) return x; return 0; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`LOGICAL_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const LOGICAL_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string | number
    (
        "andWrite",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number
    (
        "orWrite",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number
    (
        "nullishWrite",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string
    ("andWriteLeft", "string", "string", "string", "string"),
    // (string | number | false)[] / (string | number)[] / (string | number | false)[] / (string | number)[]
    (
        "andWriteValue",
        "(false | number | string)[]",
        "(number | string)[]",
        "(false | number | string)[]",
        "(number | string)[]",
    ),
    // "s" | false / "" | "s" / "s" | false / "" | "s"
    (
        "andWriteReturn",
        "\"s\" | false",
        "\"\" | \"s\"",
        "\"s\" | false",
        "\"\" | \"s\"",
    ),
    // string | number | true
    (
        "andWriteBoth",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | number
    (
        "andWriteNarrow",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number
    (
        "andAssignSelf",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number
    (
        "orAssignSelf",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string
    ("nullishAssignSelf", "string", "string", "string", "string"),
    // number
    ("nullishAssignSelfN", "number", "number", "number", "number"),
    // string | true
    (
        "nestedLogical",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | number
    (
        "andWriteTernary",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | 1
    (
        "andWriteExprStmtNarrow",
        "1 | string",
        "1 | string",
        "1 | string",
        "1 | string",
    ),
    // string | number
    (
        "andWriteNestedObj",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string
    ("andWriteTrue", "string", "string", "string", "string"),
    // number
    ("andWriteFalse", "number", "number", "number", "number"),
    // "s" | false / "" | "s" / "s" | false / "" | "s"
    (
        "evolvingLogical",
        "\"s\" | false",
        "\"\" | \"s\"",
        "\"s\" | false",
        "\"\" | \"s\"",
    ),
    // string | false / string / string | false / string
    (
        "andVal",
        "false | string",
        "string",
        "false | string",
        "string",
    ),
    // string | number
    (
        "orVal",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number
    (
        "nullishVal",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // number
    ("nullishUndef", "number", "number", "number", "number"),
    // number | null / number / number | null / number
    (
        "andObj",
        "null | number",
        "number",
        "null | number",
        "number",
    ),
    // string
    ("orDefault", "string", "string", "string", "string"),
    // "x" | 0 / "" | "x" / "x" | 0 / "" | "x"
    (
        "andNum",
        "\"x\" | 0",
        "\"\" | \"x\"",
        "\"x\" | 0",
        "\"\" | \"x\"",
    ),
    // "no" | true
    (
        "orBool",
        "\"no\" | true",
        "\"no\" | true",
        "\"no\" | true",
        "\"no\" | true",
    ),
    // "s" | false / "" | "s" / "s" | false / "" | "s"
    (
        "andLit",
        "\"s\" | false",
        "\"\" | \"s\"",
        "\"s\" | false",
        "\"\" | \"s\"",
    ),
    // "x" | "y"
    (
        "nullishLit",
        "\"x\" | \"y\"",
        "\"x\" | \"y\"",
        "\"x\" | \"y\"",
        "\"x\" | \"y\"",
    ),
    // "" | 0 | boolean / boolean / "" | 0 | boolean / boolean
    (
        "andUnion",
        "\"\" | 0 | boolean",
        "boolean",
        "\"\" | 0 | boolean",
        "boolean",
    ),
    // string / string | 1 / string / string | 1
    (
        "nullishNonNull",
        "string",
        "1 | string",
        "string",
        "1 | string",
    ),
    // any
    ("orAny", "any", "any", "any", "any"),
    // string | 0n / string / string | 0n / string
    ("andBig", "0n | string", "string", "0n | string", "string"),
    // string | false / string / string | false / string
    (
        "andNarrow",
        "false | string",
        "string",
        "false | string",
        "string",
    ),
    // string | true
    (
        "orNarrowRight",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | number | true
    (
        "nestedNullish",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // null | undefined / string / any / any
    ("nRight", "null | undefined", "string", "any", "any"),
    // string | null | undefined / string / any / any
    (
        "nShort",
        "null | string | undefined",
        "string",
        "any",
        "any",
    ),
    // string
    ("nAfter", "string", "string", "string", "string"),
    // string | undefined / string / any / any
    ("nAnd", "string | undefined", "string", "any", "any"),
    // null | undefined / number / any / any
    ("nZero", "null | undefined", "number", "any", "any"),
    // 1 | 2
    ("ifElseNarrowWrite", "1 | 2", "1 | 2", "1 | 2", "1 | 2"),
    // 1 | 2
    ("orNarrowWrite", "1 | 2", "1 | 2", "1 | 2", "1 | 2"),
    // "" | "a" | null / "" | "a" / "" | "a" | null / "" | "a"
    (
        "shortPath",
        "\"\" | \"a\" | null",
        "\"\" | \"a\"",
        "\"\" | \"a\" | null",
        "\"\" | \"a\"",
    ),
    // unknown[]
    (
        "shortPathWrite",
        "unknown[]",
        "unknown[]",
        "unknown[]",
        "unknown[]",
    ),
    // "" | "a" | null / "" | "a" / "" | "a" | null / "" | "a"
    (
        "orShort",
        "\"\" | \"a\" | null",
        "\"\" | \"a\"",
        "\"\" | \"a\" | null",
        "\"\" | \"a\"",
    ),
    // "" | null | undefined / "" | "a" / any / any
    (
        "rightPathOr",
        "\"\" | null | undefined",
        "\"\" | \"a\"",
        "any",
        "any",
    ),
    // null | undefined / "" | "a" / any / any
    (
        "rightPathNullish",
        "null | undefined",
        "\"\" | \"a\"",
        "any",
        "any",
    ),
    // "a" | undefined / "a" / any / any
    ("shortPathAnd", "\"a\" | undefined", "\"a\"", "any", "any"),
    // string | false / string / string | false / string
    (
        "letAnd",
        "false | string",
        "string",
        "false | string",
        "string",
    ),
    // "s" | false / "" | "s" / "s" | false / "" | "s"
    (
        "constAnd",
        "\"s\" | false",
        "\"\" | \"s\"",
        "\"s\" | false",
        "\"\" | \"s\"",
    ),
    // (string | false)[] / string[] / (string | false)[] / string[]
    (
        "constAndArr",
        "(false | string)[]",
        "string[]",
        "(false | string)[]",
        "string[]",
    ),
    // { v: string | false; } / { v: string; } / { v: string | false; } / { v: string; }
    (
        "objAnd",
        "{ v: false | string }",
        "{ v: string }",
        "{ v: false | string }",
        "{ v: string }",
    ),
    // (string | false)[] / string[] / (string | false)[] / string[]
    (
        "arrAnd",
        "(false | string)[]",
        "string[]",
        "(false | string)[]",
        "string[]",
    ),
    // string
    ("letOr", "string", "string", "string", "string"),
    // { v: string; }
    (
        "objOrLit",
        "{ v: string }",
        "{ v: string }",
        "{ v: string }",
        "{ v: string }",
    ),
    // string
    ("letNullish", "string", "string", "string", "string"),
    // { v: string; }
    (
        "objNullish",
        "{ v: string }",
        "{ v: string }",
        "{ v: string }",
        "{ v: string }",
    ),
    // boolean
    ("retNot", "boolean", "boolean", "boolean", "boolean"),
    // { v: boolean; }
    (
        "objNot",
        "{ v: boolean }",
        "{ v: boolean }",
        "{ v: boolean }",
        "{ v: boolean }",
    ),
    // boolean
    ("retNotObj", "boolean", "boolean", "boolean", "boolean"),
    // boolean
    ("retNotNull", "boolean", "boolean", "boolean", "boolean"),
    // 1
    ("nonNullLet", "1", "1", "1", "1"),
    // string / "" | "s" / string / "" | "s"
    (
        "retAndTrue",
        "string",
        "\"\" | \"s\"",
        "string",
        "\"\" | \"s\"",
    ),
    // { v: string; }
    (
        "objTrueAnd",
        "{ v: string }",
        "{ v: string }",
        "{ v: string }",
        "{ v: string }",
    ),
    // "" | 0 / "" | "a" | 0 / "" | 0 / "" | "a" | 0
    (
        "falsyLit",
        "\"\" | 0",
        "\"\" | \"a\" | 0",
        "\"\" | 0",
        "\"\" | \"a\" | 0",
    ),
    // "t" | 0 / "t" | 0 | { a: 1; } / "t" | 0 / "t" | 0 | { a: 1; }
    (
        "falsyObj",
        "\"t\" | 0",
        "\"t\" | 0 | { a: 1 }",
        "\"t\" | 0",
        "\"t\" | 0 | { a: 1 }",
    ),
    // "t" | 0 / "t" | 0 | true / "t" | 0 / "t" | 0 | true
    (
        "falsyTrue",
        "\"t\" | 0",
        "\"t\" | 0 | true",
        "\"t\" | 0",
        "\"t\" | 0 | true",
    ),
    // "a" | 0
    (
        "truthyLit",
        "\"a\" | 0",
        "\"a\" | 0",
        "\"a\" | 0",
        "\"a\" | 0",
    ),
];

const UNINITIALIZED_SOURCE: &str = r#"
export function armRead(c: boolean) { let x; if (c) { x = 1; return x; } return "z"; }
export function armReadStr(c: boolean) { let x; if (c) { x = "s"; return x; } return 0; }
export function armReadElse(c: boolean) { let x; if (c) { return 0; } else { x = "s"; return x; } }
export function armReadAfter(c: boolean) { let x; if (c) { x = 1; } return x; }
export function loopRead(c: boolean) { let x; while (c) { x = 1; return x; } return "z"; }
export function loopReadAfter(c: boolean) { let x; while (c) { x = 1; } return x; }
export function forOfRead(xs: string[]) { let x; for (const s of xs) { x = s; return x; } return 0; }
export function nestedArm(c: boolean, d: boolean) { let x; if (c) { if (d) { x = 1; return x; } } return "z"; }
export function armTwoWrites(c: boolean) { let x; if (c) { x = 1; x = "s"; return x; } return 0; }
export function armObj(c: boolean) { let x; if (c) { x = 1; return { v: x }; } return { v: "z" }; }
export function armVar(c: boolean) { var x; if (c) { x = 1; return x; } return "z"; }
export function armNull(c: boolean) { let x; if (c) { x = null; return x; } return 0; }
export function armLoopAccum(c: boolean) { let x; while (c) { x = 1; if (c) return x; x = "s"; } return true; }
export function switchArm(k: number) { let x; switch (k) { case 1: x = "a"; return x; } return 0; }
export function tryArm() { let x; try { x = 1; return x; } catch { return "z"; } }
export function annotatedArm(c: boolean) { let x: number | string; if (c) { x = 1; return x; } return "z"; }
export function annotatedArmAfter(c: boolean) { let x: number | undefined; if (c) { x = 1; } return x; }
export function uninitReadNone() { let x; return x; }
export function capLetAssigned() { let x; x = "s"; const f = () => x; return f(); }
export function capVar() { var x; x = 1; const f = () => x; return f(); }
export function capLetNever() { let x; const f = () => x; return f(); }
export function capLetFnExpr() { let x; x = 1; return function () { return x; }(); }
export function unreachRead() { let x; x = 1; return x; return x; }
export function capAnnAfterIf(c: boolean) { let x: string | number = "s"; if (c) { x = 1; } const f = () => x; return f(); }
export function capAnnDef() { let x: string | number = "s"; x = 1; const f = () => x; return f(); }
export function capVarAnn() { var x: string | number = "s"; x = 1; const f = () => x; return f(); }
export function capLetDefinite(c: boolean) { let x; if (c) { x = 1; } else { x = "s"; } const f = () => x; return f(); }
export function r5CallOnConditionalVar(flag: boolean, cb: () => 1 | 2) {
  if (flag) var cb: () => 1 | 2 = () => 1;
  return cb();
}
export function varArm(c: boolean) { if (c) { var x = 1; return x; } return "z"; }
export function varSwitch(k: number) { switch (k) { case 1: var x = "a"; return x; } return 0; }
export function varBoth(c: boolean) { if (c) { var x = 1; } else { x = 2; } return x; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`UNINITIALIZED_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const UNINITIALIZED_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number | "z" / number | "z" / any / any
    ("armRead", "\"z\" | number", "\"z\" | number", "any", "any"),
    // string | 0 / string | 0 / any / any
    ("armReadStr", "0 | string", "0 | string", "any", "any"),
    // string | 0 / string | 0 / any / any
    ("armReadElse", "0 | string", "0 | string", "any", "any"),
    // number | undefined / number / any / any
    ("armReadAfter", "number | undefined", "number", "any", "any"),
    // number | "z" / number | "z" / any / any
    ("loopRead", "\"z\" | number", "\"z\" | number", "any", "any"),
    // number | undefined / number / any / any
    (
        "loopReadAfter",
        "number | undefined",
        "number",
        "any",
        "any",
    ),
    // string | 0 / string | 0 / any / any
    ("forOfRead", "0 | string", "0 | string", "any", "any"),
    // number | "z" / number | "z" / any / any
    (
        "nestedArm",
        "\"z\" | number",
        "\"z\" | number",
        "any",
        "any",
    ),
    // string | 0 / string | 0 / any / any
    ("armTwoWrites", "0 | string", "0 | string", "any", "any"),
    // { v: number; } | { v: string; } / { v: number; } | { v: string; } / { v: any; } / { v: any; }
    (
        "armObj",
        "{ v: number } | { v: string }",
        "{ v: number } | { v: string }",
        "{ v: any }",
        "{ v: any }",
    ),
    // number | "z" / number | "z" / any / any
    ("armVar", "\"z\" | number", "\"z\" | number", "any", "any"),
    // 0 | null / number / any / any
    ("armNull", "0 | null", "number", "any", "any"),
    // number | true / number | true / any / any
    (
        "armLoopAccum",
        "number | true",
        "number | true",
        "any",
        "any",
    ),
    // string | 0 / string | 0 / any / any
    ("switchArm", "0 | string", "0 | string", "any", "any"),
    // number | "z" / number | "z" / any / any
    ("tryArm", "\"z\" | number", "\"z\" | number", "any", "any"),
    // number | "z"
    (
        "annotatedArm",
        "\"z\" | number",
        "\"z\" | number",
        "\"z\" | number",
        "\"z\" | number",
    ),
    // number | undefined / number / number | undefined / number
    (
        "annotatedArmAfter",
        "number | undefined",
        "number",
        "number | undefined",
        "number",
    ),
    // undefined / undefined / any / any
    ("uninitReadNone", "undefined", "undefined", "any", "any"),
    // string / string / any / any
    ("capLetAssigned", "string", "string", "any", "any"),
    // any
    ("capVar", "any", "any", "any", "any"),
    // undefined / undefined / any / any
    ("capLetNever", "undefined", "undefined", "any", "any"),
    // number / number / any / any
    ("capLetFnExpr", "number", "number", "any", "any"),
    // any
    ("unreachRead", "any", "any", "any", "any"),
    // string | number
    (
        "capAnnAfterIf",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // number
    ("capAnnDef", "number", "number", "number", "number"),
    // string | number
    (
        "capVarAnn",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number / string | number / any / any
    (
        "capLetDefinite",
        "number | string",
        "number | string",
        "any",
        "any",
    ),
    // 1 | 2
    ("r5CallOnConditionalVar", "1 | 2", "1 | 2", "1 | 2", "1 | 2"),
    // number | "z"
    (
        "varArm",
        "\"z\" | number",
        "\"z\" | number",
        "\"z\" | number",
        "\"z\" | number",
    ),
    // string | 0
    (
        "varSwitch",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // number
    ("varBoth", "number", "number", "number", "number"),
];

/// Arithmetic, unary and update operators, element access and non-null
/// assertions over frame bindings, a member read or call directly on a
/// `new` expression, and initializers written with `!`, `&&`, `||` and
/// `!`-assertions. Operators follow `checkBinaryLikeExpression` and
/// `getUnaryResultType` (`+` is `string` when either operand is
/// string-like, `number` or `bigint` when both are of that kind, `any`
/// when either is `any`); element access reads the array, tuple or string
/// element, a literal index reading the tuple position.
#[test]
fn operators_over_frame_bindings_type_by_the_checkers_rules() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(&host, "operators.ts", OPERATOR_SOURCE, OPERATOR_TABLE);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Destructured `for…of` elements, declarations, parameters, assignment
/// patterns and top-level `var`/`let`/`const` patterns read the element
/// the checker's `getTypeForBindingElement` reads: the property or
/// position of the parent, a default replacing `undefined`, a rest as the
/// remaining tuple slice or the object without the named properties.
#[test]
fn destructured_bindings_read_the_checkers_binding_element_type() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "destructuring.ts",
        DESTRUCTURE_SOURCE,
        DESTRUCTURE_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// `for…of` over a string, a tuple, a readonly array, a union of arrays,
/// a local array and `any` reads the checker's iterated element type.
#[test]
fn for_of_over_a_non_literal_iterable_reads_its_element_type() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(&host, "for-of.ts", FOR_OF_SOURCE, FOR_OF_TABLE);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// The declarations [`ITERATOR_SOURCE`] iterates through: `Symbol.iterator`
/// and the lib's iterator interfaces.
const ITERATOR_LIB: &str = r#"
interface SymbolConstructor {
    readonly iterator: unique symbol;
}
declare var Symbol: SymbolConstructor;
interface IteratorYieldResult<TYield> {
    done?: false;
    value: TYield;
}
interface IteratorReturnResult<TReturn> {
    done: true;
    value: TReturn;
}
type IteratorResult<T, TReturn = any> = IteratorYieldResult<T> | IteratorReturnResult<TReturn>;
interface Iterator<T, TReturn = any, TNext = any> {
    next(...[value]: [] | [TNext]): IteratorResult<T, TReturn>;
    return?(value?: TReturn): IteratorResult<T, TReturn>;
    throw?(e?: any): IteratorResult<T, TReturn>;
}
interface Iterable<T, TReturn = any, TNext = any> {
    [Symbol.iterator](): Iterator<T, TReturn, TNext>;
}
interface IteratorObject<T, TReturn = unknown, TNext = unknown> extends Iterator<T, TReturn, TNext> {
    [Symbol.iterator](): IteratorObject<T, TReturn, TNext>;
}
"#;

/// `for…of` over a type with a `[Symbol.iterator]` method reads the
/// `value` of the `next()` results whose `done` admits `false`, the
/// checker's iteration-types protocol. Each policy runs in its own host so
/// the lib's global declarations stay one per program.
#[test]
fn for_of_follows_the_symbol_iterator_protocol() {
    let roots = [
        STRICT_ROOT,
        LOOSE_ROOT,
        STRICT_IMPLICIT_ROOT,
        LOOSE_IMPLICIT_ROOT,
    ];
    let mut mismatches = Vec::new();
    for (index, root) in roots.into_iter().enumerate() {
        let host = four_policy_host();
        upsert(&host, &format!("{root}/lib.ts"), ITERATOR_LIB);
        let canonical = format!("{root}/iterator.ts");
        upsert(&host, &canonical, ITERATOR_SOURCE);
        for row in ITERATOR_TABLE {
            let expected = [row.1, row.2, row.3, row.4][index];
            let observed = observe(&host, &canonical, row.0);
            if observed != expected {
                mismatches.push(format!(
                    "{canonical} `{}`: expected `{expected}`, observed `{observed}`",
                    row.0
                ));
            }
        }
    }
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// A member-path assignment narrows the reference to the assigned value
/// reduced by the declared member type (`getAssignmentReducedType`), for
/// dotted, literal element-access and nested paths; assigning a prefix or
/// the root drops the narrowing of the longer paths, a call does not, and
/// arms join the narrowed values.
#[test]
fn member_path_assignments_narrow_like_the_checker() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "member-writes.ts",
        MEMBER_WRITE_SOURCE,
        MEMBER_WRITE_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// `&&`, `||` and `??` values and writes follow the checker's flow graph:
/// the right operand runs on the left operand's truthy, falsy or nullish
/// edge, its writes join with the short-circuit path, and the value is
/// the definitely-falsy part of the left operand joined with the right
/// (`&&`), the left without its definitely-falsy part joined with the
/// right (`||`), or the non-nullable left joined with the right (`??`).
/// With strictNullChecks off the falsy edge keeps every constituent.
#[test]
fn logical_operands_follow_the_checkers_flow_graph() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(&host, "logical.ts", LOGICAL_SOURCE, LOGICAL_TABLE);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// A `let` or `var` without an initializer or annotation is auto-typed
/// with noImplicitAny on (its reads are the assigned values reaching them,
/// `undefined` where no assignment reaches) and declared `any` with
/// noImplicitAny off, including a first assignment inside an arm, loop,
/// `switch` or `try` read there; a closure reads a definitely assigned
/// auto-typed binding's final type.
#[test]
fn uninitialized_locals_read_the_checkers_auto_type() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "uninitialized.ts",
        UNINITIALIZED_SOURCE,
        UNINITIALIZED_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

const ASSIGNMENT_VALUE_SOURCE: &str = r#"
export function ternaryAssignLit(c: boolean) { let x: string | number = 1; return c ? (x = "s") : 0; }
export function ternaryAssignBoth(c: boolean) { let x: string | number = 1; return c ? (x = "s") : (x = 2); }
export function ternaryAssignAfter(c: boolean) { let x: string | number = 1; c ? (x = "s") : 0; return x; }
export function ternaryAssignConst(c: boolean) { let x: string | number = 1; const v = c ? (x = "s") : 0; return v; }
export function ternaryAssignLet(c: boolean) { let x: string | number = 1; let v = c ? (x = "s") : 0; return v; }
export function ternaryAssignArr(c: boolean) { let x: string | number = 1; return [c ? (x = "s") : 0]; }
export function assignRet() { let x: string | number = 1; return (x = "s"); }
export function assignConst() { let x: string | number = 1; const v = (x = "s"); return v; }
export function assignLet() { let x: string | number = 1; let v = (x = "s"); return v; }
export function assignArr() { let x: string | number = 1; return [(x = "s")]; }
export function assignObj() { let x: string | number = 1; return { v: (x = "s") }; }
export function assignParen(c: boolean) { let x: string | number = 1; return c ? ((x = "s")) : 1; }
export function assignAnd(c: boolean) { let x: string | number = 1; return c && (x = "s"); }
export function assignOr(c: string) { let x: string | number = 1; return c || (x = "s"); }
export function assignNullish(c: string | null) { let x: string | number = 1; return c ?? (x = "s"); }
export function assignNum(c: boolean) { let x: number | string = "a"; return c ? (x = 1) : "z"; }
export function assignDeclLit(c: boolean) { let x: "a" | "b" = "a"; return c ? (x = "b") : 0; }
export function assignDeclLitConst(c: boolean) { let x: "a" | "b" = "a"; const v = c ? (x = "b") : 0; return v; }
export function assignNested(c: boolean, d: boolean) { let x: string | number = 1; return c ? (d ? (x = "s") : 1) : 0; }
export function assignSpread() { let x: string | number = 1; return [...[(x = "s")]]; }
export function assignParam(p: string | number, c: boolean) { return c ? (p = "s") : 0; }
export function c1() { let x: string | number = 1; const v = (x = "s"); return v; }
export function c2() { let x: any = 1; const v = (x = "s"); return v; }
export function c3() { let x: unknown = 1; const v = (x = "s"); return v; }
export function c4() { let x: "s" | number = 1; const v = (x = "s"); return v; }
export function c5() { let x = "a"; const v = (x = "s"); return v; }
export function c6(c: boolean) { let x: string | number = 1; const v = c ? (x = "s") : "t"; return v; }
export function c7(c: boolean) { let x: string | number = 1; const v = c ? (x = "s") : (x = "t"); return v; }
export function c8() { let x: string | number = 1; const v = (x = "s"); const w = v; return [w]; }
export function c9() { let x: string | number = 1; const v = (x = "s"); let w = v; return w; }
export function c10() { let x: string | number = 1; const v = (x = "s"); return v === "s"; }
export function c12() { let x: string | number = 1; const v = [(x = "s")] as const; return v; }
export function c15(c: boolean) { let x: string | number = 1; return c ? (x = "s") : (x = "s"); }
export function c16() { let x: string | number = 1; const v = x = "s"; return v; }
export function c17() { let x; const v = (x = "s"); return v; }
export function c18(c: boolean) { let x; return c ? (x = "s") : 0; }
export function b1() { const v = "s"; return v; }
export function b2(c: boolean) { const v = c ? "s" : 0; return v; }
export function b3() { let x: string | number = 1; x = "s"; return x; }
export function b4() { let x: string | number = 1; const v = (x = "s"); return [v]; }
export function b5() { const v = ("s"); return v; }
export function b6() { let x: string | number = 1; const v = (x = "s"); return { v }; }
export function b7(c: boolean) { let x: string | number = 1; const v = c ? (x = "s") : 0; return [v]; }
export function b8(c: boolean) { const v = c ? "s" : 0; return [v]; }
export function b9() { let x: string | number = 1; const v = (x = "s"); const o = { a: v }; return o; }
export function b11() { let x: string | number = 1; const v = (x = "s"); const w: "s" = v; return w; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`ASSIGNMENT_VALUE_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const ASSIGNMENT_VALUE_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "s" | 0
    (
        "ternaryAssignLit",
        "\"s\" | 0",
        "\"s\" | 0",
        "\"s\" | 0",
        "\"s\" | 0",
    ),
    // "s" | 2
    (
        "ternaryAssignBoth",
        "\"s\" | 2",
        "\"s\" | 2",
        "\"s\" | 2",
        "\"s\" | 2",
    ),
    // string | number
    (
        "ternaryAssignAfter",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // "s" | 0
    (
        "ternaryAssignConst",
        "\"s\" | 0",
        "\"s\" | 0",
        "\"s\" | 0",
        "\"s\" | 0",
    ),
    // string | number
    (
        "ternaryAssignLet",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // (string | number)[]
    (
        "ternaryAssignArr",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // string
    ("assignRet", "string", "string", "string", "string"),
    // string
    ("assignConst", "string", "string", "string", "string"),
    // string
    ("assignLet", "string", "string", "string", "string"),
    // string[]
    ("assignArr", "string[]", "string[]", "string[]", "string[]"),
    // { v: string; }
    (
        "assignObj",
        "{ v: string }",
        "{ v: string }",
        "{ v: string }",
        "{ v: string }",
    ),
    // "s" | 1
    (
        "assignParen",
        "\"s\" | 1",
        "\"s\" | 1",
        "\"s\" | 1",
        "\"s\" | 1",
    ),
    // "s" | false / "" | "s" / "s" | false / "" | "s"
    (
        "assignAnd",
        "\"s\" | false",
        "\"\" | \"s\"",
        "\"s\" | false",
        "\"\" | \"s\"",
    ),
    // string
    ("assignOr", "string", "string", "string", "string"),
    // string
    ("assignNullish", "string", "string", "string", "string"),
    // "z" | 1
    (
        "assignNum",
        "\"z\" | 1",
        "\"z\" | 1",
        "\"z\" | 1",
        "\"z\" | 1",
    ),
    // "b" | 0
    (
        "assignDeclLit",
        "\"b\" | 0",
        "\"b\" | 0",
        "\"b\" | 0",
        "\"b\" | 0",
    ),
    // "b" | 0
    (
        "assignDeclLitConst",
        "\"b\" | 0",
        "\"b\" | 0",
        "\"b\" | 0",
        "\"b\" | 0",
    ),
    // "s" | 0 | 1
    (
        "assignNested",
        "\"s\" | 0 | 1",
        "\"s\" | 0 | 1",
        "\"s\" | 0 | 1",
        "\"s\" | 0 | 1",
    ),
    // string[]
    (
        "assignSpread",
        "string[]",
        "string[]",
        "string[]",
        "string[]",
    ),
    // "s" | 0
    (
        "assignParam",
        "\"s\" | 0",
        "\"s\" | 0",
        "\"s\" | 0",
        "\"s\" | 0",
    ),
    // string
    ("c1", "string", "string", "string", "string"),
    // string
    ("c2", "string", "string", "string", "string"),
    // string
    ("c3", "string", "string", "string", "string"),
    // string
    ("c4", "string", "string", "string", "string"),
    // string
    ("c5", "string", "string", "string", "string"),
    // "s" | "t"
    (
        "c6",
        "\"s\" | \"t\"",
        "\"s\" | \"t\"",
        "\"s\" | \"t\"",
        "\"s\" | \"t\"",
    ),
    // "s" | "t"
    (
        "c7",
        "\"s\" | \"t\"",
        "\"s\" | \"t\"",
        "\"s\" | \"t\"",
        "\"s\" | \"t\"",
    ),
    // string[]
    ("c8", "string[]", "string[]", "string[]", "string[]"),
    // string
    ("c9", "string", "string", "string", "string"),
    // boolean
    ("c10", "boolean", "boolean", "boolean", "boolean"),
    // readonly ["s"]
    (
        "c12",
        "readonly [\"s\"]",
        "readonly [\"s\"]",
        "readonly [\"s\"]",
        "readonly [\"s\"]",
    ),
    // string
    ("c15", "string", "string", "string", "string"),
    // string
    ("c16", "string", "string", "string", "string"),
    // string
    ("c17", "string", "string", "string", "string"),
    // "s" | 0
    ("c18", "\"s\" | 0", "\"s\" | 0", "\"s\" | 0", "\"s\" | 0"),
    // string
    ("b1", "string", "string", "string", "string"),
    // "s" | 0
    ("b2", "\"s\" | 0", "\"s\" | 0", "\"s\" | 0", "\"s\" | 0"),
    // string
    ("b3", "string", "string", "string", "string"),
    // string[]
    ("b4", "string[]", "string[]", "string[]", "string[]"),
    // string
    ("b5", "string", "string", "string", "string"),
    // { v: string; }
    (
        "b6",
        "{ v: string }",
        "{ v: string }",
        "{ v: string }",
        "{ v: string }",
    ),
    // (string | number)[]
    (
        "b7",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // (string | number)[]
    (
        "b8",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // { a: string; }
    (
        "b9",
        "{ a: string }",
        "{ a: string }",
        "{ a: string }",
        "{ a: string }",
    ),
    // "s"
    ("b11", "\"s\"", "\"s\"", "\"s\"", "\"s\""),
];

const CONDITION_WRITE_SOURCE: &str = r#"
export function andWriteIf(c: boolean) { let x: string | number = 1; if (c && (x = "s")) { return x; } return x; }
export function andWriteIfElse(c: boolean) { let x: string | number = 1; if (c && (x = "s")) { return 0; } return x; }
export function orWriteIf(c: boolean) { let x: string | number = 1; if (c || (x = "s")) { return x; } return true; }
export function orWriteIfElse(c: boolean) { let x: string | number = 1; if (c || (x = "s")) { return 0; } return x; }
export function plainWriteIf() { let x: string | number = 1; if ((x = "s")) { return x; } return x; }
export function plainWriteIfEmpty() { let x: string | number = 1; if ((x = "")) { return x; } return x; }
export function writeIfAfter(c: boolean) { let x: string | number = 1; if (c && (x = "s")) { } return x; }
export function writeCompare(c: boolean) { let x: string | number = 1; if ((x = "s") === "s") { return x; } return x; }
export function writeNotIf(c: boolean) { let x: string | number = 1; if (!(c && (x = "s"))) { return x; } return 0; }
export function writeTypeofIf(c: boolean) { let x: string | number = 1; if (typeof (x = "s") === "string") { return x; } return 0; }
export function nestedAndOr(c: boolean, d: boolean) { let x: string | number | boolean = 1; if ((c && (x = "s")) || (d && (x = true))) { return x; } return x; }
export function andAssign(c: boolean) { let x: string | number = 1; let d = c; d &&= (x = "s") === "s"; return x; }
export function orAssignOther(c: boolean) { let x: string | number = 1; let d = c; d ||= (x = "s") === "s"; return x; }
export function nullishAssignWrite(c: string | undefined) { let x: string | number = 1; let d = c; d ??= (x = "s"); return x; }
export function compareStmt() { let x: string | number = 1; (x = "s") === "s"; return x; }
export function compareInAnd(c: boolean) { let x: string | number = 1; c && (x = "s") === "s"; return x; }
export function plainOther() { let x: string | number = 1; let d: unknown; d = (x = "s"); return x; }
export function plainOtherCmp() { let x: string | number = 1; let d: unknown; d = (x = "s") === "s"; return x; }
export function nullishOther(c: string | undefined) { let x: string | number = 1; let d = c; d ??= (x = "s"); return x; }
export function andOtherCmp(c: boolean) { let x: string | number = 1; let d = c; d &&= (x = "s") === "s"; return x; }
export function andOtherPlain(c: boolean) { let x: string | number = 1; let d: unknown = c; d &&= (x = "s"); return x; }
export function andOtherRet(c: boolean) { let x: string | number = 1; let d: unknown = c; d &&= (x = "s"); return d; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`CONDITION_WRITE_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const CONDITION_WRITE_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string | number
    (
        "andWriteIf",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number
    (
        "andWriteIfElse",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number | true
    (
        "orWriteIf",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | 0
    (
        "orWriteIfElse",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // string
    ("plainWriteIf", "string", "string", "string", "string"),
    // string
    ("plainWriteIfEmpty", "string", "string", "string", "string"),
    // string | number
    (
        "writeIfAfter",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string
    ("writeCompare", "string", "string", "string", "string"),
    // string | number
    (
        "writeNotIf",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | 0
    (
        "writeTypeofIf",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // string | number | true
    (
        "nestedAndOr",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | number
    (
        "andAssign",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number
    (
        "orAssignOther",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number
    (
        "nullishAssignWrite",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string
    ("compareStmt", "string", "string", "string", "string"),
    // string | number
    (
        "compareInAnd",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string
    ("plainOther", "string", "string", "string", "string"),
    // string
    ("plainOtherCmp", "string", "string", "string", "string"),
    // string | number
    (
        "nullishOther",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number
    (
        "andOtherCmp",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number
    (
        "andOtherPlain",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // unknown
    ("andOtherRet", "unknown", "unknown", "unknown", "unknown"),
];

const ELEMENT_KEY_SOURCE: &str = r#"
type A = { k: "a" | "b"; j: "a" | "b"; 0: "a" | "b" };
type R = Record<string, string | number>;
declare function key(): "k";
export function recConstKey(r: R) { const k = "x"; r[k] = 1; return r[k]; }
export function aParamKeyLit(a: A, k: "k") { a[k] = "b"; return a[k]; }
export function aParamKeyUnion(a: A, k: "k" | "j") { a[k] = "b"; return a[k]; }
export function aParamKeyWritten(a: A, k: "k") { k = "k"; a[k] = "b"; return a[k]; }
export function aLetKey(a: A) { let k: "k" = "k"; a[k] = "b"; return a[k]; }
export function aLetKeyWrittenAfter(a: A) { let k: "k" = "k"; a[k] = "b"; k = "k"; return a[k]; }
export function aConstCallKey(a: A) { const k = key(); a[k] = "b"; return a[k]; }
export function aDotThenKey(a: A, k: "k") { a.k = "b"; return a[k]; }
export function aKeyThenOther(a: A, k: "k" | "j") { a.k = "b"; a[k] = "a"; return a.k; }
export function aNumKey(a: A, i: 0) { a[i] = "b"; return a[i]; }
export function aParamKeyInvalidates(a: A, k: "k" | "j") { a.k = "b"; a[k] = "a"; return a.j; }
export function arrIdxWrite(xs: (string | number)[], i: number) { xs[i] = 1; return xs[i]; }
export function arrIdxWriteLit(xs: (string | number)[], i: number) { xs[0] = 1; xs[i] = "s"; return xs[0]; }
export function aKeyWrittenBetween(a: A, k: "k" | "j", j: "k" | "j") { a[k] = "b"; k = j; return a[k]; }
export function aDotThenParamKeyRead(a: A, k: "k") { a.k = "b"; const v = a[k]; return v; }
export function aConstKeyThenDot(a: A) { const k = "k"; a.k = "b"; return a[k]; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`ELEMENT_KEY_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const ELEMENT_KEY_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number
    ("recConstKey", "number", "number", "number", "number"),
    // "b"
    ("aParamKeyLit", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("aParamKeyUnion", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "a" | "b"
    (
        "aParamKeyWritten",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "b"
    ("aLetKey", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "a" | "b"
    (
        "aLetKeyWrittenAfter",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "b"
    ("aConstCallKey", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "a" | "b"
    (
        "aDotThenKey",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "b"
    ("aKeyThenOther", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("aNumKey", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "a" | "b"
    (
        "aParamKeyInvalidates",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // number
    ("arrIdxWrite", "number", "number", "number", "number"),
    // number
    ("arrIdxWriteLit", "number", "number", "number", "number"),
    // "a" | "b"
    (
        "aKeyWrittenBetween",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "a" | "b"
    (
        "aDotThenParamKeyRead",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // "b"
    ("aConstKeyThenDot", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
];

const STATEMENT_CALL_SOURCE: &str = r#"
type O = { x: 0 | 1; y: "a" | "b" };
declare function assertIsB(v: unknown): asserts v is "b";
declare const obj: { m(): void; n(v: unknown): asserts v is "b" };
export function memberMethodCall(o: O & { m(): void }) { o.y = "b"; o.m(); return o.y; }
export function methodCallNoWrite(o: O & { m(): void }) { o.m(); return o.y; }
export function methodCallParamRead(o: O & { m(): void }, p: string | number) { o.m(); return p; }
export function memberClosureCall(o: O) { const f = () => {}; o.y = "b"; f(); return o.y; }
export function closureCallNoWrite(o: O) { const f = () => {}; f(); return o.y; }
export function closureWritesLocal(o: O) { let z: string | number = 1; const f = () => { z = "s"; }; f(); return z; }
export function closureWritesMember(o: O) { const f = () => { o.y = "a"; }; o.y = "b"; f(); return o.y; }
export function methodCallOnOther(o: O, q: { m(): void }) { o.y = "b"; q.m(); return o.y; }
export function globalMethodCall(o: O) { o.y = "b"; obj.m(); return o.y; }
export function assertsFree(o: O) { assertIsB(o.y); return o.y; }
export function methodCallOnLocal(o: O) { const q = { m() {} }; o.y = "b"; q.m(); return o.y; }
export function methodReturnValueDiscard(o: O & { m(): number }) { o.y = "b"; o.m(); return o.y; }
export function memberNarrowGuardThenCall(o: O & { m(): void }) { if (o.y === "b") { o.m(); return o.y; } return "z"; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`STATEMENT_CALL_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const STATEMENT_CALL_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "b"
    ("memberMethodCall", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "a" | "b"
    (
        "methodCallNoWrite",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // string | number
    (
        "methodCallParamRead",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // "b"
    ("memberClosureCall", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "a" | "b"
    (
        "closureCallNoWrite",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
        "\"a\" | \"b\"",
    ),
    // number
    ("closureWritesLocal", "number", "number", "number", "number"),
    // "b"
    ("closureWritesMember", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("methodCallOnOther", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("globalMethodCall", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("assertsFree", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    ("methodCallOnLocal", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // "b"
    (
        "methodReturnValueDiscard",
        "\"b\"",
        "\"b\"",
        "\"b\"",
        "\"b\"",
    ),
    // "b" | "z"
    (
        "memberNarrowGuardThenCall",
        "\"b\" | \"z\"",
        "\"b\" | \"z\"",
        "\"b\" | \"z\"",
        "\"b\" | \"z\"",
    ),
];

const CORRELATED_SOURCE: &str = r#"
type U = { kind: "a"; v: string } | { kind: "b"; v: number };
type T = ["a", string] | ["b", number];
export function objConst(o: U) { const { kind, v } = o; if (kind === "a") return v; return true; }
export function objConstElse(o: U) { const { kind, v } = o; if (kind === "a") return 0; return v; }
export function objLet(o: U) { let { kind, v } = o; if (kind === "a") return v; return true; }
export function objLetAssigned(o: U) { let { kind, v } = o; if (kind === "a") { return v; } kind = "b"; return true; }
export function objConstParamReassigned(o: U, p: U) { const { kind, v } = o; o = p; if (kind === "a") return v; return true; }
export function arrConst(t: T) { const [k, v] = t; if (k === "a") return v; return true; }
export function arrLet(t: T) { let [k, v] = t; if (k === "a") return v; return true; }
export function paramArrDestr([k, v]: T) { if (k === "a") return v; return true; }
export function objTypeof(o: U) { const { kind, v } = o; if (typeof v === "string") return kind; return true; }
export function objNe(o: U) { const { kind, v } = o; if (kind !== "a") return v; return true; }
export function objRename(o: U) { const { kind: k, v: w } = o; if (k === "a") return w; return true; }
export function objNested(o: { p: U }) { const { p: { kind, v } } = o; if (kind === "a") return v; return true; }
export function objDefault(o: U) { const { kind, v = 5 } = o; if (kind === "a") return v; return true; }
export function objRest(o: U) { const { kind, ...rest } = o; if (kind === "a") return rest; return true; }
export function objVar(o: U) { var { kind, v } = o; if (kind === "a") return v; return true; }
export function objTruthy(o: { ok: true; v: string } | { ok: false; v: number }) { const { ok, v } = o; if (ok) return v; return true; }
export function objNonUnion(o: { kind: "a" | "b"; v: string | number }) { const { kind, v } = o; if (kind === "a") return v; return true; }
export function objFromCall(f: () => U) { const { kind, v } = f(); if (kind === "a") return v; return true; }
export function forOfCorr(xs: U[]) { for (const { kind, v } of xs) { if (kind === "a") return v; } return true; }
type M = { k1: "x"; k2: "p"; v: 1 } | { k1: "y"; k2: "q"; v: 2 } | { k1: "x"; k2: "q"; v: 3 };
type B = { ok: true; v: string } | { ok: false; v: number };
type N = { kind: "s"; v: string } | { kind: 1; v: number };
export function orGuard(o: U, c: boolean) { const { kind, v } = o; if (c || kind === "a") return v; return true; }
export function andGuard(o: U, c: boolean) { const { kind, v } = o; if (c && kind === "a") return v; return true; }
export function earlyReturn(o: U) { const { kind, v } = o; if (kind === "b") return true; return v; }
export function multiDisc(o: M) { const { k1, k2, v } = o; if (k1 === "x" && k2 === "p") return v; return 0; }
export function multiDiscJoin(o: M, c: boolean) { const { k1, k2, v } = o; if (c) { if (k1 !== "x" || k2 !== "p") return 0; } else { if (k1 !== "y") return 0; } return v; }
export function crossDisc(o: M) { const { k1, k2 } = o; if (k2 === "p") return k1; return 0; }
export function truthyDisc(o: B) { const { ok, v } = o; if (!ok) return v; return 0; }
export function typeofDisc(o: N) { const { kind, v } = o; if (typeof kind === "string") return v; return true; }
export function sourceAlias(o: U) { const { kind } = o; if (kind === "a") return o.v; return true; }
export function sourceAliasMember(o: { u: U }) { const { kind } = o.u; if (kind === "a") return o.u.v; return true; }
export function sourceAliasNonDisc(o: U) { const { v } = o; if (typeof v === "string") return o.kind; return true; }
export function sourceAliasAnnotated(o: U) { const { kind }: U = o; if (kind === "a") return o.v; return true; }
export function sourceAliasLet(o: U) { let { kind } = o; if (kind === "a") return o.v; return true; }
export function reassignedSource(o: U, p: U) { const { kind } = o; o = p; if (kind === "a") return o.v; return true; }
export function narrowedElemThenDisc(o: { kind: "a"; v: string | null } | { kind: "b"; v: number }) { const { kind, v } = o; if (v !== null && kind === "a") return v; return true; }
export function nestedCorr(o: { p: U }) { const { p: { kind, v } } = o; if (kind === "a") return v; return true; }
export function loopCorr(xs: U[]) { for (const { kind, v } of xs) { if (kind === "b") continue; return v; } return true; }
export function inElem(o: { kind: "a"; v: { x: 1 } } | { kind: "b"; v: { y: 2 } }) { const { kind, v } = o; if ("x" in v) return kind; return true; }
export function paramArrUnassigned([k, v]: ["a", string] | ["b", number]) { if (k === "a") return v; return true; }
export function paramArrAssigned([k, v]: ["a", string] | ["b", number]) { if (k === "a") { return v; } v = 1; return true; }
export function paramObjNested({ p: { kind, v } }: { p: U }) { if (kind === "a") return v; return true; }
export function discLoose(o: U) { const { kind, v } = o; if (kind == "a") return v; return true; }
export function discWithDefaultSibling(o: U) { const { kind = "a", v } = o; if (kind === "a") return v; return true; }
export function singleElem(o: U) { const { kind } = o; if (kind === "a") return kind; return true; }
export function tupDisc(t: T) { if (t[0] === "a") return t[1]; return true; }
export function tupDiscNe(t: T) { if (t[0] !== "a") return t[1]; return true; }
export function arrConstNe(t: T) { const [k, v] = t; if (k !== "a") return v; return true; }
export function arrConstK(t: T) { const [k, v] = t; if (v === 1) return k; return true; }
export function typeofMember(o: N) { if (typeof o.kind === "string") return o.v; return true; }
export function typeofMemberNe(o: N) { if (typeof o.kind !== "string") return o.v; return true; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`CORRELATED_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const CORRELATED_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string | true
    (
        "objConst",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // number
    ("objConstElse", "number", "number", "number", "number"),
    // string | number | true
    (
        "objLet",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | number | true
    (
        "objLetAssigned",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | true
    (
        "objConstParamReassigned",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true
    (
        "arrConst",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | number | true
    (
        "arrLet",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | true
    (
        "paramArrDestr",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // "a" | "b" | true
    (
        "objTypeof",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
    ),
    // number | true
    (
        "objNe",
        "number | true",
        "number | true",
        "number | true",
        "number | true",
    ),
    // string | true
    (
        "objRename",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true
    (
        "objNested",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | number | true
    (
        "objDefault",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // true | { v: string; } | { v: number; }
    (
        "objRest",
        "true | { v: number } | { v: string }",
        "true | { v: number } | { v: string }",
        "true | { v: number } | { v: string }",
        "true | { v: number } | { v: string }",
    ),
    // string | number | true
    (
        "objVar",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | true
    (
        "objTruthy",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | number | true
    (
        "objNonUnion",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | true
    (
        "objFromCall",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true
    (
        "forOfCorr",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | number | true
    (
        "orGuard",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | true
    (
        "andGuard",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true
    (
        "earlyReturn",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // 0 | 1
    ("multiDisc", "0 | 1", "0 | 1", "0 | 1", "0 | 1"),
    // 0 | 1 | 2
    (
        "multiDiscJoin",
        "0 | 1 | 2",
        "0 | 1 | 2",
        "0 | 1 | 2",
        "0 | 1 | 2",
    ),
    // "x" | 0
    (
        "crossDisc",
        "\"x\" | 0",
        "\"x\" | 0",
        "\"x\" | 0",
        "\"x\" | 0",
    ),
    // number / string | number / number / string | number
    (
        "truthyDisc",
        "number",
        "number | string",
        "number",
        "number | string",
    ),
    // string | true
    (
        "typeofDisc",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true
    (
        "sourceAlias",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true
    (
        "sourceAliasMember",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // "a" | "b" | true
    (
        "sourceAliasNonDisc",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
    ),
    // string | number | true
    (
        "sourceAliasAnnotated",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | number | true
    (
        "sourceAliasLet",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | true
    (
        "reassignedSource",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true
    (
        "narrowedElemThenDisc",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true
    (
        "nestedCorr",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true
    (
        "loopCorr",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // "a" | "b" | true
    (
        "inElem",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
    ),
    // string | true
    (
        "paramArrUnassigned",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | number | true
    (
        "paramArrAssigned",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | true
    (
        "paramObjNested",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true
    (
        "discLoose",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | number | true
    (
        "discWithDefaultSibling",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // "a" | true
    (
        "singleElem",
        "\"a\" | true",
        "\"a\" | true",
        "\"a\" | true",
        "\"a\" | true",
    ),
    // string | true
    (
        "tupDisc",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // number | true
    (
        "tupDiscNe",
        "number | true",
        "number | true",
        "number | true",
        "number | true",
    ),
    // number | true
    (
        "arrConstNe",
        "number | true",
        "number | true",
        "number | true",
        "number | true",
    ),
    // "a" | "b" | true
    (
        "arrConstK",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
    ),
    // string | true
    (
        "typeofMember",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // number | true
    (
        "typeofMemberNe",
        "number | true",
        "number | true",
        "number | true",
        "number | true",
    ),
];

const RETURN_PREDICATE_SOURCE: &str = r#"
export function elemRoArr(xs: readonly boolean[], i: number) { return xs[i]; }
export function arrIdx(xs: boolean[], i: number) { return xs[i]; }
export function elemLit(xs: boolean[]) { return xs[0]; }
export function tupleElem(t: [boolean, string]) { return t[0]; }
export function paramBool(b: boolean) { return b; }
export function paramBoolNot(b: boolean) { return !b; }
export function andElem(xs: boolean[], x: string | number) { return typeof x === "string" && xs[0]; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`RETURN_PREDICATE_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const RETURN_PREDICATE_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // boolean
    ("elemRoArr", "boolean", "boolean", "boolean", "boolean"),
    // boolean
    ("arrIdx", "boolean", "boolean", "boolean", "boolean"),
    // boolean
    ("elemLit", "boolean", "boolean", "boolean", "boolean"),
    // boolean
    ("tupleElem", "boolean", "boolean", "boolean", "boolean"),
    // boolean
    ("paramBool", "boolean", "boolean", "boolean", "boolean"),
    // boolean
    ("paramBoolNot", "boolean", "boolean", "boolean", "boolean"),
    // boolean
    ("andElem", "boolean", "boolean", "boolean", "boolean"),
];

const CATCH_SOURCE: &str = r#"
export function catchRet() { try { return 1; } catch (e) { return e; } }
export function catchTypeof() { try { return 1; } catch (e) { if (typeof e === "string") return e; return 0; } }
export function catchAnyAnn() { try { return 1; } catch (e: any) { return e; } }
export function catchUnknownAnn() { try { return 1; } catch (e: unknown) { return e; } }
export function catchAssign() { try { return 1; } catch (e) { e = "s"; return e; } }
export function catchNoBinding() { try { return 1; } catch { return "c"; } }
export function catchClosure() { try { return 1; } catch (e) { const f = () => e; return f(); } }
export function catchTruthy() { try { return 1; } catch (e) { if (e) return e; return 0; } }
export function catchEq() { try { return 1; } catch (e) { if (e === "x") return e; return 0; } }
"#;

/// `(symbol, strict, strict off, strict without useUnknownInCatchVariables,
/// strict off with useUnknownInCatchVariables)` for [`CATCH_SOURCE`] in
/// the projects of [`catch_policy_host`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const CATCH_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // unknown / any / any / unknown
    ("catchRet", "unknown", "any", "any", "unknown"),
    // string | 0 | 1
    (
        "catchTypeof",
        "0 | 1 | string",
        "0 | 1 | string",
        "0 | 1 | string",
        "0 | 1 | string",
    ),
    // any
    ("catchAnyAnn", "any", "any", "any", "any"),
    // unknown
    (
        "catchUnknownAnn",
        "unknown",
        "unknown",
        "unknown",
        "unknown",
    ),
    // unknown / any / any / unknown
    ("catchAssign", "unknown", "any", "any", "unknown"),
    // "c" | 1
    (
        "catchNoBinding",
        "\"c\" | 1",
        "\"c\" | 1",
        "\"c\" | 1",
        "\"c\" | 1",
    ),
    // unknown / any / any / unknown
    ("catchClosure", "unknown", "any", "any", "unknown"),
    // {} / any / any / unknown
    ("catchTruthy", "{  }", "any", "any", "unknown"),
    // "x" | 0 | 1 / any / any / "x" | 0 | 1
    ("catchEq", "\"x\" | 0 | 1", "any", "any", "\"x\" | 0 | 1"),
];

/// The value of a value-position `=` is its right-hand side's type, fresh
/// literals included (`checkAssignmentOperator` returns `rightType`), never
/// the target's assignment-reduced type: `c ? (x = "s") : 0` is `"s" | 0`
/// where the declared `string | number` target reads `string`, a lone
/// `return (x = "s")` widens to `string`, a `let` or a mutable slot widens
/// the fresh literal, and `as const` keeps it.
#[test]
fn assignment_values_are_their_right_hand_side() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "assignment-values.ts",
        ASSIGNMENT_VALUE_SOURCE,
        ASSIGNMENT_VALUE_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// A write inside an `if` test runs on the checker's flow graph: the test's
/// operands thread as a condition, each edge carrying the writes and
/// narrowings of the operands it ran through, into the consequent (every
/// true edge) and the alternate (every false edge); an operand of a
/// comparison, a `typeof` or a `!` runs before it. The right-hand side of
/// a write no demanded read selects still runs its own writes
/// (`d &&= (x = "s") === "s"`).
#[test]
fn writes_in_conditions_follow_the_checkers_flow_graph() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "condition-writes.ts",
        CONDITION_WRITE_SOURCE,
        CONDITION_WRITE_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// An element access `o[k]` whose key reads a binding is the reference the
/// checker's `isMatchingReference` names: a `const` key of one literal type
/// is the member it spells (`o.k`), a key binding no write reaches is one
/// reference wherever it reads that binding (never `o.k`, whatever its
/// literal type), and a key some write reaches matches no reference. A
/// write to it narrows exactly that reference, reduced against the indexed
/// access of the object's type.
#[test]
fn element_access_references_follow_the_checkers_key_identity() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "element-keys.ts",
        ELEMENT_KEY_SOURCE,
        ELEMENT_KEY_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// A statement call's effects signature is the checker's
/// `getEffectsSignature` over `getTypeOfDottedName`: a callee rooted at a
/// parameter or local without an annotation has no explicit type, and an
/// annotated or module-level dotted callee whose signatures neither assert
/// nor return `never` leaves every narrowing standing — a member write's
/// included.
#[test]
fn statement_calls_keep_narrowings_their_callee_cannot_change() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "statement-calls.ts",
        STATEMENT_CALL_SOURCE,
        STATEMENT_CALL_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Destructured elements narrow as the checker's destructured
/// discriminated unions (`getNarrowedTypeOfSymbol`): in a `const`
/// declaration, a `const` loop pattern or a parameter pattern none of whose
/// bindings is assigned, a pattern of two or more elements over a union
/// correlates its elements that have no default and no rest spread — a
/// test of a discriminant element narrows the pattern's parent and every
/// sibling reads its member of it. A top-level element of an unannotated
/// `const` destructuring of a narrowable reference aliases that
/// reference's member (`const { kind } = o` narrows `o`). A `typeof` test
/// of a discriminant member narrows its parent.
#[test]
fn destructured_discriminants_narrow_their_siblings_and_source() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(&host, "correlated.ts", CORRELATED_SOURCE, CORRELATED_TABLE);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// A `catch` variable's type is its annotation, else `unknown` under
/// `useUnknownInCatchVariables` (which `strict` sets) and `any` without
/// it; an assignment to it narrows nothing, and guards narrow it as any
/// binding.
#[test]
fn catch_variables_follow_use_unknown_in_catch_variables() {
    let host = catch_policy_host();
    let mismatches = matrix_mismatches(&host, "catch.ts", CATCH_SOURCE, CATCH_TABLE);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// A `boolean` return infers a type predicate only when its test narrows a
/// parameter; an element access by a key that names no member (`xs[i]`)
/// narrows no parameter, so it infers none.
#[test]
fn element_reads_infer_no_return_predicate() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "return-predicates.ts",
        RETURN_PREDICATE_SOURCE,
        RETURN_PREDICATE_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Correlated destructured narrowings. Measured on TypeScript 7.0.2, every
/// project: `closureRead` is `string | true` (a closure an arm creates
/// reads the sibling the test narrowed — a `const` capture enters the
/// closure at its narrowed type) and `objSwitch` is `string | true` (a
/// `switch` over a discriminant element narrows its siblings). The lane
/// answers `closureRead` clean and `objSwitch`, which it does not carry,
/// with the typed gap rather than the unnarrowed sibling.
#[test]
fn correlated_narrowings_the_lane_does_not_carry_degrade() {
    let host = four_policy_host();
    let source = "type U = { kind: \"a\"; v: string } | { kind: \"b\"; v: number };\n\
        export function closureRead(o: U) { const { kind, v } = o; if (kind === \"a\") { const f = () => v; return f(); } return true; }\n\
        export function objSwitch(o: U) { const { kind, v } = o; switch (kind) { case \"a\": return v; } return true; }\n";
    for root in [
        STRICT_ROOT,
        LOOSE_ROOT,
        STRICT_IMPLICIT_ROOT,
        LOOSE_IMPLICIT_ROOT,
    ] {
        let canonical = format!("{root}/correlated-gaps.ts");
        upsert(&host, &canonical, source);
        assert_eq!(
            observe(&host, &canonical, "closureRead"),
            "string | true",
            "{canonical} `closureRead`"
        );
        let observed = observe(&host, &canonical, "objSwitch");
        assert!(
            observed.ends_with("[degraded Some(FlowGap(GuardNarrowing))]"),
            "{canonical} `objSwitch`: {observed}"
        );
    }
}

/// The object rest of a parameter with a method member keeps it a METHOD
/// (TypeScript 7.0.2 prints `{ m(x: string): void; }`): the rest's member
/// carries the source member's method kind and signature, as the whole
/// parameter publishes. This suite's spelling prints every method member
/// as a property, the whole parameter's included, so only the print
/// differs from the checker's.
#[test]
fn object_rest_keeps_a_method_member_a_method() {
    let host = four_policy_host();
    let canonical = format!("{STRICT_ROOT}/rest-method.ts");
    upsert(
        &host,
        &canonical,
        "export function objRestMethod(o: { a: number; m(x: string): void }) { const { a, ...r } = o; return r; }\n\
         export function objWhole(o: { a: number; m(x: string): void }) { return o; }\n",
    );
    let graph = host.project_type_store().semantic_graph();
    let member = |symbol: &str| {
        let carrier = host.get_flow_return_type_with_audit(
            &identity(&canonical, symbol),
            ReturnProjectionDemand::whole_return(),
        );
        let result = carrier.as_result().expect("a flow-return value");
        assert!(
            result.degradation().is_none(),
            "{symbol}: {:?}",
            result.degradation()
        );
        let data = graph.node_data(result.return_type()).expect("a live node");
        let SemanticNodeData::Object(surface) = data.as_ref() else {
            panic!("{symbol}: an object return");
        };
        surface
            .positive_members()
            .iter()
            .find(|member| member.key.as_string() == Some("m"))
            .map(|member| (member.method_kind, answer_text(&host, member.value)))
            .expect("the member `m`")
    };
    let rest = member("objRestMethod");
    assert!(rest.0.is_some(), "the rest's `m` is a method: {rest:?}");
    assert_eq!(rest, member("objWhole"));
}

const CORRELATED_INSTANCEOF_SOURCE: &str = r#"
class K { k = 1; }
export function instanceofElem(o: { kind: "a"; v: K } | { kind: "b"; v: string }) { const { kind, v } = o; if (v instanceof K) return kind; return true; }
export function instMember(o: { v: K; k: 1 } | { v: string; k: 2 }) { if (o.v instanceof K) return o.k; return true; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`CORRELATED_INSTANCEOF_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const CORRELATED_INSTANCEOF_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "a" | "b" | true
    (
        "instanceofElem",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
        "\"a\" | \"b\" | true",
    ),
    // 1 | 2 | true
    (
        "instMember",
        "1 | 2 | true",
        "1 | 2 | true",
        "1 | 2 | true",
        "1 | 2 | true",
    ),
];

const LITERAL_ROOT_SOURCE: &str = r#"
export function litProp() { return ({ a: 1 }).a; }
export function litPropStr() { return ({ a: "s", b: 2 }).a; }
export function litGetter() { return ({ get v() { return 1; } }).v; }
export function litGetterAnn() { return ({ get v(): string { return "s"; } }).v; }
export function litMethodCall() { return ({ m() { return true; } }).m(); }
export function litParam(p: string) { return ({ a: p }).a; }
export function litNested() { return ({ o: { x: 1 } }).o.x; }
export function litSpread(o: { a: boolean }) { return ({ ...o, b: 1 }).a; }
export function litOtherMember() { return ({ a: 1, b: "s" }).b; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`LITERAL_ROOT_SOURCE`], each the lane's spelling of the
/// checker's TypeScript 7.0.2 answer (the checker's print follows each
/// row's `//`).
const LITERAL_ROOT_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number
    ("litProp", "number", "number", "number", "number"),
    // string
    ("litPropStr", "string", "string", "string", "string"),
    // number
    ("litGetter", "number", "number", "number", "number"),
    // string
    ("litGetterAnn", "string", "string", "string", "string"),
    // boolean
    ("litMethodCall", "boolean", "boolean", "boolean", "boolean"),
    // string
    ("litParam", "string", "string", "string", "string"),
    // number
    ("litNested", "number", "number", "number", "number"),
    // boolean
    ("litSpread", "boolean", "boolean", "boolean", "boolean"),
    // string
    ("litOtherMember", "string", "string", "string", "string"),
];

const OPEN_ASSIGNMENT_VALUE_SOURCE: &str = r#"
export function assignChain() { let x: string | number = 1; let y: string | number = 2; return (x = y = "s"); }
export function assignCompound() { let x = 1; return (x += 2); }
export function assignCompoundStr() { let x = "a"; return (x += "b"); }
export function assignLogical(c: string | number) { let x = c; return (x ||= "s"); }
export function assignTemplate() { let x: string | number = 1; return `${(x = "s")}`; }
export function assignCall(f: (v: string) => void) { let x: string | number = 1; f((x = "s")); return x; }
export function assignMember(o: { y: string }) { return (o.y = "s"); }
export function assignMemberTernary(o: { y: string }, c: boolean) { return c ? (o.y = "s") : 0; }
export function c11() { let x: string | number = 1; return (x = "s") as "s"; }
export function c13() { let x: string | number = 1; const v = (x = "s"); type T = typeof v; const t: T = "s"; return t; }
export function c19(o: { y: string | number }) { const v = (o.y = "s"); return v; }
export function c20(o: { y: string | number }, c: boolean) { const v = c ? (o.y = "s") : 0; return v; }
export function b10() { let x: string | number = 1; const v = (x = "s"); return v satisfies string; }
export function b12() { let y = "s" as const; let x: string | number = 1; const v = (x = y); return v; }
export function b13() { let x: string | number = 1; return x = "s", x; }
export function capLetObj() { let x; x = 1; return { get v() { return x; } }.v; }
export function litWrite() { let x: string | number = 1; const v = ({ a: (x = "s") }).a; return [v, x]; }
export function litConstAs() { return ({ a: 1 } as const).a; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`OPEN_ASSIGNMENT_VALUE_SOURCE`]: the checker's TypeScript 7.0.2
/// answer in the lane's spelling (the checker's print follows each row's
/// `//`).
const OPEN_ASSIGNMENT_VALUE_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string
    ("assignChain", "string", "string", "string", "string"),
    // number
    ("assignCompound", "number", "number", "number", "number"),
    // string
    ("assignCompoundStr", "string", "string", "string", "string"),
    // string | number
    (
        "assignLogical",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string
    ("assignTemplate", "string", "string", "string", "string"),
    // string
    ("assignCall", "string", "string", "string", "string"),
    // string
    ("assignMember", "string", "string", "string", "string"),
    // "s" | 0
    (
        "assignMemberTernary",
        "\"s\" | 0",
        "\"s\" | 0",
        "\"s\" | 0",
        "\"s\" | 0",
    ),
    // "s"
    ("c11", "\"s\"", "\"s\"", "\"s\"", "\"s\""),
    // "s"
    ("c13", "\"s\"", "\"s\"", "\"s\"", "\"s\""),
    // string
    ("c19", "string", "string", "string", "string"),
    // "s" | 0
    ("c20", "\"s\" | 0", "\"s\" | 0", "\"s\" | 0", "\"s\" | 0"),
    // string
    ("b10", "string", "string", "string", "string"),
    // "s"
    ("b12", "\"s\"", "\"s\"", "\"s\"", "\"s\""),
    // string
    ("b13", "string", "string", "string", "string"),
    // number / number / any / any
    ("capLetObj", "number", "number", "any", "any"),
    // string[]
    ("litWrite", "string[]", "string[]", "string[]", "string[]"),
    // 1
    ("litConstAs", "1", "1", "1", "1"),
];

const OPEN_CONDITION_WRITE_SOURCE: &str = r#"
export function plainWriteIfParam(p: string) { let x: string | number = 1; if ((x = p)) { return x; } return x; }
export function writeWhile(c: boolean) { let x: string | number = 1; while (c && (x = "s")) { return x; } return x; }
export function writeTernaryTest(c: boolean) { let x: string | number = 1; return (c && (x = "s")) ? x : 0; }
export function nullishWriteIf(c: string | null) { let x: string | number = 1; if (c ?? (x = "s")) { return x; } return 0; }
export function forTest() { let x: string | number = 1; for (; (x = "s"); ) { return x; } return x; }
export function doWhileTest(c: boolean) { let x: string | number = 1; do { } while (c && (x = "s")); return x; }
export function cmpValue() { let x: string | number = 1; const b = (x = "s") === "s"; return [b, x]; }
export function cmpRet() { let x: string | number = 1; return (x = "s") === "s"; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`OPEN_CONDITION_WRITE_SOURCE`]: the checker's TypeScript 7.0.2
/// answer in the lane's spelling (the checker's print follows each row's
/// `//`).
const OPEN_CONDITION_WRITE_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string
    ("plainWriteIfParam", "string", "string", "string", "string"),
    // string | number
    (
        "writeWhile",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | 0
    (
        "writeTernaryTest",
        "0 | string",
        "0 | string",
        "0 | string",
        "0 | string",
    ),
    // string | number
    (
        "nullishWriteIf",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string
    ("forTest", "string", "string", "string", "string"),
    // string | number
    (
        "doWhileTest",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // (string | boolean)[]
    (
        "cmpValue",
        "(boolean | string)[]",
        "(boolean | string)[]",
        "(boolean | string)[]",
        "(boolean | string)[]",
    ),
    // boolean
    ("cmpRet", "boolean", "boolean", "boolean", "boolean"),
];

const OPEN_ELEMENT_KEY_SOURCE: &str = r#"
type A = { k: "a" | "b"; j: "a" | "b"; 0: "a" | "b" };
type R = Record<string, string | number>;
declare function key(): "k";
export function recParamKey(r: R, k: string) { r[k] = 1; return r[k]; }
export function recLetKey(r: R) { let k = "x"; r[k] = 1; return r[k]; }
export function recLetKeyWritten(r: R, s: string) { let k = "x"; k = s; r[k] = 1; return r[k]; }
export function aConstKeyDot(a: A) { const k = "k"; a[k] = "b"; return a.k; }
export function recWriteOther(r: R, k: string) { r.x = 1; r[k] = "s"; return r.x; }
export function aParamKeyRead(a: A, k: "k") { if (a[k] === "b") return a[k]; return "z"; }
export function aParamKeyGuardDot(a: A, k: "k") { if (a[k] === "b") return a.k; return "z"; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`OPEN_ELEMENT_KEY_SOURCE`]: the checker's TypeScript 7.0.2
/// answer in the lane's spelling (the checker's print follows each row's
/// `//`).
const OPEN_ELEMENT_KEY_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number
    ("recParamKey", "number", "number", "number", "number"),
    // number
    ("recLetKey", "number", "number", "number", "number"),
    // string | number
    (
        "recLetKeyWritten",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // "b"
    ("aConstKeyDot", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // number
    ("recWriteOther", "number", "number", "number", "number"),
    // "b" | "z"
    (
        "aParamKeyRead",
        "\"b\" | \"z\"",
        "\"b\" | \"z\"",
        "\"b\" | \"z\"",
        "\"b\" | \"z\"",
    ),
    // "a" | "b" | "z"
    (
        "aParamKeyGuardDot",
        "\"a\" | \"b\" | \"z\"",
        "\"a\" | \"b\" | \"z\"",
        "\"a\" | \"b\" | \"z\"",
        "\"a\" | \"b\" | \"z\"",
    ),
];

const OPEN_NARROWING_SOURCE: &str = r#"
type O = { x: 0 | 1; y: "a" | "b" };
declare function assertIsB(v: unknown): asserts v is "b";
declare const obj: { m(): void; n(v: unknown): asserts v is "b" };
export function assertsMethod(o: O) { obj.n(o.y); return o.y; }
type U = { kind: "a"; v: string } | { kind: "b"; v: number };
type T = ["a", string] | ["b", number];
export function paramDestr({ kind, v }: U) { if (kind === "a") return v; return true; }
export function paramDestrAssigned({ kind, v }: U) { if (kind === "a") { return v; } v = 1; return true; }
export function objSwitch(o: U) { const { kind, v } = o; switch (kind) { case "a": return v; } return true; }
export function objGeneric<X extends U>(o: X) { const { kind, v } = o; if (kind === "a") return v; return true; }
type M = { k1: "x"; k2: "p"; v: 1 } | { k1: "y"; k2: "q"; v: 2 } | { k1: "x"; k2: "q"; v: 3 };
type B = { ok: true; v: string } | { ok: false; v: number };
type N = { kind: "s"; v: string } | { kind: 1; v: number };
export function closureRead(o: U) { const { kind, v } = o; if (kind === "a") { const f = () => v; return f(); } return true; }
export function localBool(xs: boolean[]) { const b = xs[0]; return b; }
"#;

/// `(symbol, strict, off, strict without noImplicitAny, off without
/// noImplicitAny)` for [`OPEN_NARROWING_SOURCE`]: the checker's TypeScript 7.0.2
/// answer in the lane's spelling (the checker's print follows each row's
/// `//`).
const OPEN_NARROWING_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "b"
    ("assertsMethod", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // string | true
    (
        "paramDestr",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | number | true
    (
        "paramDestrAssigned",
        "number | string | true",
        "number | string | true",
        "number | string | true",
        "number | string | true",
    ),
    // string | true
    (
        "objSwitch",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true
    (
        "objGeneric",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // string | true
    (
        "closureRead",
        "string | true",
        "string | true",
        "string | true",
        "string | true",
    ),
    // boolean
    ("localBool", "boolean", "boolean", "boolean", "boolean"),
];

/// An `instanceof` test of a destructured element or of a member narrows
/// only the tested reference: its member is no discriminant of the parent
/// (a class instance is not a unit type), so the siblings and the parent
/// keep every arm.
#[test]
fn instanceof_tests_of_non_discriminant_members_narrow_only_themselves() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "correlated-instanceof.ts",
        CORRELATED_INSTANCEOF_SOURCE,
        CORRELATED_INSTANCEOF_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// A member read or a member call directly off an object literal
/// (`({ a: 1 }).a`, `({ get v() { return 1 } }).v`, `({ m() {} }).m()`)
/// reads the literal's member type — a property's widened value, a
/// getter's return — exactly as a read off a named object does.
#[test]
fn member_reads_off_an_object_literal_read_its_member_type() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "literal-roots.ts",
        LITERAL_ROOT_SOURCE,
        LITERAL_ROOT_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Assignment values the lane does not yet read (it degrades them): a
/// chained or compound assignment's value, a write inside a call argument,
/// a template or a comparison, a member write's value, `as const` and
/// `satisfies` over an assignment, a comma operand, an object-literal root
/// holding a write or under `as const`, and a getter reading an auto-typed
/// `let` (the closure-read rule). Each row is the checker's answer.
#[test]
#[ignore = "waits for the checker's value of chained, compound, member and wrapped assignments"]
fn open_assignment_values_match_the_checker() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "open-assignment-values.ts",
        OPEN_ASSIGNMENT_VALUE_SOURCE,
        OPEN_ASSIGNMENT_VALUE_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Condition writes the lane does not yet apply: a write in a loop test or
/// a conditional expression's test, an `if (x = p)` test (which narrows
/// both `x` and `p` on each edge), a `??` test, and a comparison holding a
/// write in value position. Each row is the checker's answer.
#[test]
#[ignore = "waits for writes in loop and conditional-expression tests and in value-position comparisons"]
fn open_condition_writes_match_the_checker() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "open-condition-writes.ts",
        OPEN_CONDITION_WRITE_SOURCE,
        OPEN_CONDITION_WRITE_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Element-access references the lane does not yet read: an index-signature
/// object's declared element (`Record<string, T>`), a guard over an element
/// access by a key binding, and a `const` key naming the dotted member it
/// spells after a keyed write. Each row is the checker's answer.
#[test]
#[ignore = "waits for index-signature element reads and guards over keyed element accesses"]
fn open_element_keys_match_the_checker() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "open-element-keys.ts",
        OPEN_ELEMENT_KEY_SOURCE,
        OPEN_ELEMENT_KEY_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

/// Narrowings the lane does not yet carry: an assertion method call, a flat
/// destructured parameter's correlated elements, a `switch` over a
/// destructured discriminant, a destructured generic parameter, a closure
/// reading a correlated sibling, and a `boolean` return of a local
/// initialized from an element read. Each row is the checker's answer.
#[test]
#[ignore = "waits for assertion methods, flat-parameter and switch correlation, and closure reads of correlated siblings"]
fn open_narrowings_match_the_checker() {
    let host = four_policy_host();
    let mismatches = matrix_mismatches(
        &host,
        "open-narrowings.ts",
        OPEN_NARROWING_SOURCE,
        OPEN_NARROWING_TABLE,
    );
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
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

/// Loops whose written references are independent, or depend on each other
/// along a chain, measured on TypeScript 7.0.2 (`--strict`, with
/// `strictNullChecks` and `noImplicitAny` each off in turn).
const LOOP_HEADS_SOURCE: &str = r#"
export function independentCounters(go: boolean) { let a = 0, b = 0, d = 0, e = 0, f = 0, g = 0, h = 0, i = 0, j = 0, k = 0, l = 0, m = 0; while (go) { a++; b++; d++; e++; f++; g++; h++; i++; j++; k++; l++; m++; } return [a, b, d, e, f, g, h, i, j, k, l, m]; }
export function independentWrites(go: boolean) { let a: number | null = null, b: number | null = null, d: number | null = null, e: number | null = null, f: number | null = null, g: number | null = null, h: number | null = null, i: number | null = null, j: number | null = null, k: number | null = null, l: number | null = null, m: number | null = null; while (go) { a = 1; b = 1; d = 1; e = 1; f = 1; g = 1; h = 1; i = 1; j = 1; k = 1; l = 1; m = 1; } return [a, b, d, e, f, g, h, i, j, k, l, m]; }
export function chain(go: boolean) { let x: string | number | boolean | null = null; let y: string | number | boolean | null = 0; let z: string | number | boolean | null = true; while (go) { x = y; y = z; z = "s"; } return x; }
"#;

const LOOP_HEADS_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number[] / number[] / number[] / number[]
    (
        "independentCounters",
        "number[]",
        "number[]",
        "number[]",
        "number[]",
    ),
    // (number | null)[] / number[] / (number | null)[] / number[]
    (
        "independentWrites",
        "(null | number)[]",
        "number[]",
        "(null | number)[]",
        "number[]",
    ),
    // string | number | true | null / string | number | boolean /
    // string | number | true | null / string | number | boolean
    (
        "chain",
        "null | number | string | true",
        "boolean | number | string",
        "null | number | string | true",
        "boolean | number | string",
    ),
];

/// A loop types each reference it writes at its head once per set of the
/// references under analysis its value can read: the head of a reference
/// no other one feeds takes one pass, and a chain `x = y; y = z; z = "s"`
/// takes one pass per link — `x` reaches `z`'s `"s"` two iterations away
/// (`string | number | true | null`). Twelve independent counters take
/// twelve head passes and the final pass, never a pass per subset of them.
#[test]
fn loop_heads_follow_each_references_dependencies() {
    assert_measured_matrix("loop-heads.ts", LOOP_HEADS_SOURCE, LOOP_HEADS_TABLE);
    let host = four_policy_host();
    let path = format!("{STRICT_ROOT}/loop-heads.ts");
    upsert(&host, &path, LOOP_HEADS_SOURCE);
    for (symbol, passes) in [
        ("independentCounters", 13),
        ("independentWrites", 13),
        ("chain", 4),
    ] {
        let before = super::flow_return::loop_passes_for_tests();
        let _ = observe(&host, &path, symbol);
        assert_eq!(
            super::flow_return::loop_passes_for_tests() - before,
            passes,
            "`{symbol}` evaluates one pass per loop head and one final pass"
        );
    }
}

const CAPTURE_EXTENT_SOURCE: &str = r#"
export function memberWriteAfter(p: { y: number } | null) { let o = p; if (o) { const f = () => o.y; o.y = 2; return f; } return null; }
export function loopExtent(n: number) { let x: string | number = 1; let f = () => 0 as string | number; for (let i = 0; i < n; i++) { x = "s"; if (typeof x === "string") { f = () => x; } } return f; }
export function loopExtentAfter(n: number) { let x: string | number = 1; for (let i = 0; i < n; i++) { x = "s"; } if (typeof x === "string") { return () => x; } return null; }
export function straightWriteBefore() { let x: string | number = 1; x = "s"; if (typeof x === "string") { return () => x; } return null; }
"#;

const CAPTURE_EXTENT_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // (() => number) | null / () => number / (() => number) | null / () => number
    (
        "memberWriteAfter",
        "() => number | null",
        "() => number",
        "() => number | null",
        "() => number",
    ),
    // () => string | number under all four: the capture reads its declared
    // type, which this lane does not supply for a `let` — the typed gap.
    (
        "loopExtent",
        "() => number | string | () => string [degraded Some(FlowGap(ClosureCapture))]",
        "() => number | string | () => string [degraded Some(FlowGap(ClosureCapture))]",
        "() => number | string | () => string [degraded Some(FlowGap(ClosureCapture))]",
        "() => number | string | () => string [degraded Some(FlowGap(ClosureCapture))]",
    ),
    // (() => string) | null / () => string / (() => string) | null / () => string
    (
        "loopExtentAfter",
        "() => string | null",
        "() => string",
        "() => string | null",
        "() => string",
    ),
    // (() => string) | null / () => string / (() => string) | null / () => string
    (
        "straightWriteBefore",
        "() => string | null",
        "() => string",
        "() => string | null",
        "() => string",
    ),
];

/// A closure extends its creator's control-flow container over a `let`
/// past its last assignment (`isPastLastAssignment`), measured on
/// TypeScript 7.0.2: only a write to the binding itself counts — a member
/// write `o.y = 2` after the creation leaves `o` narrowed there — and a
/// write inside a loop counts up to the loop's end
/// (`extendAssignmentPosition`), so a closure created after it in the same
/// loop reads the declared `string | number`, which this lane refuses
/// with the typed capture gap; one created after the loop reads the
/// narrowed `string`.
#[test]
fn closures_extend_past_the_last_assignment_by_its_extent() {
    assert_measured_matrix(
        "capture-extent.ts",
        CAPTURE_EXTENT_SOURCE,
        CAPTURE_EXTENT_TABLE,
    );
}

const TEST_UPDATES_SOURCE: &str = r#"
export function whileTestUpdateExit() { let x = 1 as 1 | 2; while (x++ < 3) { break; } return x; }
export function doTestUpdateExit() { let x = 1 as 1 | 2; do { } while (x++ < 3); return x; }
export function ternaryTestUpdate() { let x = 1 as 1 | 2; let y = 0; (x++ > 0) ? (y = 1) : (y = 2); return x; }
export function andTestUpdate() { let x = 1 as 1 | 2; let y = 0; (x++ > 0) && (y = 1); return x; }
"#;

const TEST_UPDATES_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number under all four
    (
        "whileTestUpdateExit",
        "number",
        "number",
        "number",
        "number",
    ),
    // number under all four
    ("doTestUpdateExit", "number", "number", "number", "number"),
    // number under all four
    ("ternaryTestUpdate", "number", "number", "number", "number"),
    // number under all four
    ("andTestUpdate", "number", "number", "number", "number"),
];

/// An update in a test (`while (x++ < 3)`, `do … while (x++ < 3)`, the
/// test of a conditional or the left operand of `&&` at statement
/// position) runs where the test evaluates, before its condition splits,
/// so every path leaving the test holds the update's `number` — measured
/// on TypeScript 7.0.2, `x` over `let x = 1 as 1 | 2` is `number` past
/// each of them, even where the loop body only breaks.
#[test]
fn test_updates_apply_where_the_test_evaluates() {
    assert_measured_matrix("test-updates.ts", TEST_UPDATES_SOURCE, TEST_UPDATES_TABLE);
}

/// The global declarations TypeScript 7.0.2's libraries make for the
/// members the array tables read, verbatim (`lib.es5.d.ts`, with `find`
/// from `lib.es2015.core.d.ts` and `includes` from
/// `lib.es2016.array.include.d.ts`; measured with `--target es2022`).
const ARRAY_LIB: &str = r#"
interface ConcatArray<T> { readonly length: number; readonly [n: number]: T; join(separator?: string): string; slice(start?: number, end?: number): T[]; }
interface Array<T> {
    length: number;
    pop(): T | undefined;
    push(...items: T[]): number;
    concat(...items: ConcatArray<T>[]): T[];
    concat(...items: (T | ConcatArray<T>)[]): T[];
    join(separator?: string): string;
    slice(start?: number, end?: number): T[];
    indexOf(searchElement: T, fromIndex?: number): number;
    some(predicate: (value: T, index: number, array: T[]) => unknown, thisArg?: any): boolean;
    map<U>(callbackfn: (value: T, index: number, array: T[]) => U, thisArg?: any): U[];
    filter<S extends T>(predicate: (value: T, index: number, array: T[]) => value is S, thisArg?: any): S[];
    filter(predicate: (value: T, index: number, array: T[]) => unknown, thisArg?: any): T[];
    find<S extends T>(predicate: (value: T, index: number, obj: T[]) => value is S, thisArg?: any): S | undefined;
    find(predicate: (value: T, index: number, obj: T[]) => unknown, thisArg?: any): T | undefined;
    includes(searchElement: T, fromIndex?: number): boolean;
    [n: number]: T;
}
interface ReadonlyArray<T> { readonly length: number; readonly [n: number]: T; }
interface String { readonly length: number; readonly [index: number]: string; }
interface StringConstructor { new (value?: any): String; (value?: any): string; readonly prototype: String; }
declare var String: StringConstructor;
"#;

/// [`assert_measured_matrix`] with `lib` registered as every project's
/// ambient library.
fn assert_measured_matrix_with_lib(
    file: &str,
    source: &str,
    lib: &str,
    table: &[(&str, &str, &str, &str, &str)],
) {
    let host = four_policy_host();
    for ordinal in 0..4u32 {
        host.workspace()
            .register_ambient_lib(verter_workspace::AmbientLibSpec {
                project_id: Some(verter_workspace::workspace_snapshot::ProjectId(ordinal)),
                canonical_id: std::sync::Arc::from(format!("lib.array{ordinal}.d.ts").as_str()),
                source: std::sync::Arc::from(lib),
            })
            .expect("the library registers against its project");
    }
    let mismatches = matrix_mismatches(&host, file, source, table);
    assert!(
        mismatches.is_empty(),
        "flow-return answers differ from the measured TypeScript 7.0.2 matrix:\n{}",
        mismatches.join("\n")
    );
}

const LITERAL_UNION_SOURCE: &str = r#"
declare const c: boolean;
declare function num(): number;
export function assertSingle() { let x = "a" as "a"; return x; }
export function assertSingleAngle() { let x = <1>1; return x; }
export function assertSingleObj() { let x = "a" as "a"; return { x }; }
export function assignLit() { let x = 0 as 0 | 1 | 2; x = 1; return x; }
export function assignLitNum() { let x = 0 as 0 | 1 | 2; x = num() as 0 | 1; return x; }
export function assignLitDecl() { let x: 0 | 1 | 2 = 0; x = 1; return x; }
export function assignLitBool() { let x = true as boolean; x = false; return x; }
export function assignLitStrUnion() { let x = "a" as "a" | "b"; x = "b"; return x; }
export function assignWide() { let x = 0 as number; x = 1; return x; }
export function assignConstAssert() { let x = 0 as const; return x; }
export function assignSat() { let x = 0 satisfies number; x = 1; return x; }
export function assignAngle() { let x = <0 | 1 | 2>0; x = 2; return x; }
export function assignParenAs() { let x = (0 as 0 | 1 | 2); x = 1; return x; }
export function counterForLit() { let x = 0 as 0 | 1 | 2; for (let i = 0; i < 3; i++) { x = 1; } return x; }
export function counterWhileLit() { let x = 0 as 0 | 1 | 2; while (c) { x = 2; } return x; }
export function counterIfLit() { let x = 0 as 0 | 1 | 2; if (c) { x = 2; } return x; }
export function counterLoopRead() { let x = 0 as 0 | 1 | 2; let y = x; while (c) { y = x; x = 1; } return y; }
export function assignNonUnionFromUnion() { let x = 0 as 0 | 1 | 2; x = (c ? 1 : 2); return x; }
export function assignMismatch() { let x = 0 as 0 | 1 | 2; x = 5 as any; return x; }
export function incLitUnion() { let x = 0 as 0 | 1 | 2; x++; return x; }
export function inductionLiteral(n: number) { let i = 0 as 0 | 1; while (i < n) { i++; } return i; }
export function inductionLiteralBody(n: number) { let i = 0 as 0 | 1; while (i < n) { i++; if (c) return i; } return null; }
export function paramLoopCompound(p: 0 | 1) { while (c) { p++; } return p; }
export function letVarUnionInit() { var v = 0 as 0 | 1; v = 1; return v; }
export function condInit() { let x = c ? 1 : "a"; x = 1; return x; }
"#;

const LITERAL_UNION_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // "a" under all four
    ("assertSingle", "\"a\"", "\"a\"", "\"a\"", "\"a\""),
    // 1 under all four
    ("assertSingleAngle", "1", "1", "1", "1"),
    // { x: "a"; } under all four
    (
        "assertSingleObj",
        "{ x: \"a\" }",
        "{ x: \"a\" }",
        "{ x: \"a\" }",
        "{ x: \"a\" }",
    ),
    // 1 under all four
    ("assignLit", "1", "1", "1", "1"),
    // 0 | 1 under all four
    ("assignLitNum", "0 | 1", "0 | 1", "0 | 1", "0 | 1"),
    // 1 under all four
    ("assignLitDecl", "1", "1", "1", "1"),
    // boolean under all four
    ("assignLitBool", "boolean", "boolean", "boolean", "boolean"),
    // "b" under all four
    ("assignLitStrUnion", "\"b\"", "\"b\"", "\"b\"", "\"b\""),
    // number under all four
    ("assignWide", "number", "number", "number", "number"),
    // 0 under all four
    ("assignConstAssert", "0", "0", "0", "0"),
    // number under all four
    ("assignSat", "number", "number", "number", "number"),
    // 2 under all four
    ("assignAngle", "2", "2", "2", "2"),
    // 1 under all four
    ("assignParenAs", "1", "1", "1", "1"),
    // 0 | 1 | 2 under all four
    (
        "counterForLit",
        "0 | 1 | 2",
        "0 | 1 | 2",
        "0 | 1 | 2",
        "0 | 1 | 2",
    ),
    // 0 | 1 | 2 under all four
    (
        "counterWhileLit",
        "0 | 1 | 2",
        "0 | 1 | 2",
        "0 | 1 | 2",
        "0 | 1 | 2",
    ),
    // 0 | 1 | 2 under all four
    (
        "counterIfLit",
        "0 | 1 | 2",
        "0 | 1 | 2",
        "0 | 1 | 2",
        "0 | 1 | 2",
    ),
    // 0 | 1 | 2 under all four
    (
        "counterLoopRead",
        "0 | 1 | 2",
        "0 | 1 | 2",
        "0 | 1 | 2",
        "0 | 1 | 2",
    ),
    // 1 | 2 under all four
    (
        "assignNonUnionFromUnion",
        "1 | 2",
        "1 | 2",
        "1 | 2",
        "1 | 2",
    ),
    // 0 | 1 | 2 under all four
    (
        "assignMismatch",
        "0 | 1 | 2",
        "0 | 1 | 2",
        "0 | 1 | 2",
        "0 | 1 | 2",
    ),
    // number under all four
    ("incLitUnion", "number", "number", "number", "number"),
    // 0 | 1 under all four
    ("inductionLiteral", "0 | 1", "0 | 1", "0 | 1", "0 | 1"),
    // number | null / number / number | null / number
    (
        "inductionLiteralBody",
        "null | number",
        "number",
        "null | number",
        "number",
    ),
    // 0 | 1 under all four
    ("paramLoopCompound", "0 | 1", "0 | 1", "0 | 1", "0 | 1"),
    // 1 under all four
    ("letVarUnionInit", "1", "1", "1", "1"),
    // number under all four
    ("condInit", "number", "number", "number", "number"),
];

/// An unannotated `let` / `var` is declared as its initializer's widened
/// type, and a type assertion's type is not fresh, so it does not widen:
/// `let x = 0 as 0 | 1 | 2` declares `0 | 1 | 2`, and `let x = "a" as "a"`
/// reads `"a"`. Each assignment reduces against a union declared type
/// (`getAssignmentReducedType`): `x = 1` leaves `1`, and
/// a value no constituent can hold leaves the whole union. A loop head where
/// the reference enters at its declared type stays that type, so `i++` in
/// `while (i < n)` over `let i = 0 as 0 | 1` leaves `0 | 1` past the loop
/// while the body reads `number` after the update. Measured on TypeScript
/// 7.0.2 in all four `strictNullChecks` × `noImplicitAny` settings.
#[test]
fn assignments_reduce_against_an_asserted_declared_type() {
    assert_measured_matrix(
        "literal-union.ts",
        LITERAL_UNION_SOURCE,
        LITERAL_UNION_TABLE,
    );
}

const INDUCTION_LOOPS_SOURCE: &str = r#"
export function inductionOnly(n: number) { let i = 0; for (; i < n; i++) { } return i; }
export function inductionOnlyWhile(n: number) { let i = 0; while (i < n) { i++; } return i; }
export function inductionLiteral(n: number) { let i = 0 as 0 | 1; while (i < n) { i++; } return i; }
export function inductionFor(n: number) { let k = 0; for (let i = 0; i < n; i++) { k = i; } return k; }
export function inductionReturn(n: number) { for (let i = 0; i < n; i++) { if (i > 3) return i; } return null; }
"#;

const INDUCTION_LOOPS_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number under all four
    ("inductionOnly", "number", "number", "number", "number"),
    // number under all four
    ("inductionOnlyWhile", "number", "number", "number", "number"),
    // 0 | 1 under all four
    ("inductionLiteral", "0 | 1", "0 | 1", "0 | 1", "0 | 1"),
    // number under all four
    ("inductionFor", "number", "number", "number", "number"),
    // number | null / number / number | null / number
    (
        "inductionReturn",
        "null | number",
        "number",
        "null | number",
        "number",
    ),
];

/// A loop whose only write is its induction variable (`i++` in a `for` update
/// or a `while` body) types that variable at the head like any other written
/// reference: `let i = 0` reads `number` past the loop and inside it, and one
/// declared `0 | 1` stays `0 | 1`. Measured on TypeScript 7.0.2.
#[test]
fn loops_writing_only_their_induction_variable_take_the_loop_head() {
    assert_measured_matrix(
        "induction-loops.ts",
        INDUCTION_LOOPS_SOURCE,
        INDUCTION_LOOPS_TABLE,
    );
}

const ARRAY_MEMBERS_SOURCE: &str = r#"
export function evLength() { const a = []; a.push(1); return a.length; }
export function evLengthEmpty() { const a = []; return a.length; }
export function evLengthBetween() { const a = []; const n = a.length; a.push(1); return [n, a]; }
export function evIndex() { const a = []; a.push(1); return a[0]; }
export function evIndexEmpty() { const a = []; return a[0]; }
export function evFilter() { const a = []; a.push(1); a.push("s"); return a.filter(x => x); }
export function evJoin() { const a = []; a.push(1); return a.join(","); }
export function evSlice() { const a = []; a.push(1); return a.slice(); }
export function evPop() { const a = []; a.push(1); return a.pop(); }
export function evFind() { const a = []; a.push(1); return a.find(x => x > 0); }
export function evSome() { const a = []; a.push(1); return a.some(x => x > 0); }
export function decLength(a: number[]) { return a.length; }
export function decIndex(a: number[]) { return a[0]; }
export function decFilter(a: (string | number)[]) { return a.filter(x => x); }
export function decJoin(a: number[]) { return a.join(","); }
export function decIndexOf(a: number[]) { return a.indexOf(1); }
export function decSlice(a: number[]) { return a.slice(); }
export function decPop(a: number[]) { return a.pop(); }
export function decFind(a: number[]) { return a.find(x => x > 0); }
export function decLocalLength() { const a: number[] = [1]; return a.length; }
export function decReadonlyLength(a: readonly string[]) { return a.length; }
export function decReadonlyIndex(a: readonly string[]) { return a[0]; }
export function decTupleLength(t: [number, string]) { return t.length; }
export function tupleIndex(t: [number, string]) { return t[1]; }
export function localTupleIndex() { const t = [1, "a"] as const; return t[0]; }
"#;

const ARRAY_MEMBERS_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number under all four
    ("evLength", "number", "number", "number", "number"),
    // number under all four
    ("evLengthEmpty", "number", "number", "number", "number"),
    // (number | number[])[] / (number | number[])[] / (number | never[])[] / (number | any[])[]
    (
        "evLengthBetween",
        "(number | number[])[]",
        "(number | number[])[]",
        "(never[] | number)[]",
        "(any[] | number)[]",
    ),
    // number / number / never / any
    ("evIndex", "number", "number", "never", "any"),
    // any / any / never / any
    ("evIndexEmpty", "any", "any", "never", "any"),
    // (string | number)[] / (string | number)[] / never[] / any[]
    (
        "evFilter",
        "(number | string)[]",
        "(number | string)[]",
        "never[]",
        "any[]",
    ),
    // string under all four
    ("evJoin", "string", "string", "string", "string"),
    // number[] / number[] / never[] / any[]
    ("evSlice", "number[]", "number[]", "never[]", "any[]"),
    // number | undefined / number / undefined / any
    ("evPop", "number | undefined", "number", "undefined", "any"),
    // number | undefined / number / undefined / any
    ("evFind", "number | undefined", "number", "undefined", "any"),
    // boolean under all four
    ("evSome", "boolean", "boolean", "boolean", "boolean"),
    // number under all four
    ("decLength", "number", "number", "number", "number"),
    // number under all four
    ("decIndex", "number", "number", "number", "number"),
    // (string | number)[] under all four
    (
        "decFilter",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
        "(number | string)[]",
    ),
    // string under all four
    ("decJoin", "string", "string", "string", "string"),
    // number under all four
    ("decIndexOf", "number", "number", "number", "number"),
    // number[] under all four
    ("decSlice", "number[]", "number[]", "number[]", "number[]"),
    // number | undefined / number / number | undefined / number
    (
        "decPop",
        "number | undefined",
        "number",
        "number | undefined",
        "number",
    ),
    // number | undefined / number / number | undefined / number
    (
        "decFind",
        "number | undefined",
        "number",
        "number | undefined",
        "number",
    ),
    // number under all four
    ("decLocalLength", "number", "number", "number", "number"),
    // number under all four
    ("decReadonlyLength", "number", "number", "number", "number"),
    // string under all four
    ("decReadonlyIndex", "string", "string", "string", "string"),
    // 2 under all four
    ("decTupleLength", "2", "2", "2", "2"),
    // string under all four
    ("tupleIndex", "string", "string", "string", "string"),
    // 1 under all four
    ("localTupleIndex", "1", "1", "1", "1"),
];

/// A member read off an array, a readonly array or a tuple reads the global
/// `Array` / `ReadonlyArray` wrapper's member (`getApparentType`), and an
/// element read at an integer literal reads the element (a tuple's position):
/// an EVOLVING array finalizes where it is read, so `a.length` and `a[0]`
/// over `const a = []; a.push(1)` read `number`, `a.pop()` `number |
/// undefined` (`number` with `strictNullChecks` off), and `a.filter(x => x)`
/// over a `string | number` array `(string | number)[]`; without
/// `noImplicitAny` the array is declared `never[]` (`any[]` with
/// `strictNullChecks` off). Measured on TypeScript 7.0.2 (`--target es2022`),
/// the table's comments in the checker's print order.
#[test]
fn array_members_read_through_the_apparent_wrapper() {
    assert_measured_matrix_with_lib(
        "array-members.ts",
        ARRAY_MEMBERS_SOURCE,
        ARRAY_LIB,
        ARRAY_MEMBERS_TABLE,
    );
}

const ARRAY_MAP_SOURCE: &str = r#"
export function evMap() { const a = []; a.push(1); return a.map(x => x); }
export function evMapString() { const a = []; a.push(1); return a.map(x => String(x)); }
export function decMap(a: number[]) { return a.map(x => x); }
export function decMapString(a: number[]) { return a.map(x => String(x)); }
"#;

const ARRAY_MAP_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number[] / number[] / never[] / any[]
    ("evMap", "number[]", "number[]", "never[]", "any[]"),
    // string[] under all four
    (
        "evMapString",
        "string[]",
        "string[]",
        "string[]",
        "string[]",
    ),
    // number[] under all four
    ("decMap", "number[]", "number[]", "number[]", "number[]"),
    // string[] under all four
    (
        "decMapString",
        "string[]",
        "string[]",
        "string[]",
        "string[]",
    ),
];

/// `map<U>` infers `U` from its callback's return, the callback's parameter
/// contextually typed by the array's element: `a.map(x => x)` over
/// `number[]` is `number[]` and `a.map(x => String(x))` `string[]`
/// (measured on TypeScript 7.0.2). The lane answers `unknown[]`.
#[test]
#[ignore = "call inference from a contextually typed callback return: `a.map(x => x)` over `number[]` is `number[]`"]
fn array_map_infers_its_callback_return() {
    assert_measured_matrix_with_lib("array-map.ts", ARRAY_MAP_SOURCE, ARRAY_LIB, ARRAY_MAP_TABLE);
}

const ARRAY_CONCAT_SOURCE: &str = r#"
export function evConcat() { const a = []; a.push(1); return a.concat([2]); }
export function decConcat(a: number[]) { return a.concat([2]); }
"#;

const ARRAY_CONCAT_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number[] / number[] / never[] / any[]
    ("evConcat", "number[]", "number[]", "never[]", "any[]"),
    // number[] under all four
    ("decConcat", "number[]", "number[]", "number[]", "number[]"),
];

/// `a.concat([2])` over `number[]` resolves `concat`'s first overload and
/// is `number[]` (measured on TypeScript 7.0.2). The lane refuses the callee.
#[test]
#[ignore = "overload resolution of `concat(...items: ConcatArray<T>[])` over an array literal argument"]
fn array_concat_resolves_its_overloads() {
    assert_measured_matrix_with_lib(
        "array-concat.ts",
        ARRAY_CONCAT_SOURCE,
        ARRAY_LIB,
        ARRAY_CONCAT_TABLE,
    );
}

const NON_ASSIGNABLE_ARGUMENT_SOURCE: &str = r#"
export function evIndexOf() { const a = []; a.push(1); return a.indexOf(1); }
export function evIncludes() { const a = []; a.push(1); return a.includes(1); }
"#;

const NON_ASSIGNABLE_ARGUMENT_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // number under all four
    ("evIndexOf", "number", "number", "number", "number"),
    // boolean under all four
    ("evIncludes", "boolean", "boolean", "boolean", "boolean"),
];

/// Without `noImplicitAny` an evolving array is `never[]`, so `a.indexOf(1)`
/// passes an argument `never` cannot hold (TS2345), and the checker still
/// types the call by its only signature: `number`, and `boolean` for
/// `includes` (measured on TypeScript 7.0.2). The lane refuses the callee in
/// that setting.
#[test]
#[ignore = "a call whose argument is not assignable still takes the return type of its only signature"]
fn a_call_with_a_non_assignable_argument_keeps_its_only_signature() {
    assert_measured_matrix_with_lib(
        "non-assignable-argument.ts",
        NON_ASSIGNABLE_ARGUMENT_SOURCE,
        ARRAY_LIB,
        NON_ASSIGNABLE_ARGUMENT_TABLE,
    );
}

const BOOLEAN_MEMBER_RETURNS_SOURCE: &str = r#"
export function decIncludes(a: number[]) { return a.includes(1); }
export function decSome(a: number[]) { return a.some(x => x > 0); }
export function nestedIndex(o: { xs: boolean[] }) { return o.xs[0]; }
"#;

const BOOLEAN_MEMBER_RETURNS_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // boolean under all four
    ("decIncludes", "boolean", "boolean", "boolean", "boolean"),
    // boolean under all four
    ("decSome", "boolean", "boolean", "boolean", "boolean"),
    // boolean under all four
    ("nestedIndex", "boolean", "boolean", "boolean", "boolean"),
];

/// `return a.includes(1)`, `return a.some(x => x > 0)` and `return o.xs[0]`
/// read a parameter's member and are `boolean`: the checker infers no type
/// predicate from them (measured on TypeScript 7.0.2). The lane answers
/// `boolean` degraded by the guard-narrowing gap.
#[test]
#[ignore = "a boolean member call or element read of a parameter infers no type predicate"]
fn boolean_member_reads_of_a_parameter_infer_no_type_predicate() {
    assert_measured_matrix_with_lib(
        "boolean-member-returns.ts",
        BOOLEAN_MEMBER_RETURNS_SOURCE,
        ARRAY_LIB,
        BOOLEAN_MEMBER_RETURNS_TABLE,
    );
}

const STRING_ELEMENT_SOURCE: &str = r#"
export function strIndex(s: string) { return s[0]; }
"#;

const STRING_ELEMENT_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string under all four
    ("strIndex", "string", "string", "string", "string"),
];

/// `s[0]` over `s: string` is `string`, the `String` wrapper's
/// `[index: number]: string` (measured on TypeScript 7.0.2). The lane misses
/// the read.
#[test]
#[ignore = "an element read of a string reads the `String` wrapper's number index signature"]
fn a_string_element_read_reads_the_string_index_signature() {
    assert_measured_matrix_with_lib(
        "string-element.ts",
        STRING_ELEMENT_SOURCE,
        ARRAY_LIB,
        STRING_ELEMENT_TABLE,
    );
}

const INVOKED_WRITES_SOURCE: &str = r#"
declare const c: boolean;
export function iifeStraight() { let x: string | number = 0; (() => { x = "s"; })(); return x; }
export function iifeCond() { let x: string | number = 0; if (c) { (() => { x = "s"; })(); } return x; }
export function iifeFnExpr() { let x: string | number = 0; (function () { x = "s"; })(); return x; }
export function iifeLoop(n: number) { let x: string | number = 0; for (let i = 0; i < n; i++) { (() => { x = "s"; })(); } return x; }
export function iifeLoopWhile() { let x: string | number = 0; while (c) { (function () { x = "s"; })(); } return x; }
export function iifeLoopRead(n: number) { let x: string | number = 0; let r = 0 as string | number; for (let i = 0; i < n; i++) { r = x; (() => { x = "s"; })(); } return r; }
"#;

const INVOKED_WRITES_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string under all four
    ("iifeStraight", "string", "string", "string", "string"),
    // string | number under all four
    (
        "iifeCond",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string under all four
    ("iifeFnExpr", "string", "string", "string", "string"),
    // string | number under all four
    (
        "iifeLoop",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number under all four
    (
        "iifeLoopWhile",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
    // string | number under all four
    (
        "iifeLoopRead",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
];

/// The checker's flow runs through an immediately invoked function
/// expression's body, so its writes to a captured binding reach the reads
/// after the call: `(() => { x = "s"; })()` over `let x: string | number =
/// 0` leaves `string`, in a loop `string | number` (measured on TypeScript
/// 7.0.2). The lane refuses the function.
#[test]
#[ignore = "an immediately invoked function expression's writes to captured bindings enter the caller's flow"]
fn invoked_function_expressions_apply_their_writes() {
    assert_measured_matrix(
        "invoked-writes.ts",
        INVOKED_WRITES_SOURCE,
        INVOKED_WRITES_TABLE,
    );
}

const MEMBER_WRITES_SOURCE: &str = r#"
declare const c: boolean;
export function memberWriteStraight() { const o: { y: string | number } = { y: 0 }; o.y = "s"; return o.y; }
export function memberWriteLoop() { const o: { y: string | number | boolean } = { y: 0 }; o.y = 0; while (c) { o.y = "s"; } return o.y; }
"#;

const MEMBER_WRITES_TABLE: &[(&str, &str, &str, &str, &str)] = &[
    // string under all four
    (
        "memberWriteStraight",
        "string",
        "string",
        "string",
        "string",
    ),
    // string | number under all four
    (
        "memberWriteLoop",
        "number | string",
        "number | string",
        "number | string",
        "number | string",
    ),
];

/// An assignment `o.y = "s"` narrows the reference `o.y` for the reads after
/// it: `string` straight after it, and `string | number` past a loop that
/// assigns it over `o.y = 0` (measured on TypeScript 7.0.2). The lane reads the
/// member's declared type.
#[test]
#[ignore = "an assignment to a member path narrows later reads of that path, in and outside loops"]
fn member_writes_narrow_their_member_path() {
    assert_measured_matrix(
        "member-writes.ts",
        MEMBER_WRITES_SOURCE,
        MEMBER_WRITES_TABLE,
    );
}
