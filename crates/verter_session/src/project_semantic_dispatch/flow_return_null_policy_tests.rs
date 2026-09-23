//! Flow-return inference under the function's OWN `strictNullChecks`.
//!
//! Every expected answer here was measured on the pinned TypeScript 7.0.2
//! (`tsc --declaration --emitDeclarationOnly --strict`, once as is and once
//! with `--strictNullChecks false`, reading the emitted `.d.ts`). The TS
//! print of each row is quoted beside it; the table itself spells the
//! answer in [`answer_text`]'s form, which sorts union members so the
//! comparison is independent of union member order.

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
/// declaration order, a signature as `(name?: type) => return`.
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
                        "{}{}: {}",
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
            format!(
                "({}) => {}",
                params.join(", "),
                answer_text(host, *return_type)
            )
        }
        other => format!("<unrendered {other:?}>"),
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
