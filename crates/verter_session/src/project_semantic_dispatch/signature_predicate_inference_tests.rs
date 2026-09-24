//! Type predicates the checker INFERS from a function body: a function
//! with no return annotation whose single returned expression is a
//! `boolean` narrowing a parameter publishes `x is T` beside its `boolean`
//! return (`getTypePredicateFromBody`).
//!
//! Every expected answer is measured on the pinned TypeScript 7.0.2
//! (`tsc --declaration --emitDeclarationOnly --strict`, and once more with
//! `--strictNullChecks false` for the loose table). Each row quotes the
//! checker's print of the function's type; the flow lane answers it
//! through a wrapper returning the function value.

use std::sync::Arc;

use crate::project_semantic_dispatch::ProjectSemanticDispatch;
use crate::semantic_query::{
    FlowReturnDegradation, PredicateSubject, PrimitiveKind, ReturnProjectionDemand,
    SemanticNodeData, SemanticNodeId,
};
use crate::types::HostConfig;
use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
use crate::VerterHost;

const STRICT_ROOT: &str = "/strict";
const LOOSE_ROOT: &str = "/loose";

/// One host carrying two projects that differ ONLY in `strictNullChecks`.
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

/// The flow-return identity of `symbol`: a `const` initializer's function
/// is served at its initializer position, every other function at its
/// declaration body.
fn identity(canonical: &str, symbol: &str) -> verter_type_expr::facts::FlowFunctionReturnIdentity {
    let function_part = if SOURCE.contains(&format!("export const {symbol} =")) {
        verter_type_expr::facts::FunctionPartIdentity::Initializer
    } else {
        verter_type_expr::facts::FunctionPartIdentity::DeclarationBody
    };
    verter_type_expr::facts::FlowFunctionReturnIdentity {
        anchor: verter_type_expr::locators::AuthoredAnchor {
            canonical_id: Arc::from(canonical),
            owner: verter_type_expr::TopLevelOwnerId::ordinary_file(),
            symbol: Arc::from(symbol),
            space: verter_type_expr::locators::LocatorSymbolSpace::Value,
        },
        function_part,
        overload_ordinal: 0,
    }
}

/// A host whose two projects both hold `source` as `main.ts`.
fn host_with(source: &str) -> VerterHost {
    let host = two_policy_host();
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        crate::u6_flow_shape_corpus_tests::upsert(
            &host,
            &format!("{root}/main.ts"),
            source,
            crate::FileLanguage::script(verter_language::ScriptSourceType::Ts),
        );
    }
    host
}

/// The whole-return answer of `symbol` in `root`'s copy through the public
/// audited flow-return boundary: its degradation and the live node, handed
/// to `read` while the graph is pinned.
fn observe<R>(
    host: &VerterHost,
    root: &str,
    symbol: &str,
    read: impl FnOnce(&ProjectSemanticDispatch<'_>, Option<FlowReturnDegradation>, SemanticNodeId) -> R,
) -> R {
    let carrier = host.get_flow_return_type_with_audit(
        &identity(&format!("{root}/main.ts"), symbol),
        ReturnProjectionDemand::whole_return(),
    );
    let result = carrier
        .as_result()
        .unwrap_or_else(|error| panic!("`{symbol}` in {root} produced no value: {error:?}"));
    let (degradation, node) = (result.degradation(), result.return_type());
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    read(&dispatch, degradation, node)
}

/// `symbol`'s answer in `root` is COMPLETE and is the checker's `printed`
/// function type — predicate included, or its absence.
fn assert_prints(host: &VerterHost, root: &str, symbol: &str, printed: &str) {
    let expected = checker_syntax::parse(printed).expect("checker print parses");
    observe(host, root, symbol, |dispatch, degradation, node| {
        assert!(
            degradation.is_none() && checker_syntax::matches_node(dispatch, node, &expected, 0),
            "`{symbol}` in {root} is `{printed}` on 7.0.2; measured `{}` (degradation {degradation:?})",
            render_node(dispatch, node, 0)
        );
    });
}

/// `function`'s OWN answer in `root` DEGRADES: the checker may infer a
/// predicate the flow lane cannot compute, so no predicate-less signature
/// publishes complete.
fn assert_degrades(host: &VerterHost, root: &str, function: &str) {
    observe(host, root, function, |dispatch, degradation, node| {
        assert!(
            degradation.is_some(),
            "`{function}` in {root} must degrade; measured `{}` complete",
            render_node(dispatch, node, 0)
        );
    });
}

/// `function`'s OWN answer in `root` is complete: a wrapper reading the
/// function's signature does not inherit the function's own degradation.
fn assert_complete(host: &VerterHost, root: &str, function: &str) {
    observe(host, root, function, |dispatch, degradation, node| {
        assert!(
            degradation.is_none(),
            "`{function}` in {root} must be complete; measured `{}` (degradation {degradation:?})",
            render_node(dispatch, node, 0)
        );
    });
}

/// The lone call signature of a function value.
fn lone_signature(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> SemanticNodeId {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Object(surface)) if surface.call_signatures.len() == 1 => {
            surface.call_signatures[0]
        }
        _ => node,
    }
}

/// `symbol`'s answer in `root` is complete and carries exactly the
/// predicate of the checker's `printed` function type (or none) beside a
/// `boolean` return. Its parameter types are not compared.
fn assert_predicate(host: &VerterHost, root: &str, symbol: &str, printed: &str) {
    use checker_syntax::{CheckerPredicate, CheckerPredicateSubject, CheckerType};
    let CheckerType::Function {
        predicate: expected,
        ..
    } = checker_syntax::parse(printed).expect("checker print parses")
    else {
        panic!("`{printed}` is a function print");
    };
    observe(host, root, symbol, |dispatch, degradation, node| {
        let signature = lone_signature(dispatch, node);
        let graph = dispatch.graph();
        let matches = match graph.node_data(signature).as_deref() {
            Some(SemanticNodeData::Signature {
                return_type,
                predicate,
                ..
            }) => {
                let boolean = graph.node_data(*return_type).as_deref()
                    == Some(&SemanticNodeData::Primitive(PrimitiveKind::Boolean));
                boolean
                    && match (predicate, &expected) {
                        (None, None) => true,
                        (Some(live), Some(CheckerPredicate::TypePredicate { subject, ty })) => {
                            !live.asserts
                                && match (live.subject, subject) {
                                    (
                                        PredicateSubject::Parameter(index),
                                        CheckerPredicateSubject::Parameter(expected),
                                    ) => index as usize == *expected,
                                    (PredicateSubject::This, CheckerPredicateSubject::This) => true,
                                    _ => false,
                                }
                                && live.ty.is_some_and(|target| {
                                    checker_syntax::matches_node(dispatch, target, ty, 0)
                                })
                        }
                        _ => false,
                    }
            }
            _ => false,
        };
        assert!(
            degradation.is_none() && matches,
            "`{symbol}` in {root} carries the predicate of `{printed}` on 7.0.2; measured `{}` (degradation {degradation:?})",
            render_node(dispatch, node, 0)
        );
    });
}

/// The inference source: every function under test, and a wrapper per
/// function (`sig_NAME`) whose flow return is the function value.
const SOURCE: &str = r#"
interface Foo { kind: 'foo'; n: number }
interface Bar { kind: 'bar'; s: string }
class Cls { c = 1 }
function isFoo(x: unknown): x is Foo { return true; }
export function isStr(x: unknown) { return typeof x === "string"; }
export const isStrArrow = (x: unknown) => typeof x === "string";
export const isStrFnExpr = function (x: unknown) { return typeof x === "string"; };
export function notNull(x: string | null) { return x !== null; }
export function notUndef(x: string | undefined) { return x !== undefined; }
export function isCls(x: Cls | string) { return x instanceof Cls; }
export function hasKind(x: Foo | Bar | number) { return typeof x === "object" && "s" in x; }
export function inOp(x: Foo | Bar) { return "n" in x; }
export function isFooDisc(x: Foo | Bar) { return x.kind === 'foo'; }
export function discNot(x: Foo | Bar) { return x.kind !== 'foo'; }
export function discAnd(x: Foo | Bar | null) { return x !== null && x.kind === 'bar'; }
export function andBoth(x: string | number | null) { return x !== null && typeof x === "string"; }
export function orBoth(x: string | number | boolean) { return typeof x === "string" || typeof x === "number"; }
export function orIn(x: Foo | Bar | number) { return typeof x === "number" || "n" in x; }
export function isNum(x: string | number) { return typeof x === "number"; }
export function notStr(x: string | number) { return typeof x !== "string"; }
export function twoParams(x: unknown, y: unknown) { return typeof y === "string"; }
export function secondOnly(x: boolean, y: string | number) { return typeof y === "string"; }
export function boolFirst(x: boolean | string) { return x === true; }
export function eqLiteral(x: 'a' | 'b' | 'c') { return x === 'a'; }
export function eqNumber(x: number | string) { return x === 1; }
export function numLitEq(x: 1 | 2 | 3) { return x === 2; }
export function anyParam(x: any) { return typeof x === "string"; }
export function parenthesized(x: unknown) { return (typeof x === "string"); }
export function notNot(x: unknown) { return !(typeof x !== "string"); }
export function wrapPred(x: unknown) { return isFoo(x); }
export function isFooExported(x: unknown): x is Foo { return true; }
export function wrapExported(x: unknown) { return isFooExported(x); }
export function withThis(this: unknown, x: unknown) { return typeof x === "string"; }
export function narrowedBefore(x: string | number | null) { if (x === null) throw 0; return typeof x === "string"; }
export function memberWrite(x: { a: number } | null) { if (x) x.a = 1; return x !== null; }
export function throwAfter(x: unknown) { return typeof x === "string"; throw 1; }
export function nestedFnDecl(x: unknown) { function inner() { return 1; } return typeof x === "string"; }
export function numKey(x: { a: string } | { b: number }) { return "a" in x; }
export function multiReturn(x: unknown) { if (x) { return typeof x === "string"; } return typeof x === "string"; }
export function unreachableSecond(x: unknown) { return typeof x === "string"; return true; }
export function reassigned(x: unknown) { x = 1; return typeof x === "string"; }
export function closureWrite(x: unknown) { const g = () => { x = 1; }; return typeof x === "string"; }
export function thisRecv(this: unknown) { return typeof this === "string"; }
export function boolParam(x: boolean) { return x === true; }
export function bothParams(x: unknown, y: unknown) { return typeof x === "string" && typeof y === "string"; }
export function twoFirst(x: string | number, y: string | number) { return typeof x === "string" && typeof y === "string"; }
export function falseNotNarrow(x: string | number, n: number) { return typeof x === "string" && n > 0; }
export function strNum(x: string | number) { return typeof x === "string" && x.length > 0; }
export function litTrue(x: 'a' | 'b') { return x === 'a' || x === 'b'; }
export function neverFalse(x: string) { return typeof x === "string"; }
export function relational(x: number) { return x > 1; }
export function literalTrue(x: unknown) { return true; }
export function annotated(x: unknown): boolean { return typeof x === "string"; }
export function implicitEnd(x: unknown) { if (x) return typeof x === "string"; }
export function noParams() { return typeof globalThis === "object"; }
export function isArr(x: string | string[]) { return Array.isArray(x); }
export function looseNull(x: string | null | undefined) { return x == null; }
export function looseNotNull(x: string | null | undefined) { return x != null; }
export function looseLit(x: string | number) { return x == "a"; }
export function viaLocal(x: unknown) { const r = typeof x === "string"; return r; }
export function eqParams(x: string, y: string) { return x === y; }
export function nestedArrow() { return (x: string | number) => typeof x === "number"; }
export function nestedFn() { return function (x: Foo | Bar) { return x.kind === "bar"; }; }
export function nestedMulti() { return (x: unknown) => { if (x) return true; return typeof x === "string"; }; }
export const obj = { isS(x: unknown) { return typeof x === "string"; } };
export function sigObjMethod() { return obj.isS; }
"#;

/// The source with a `sig_NAME` wrapper appended for every function.
fn source_with_wrappers(names: &[&str]) -> String {
    let mut source = String::from(SOURCE);
    for name in names {
        source.push_str(&format!(
            "export function sig_{name}() {{ return {name}; }}\n"
        ));
    }
    source
}

/// `(function, strict print, strictNullChecks-off print)`, each measured
/// on 7.0.2: the strict print is the function's `.d.ts` line; the loose
/// print is the type a TS2322 message quotes for `typeof f` (the loose
/// `.d.ts` reprints the authored annotation, while the type itself has
/// `null` / `undefined` erased from its unions).
const INFERS: &[(&str, &str, &str)] = &[
    (
        "isStr",
        "(x: unknown) => x is string",
        "(x: unknown) => x is string",
    ),
    (
        "isStrArrow",
        "(x: unknown) => x is string",
        "(x: unknown) => x is string",
    ),
    (
        "isStrFnExpr",
        "(x: unknown) => x is string",
        "(x: unknown) => x is string",
    ),
    // `x !== null` over `string | null`: the loose `string | null` IS
    // `string`, so nothing narrows there.
    (
        "notNull",
        "(x: string | null) => x is string",
        "(x: string) => boolean",
    ),
    (
        "notUndef",
        "(x: string | undefined) => x is string",
        "(x: string) => boolean",
    ),
    // A loose nullish test selects both nullish members; a loose test
    // against another literal narrows as the strict one does.
    (
        "looseNull",
        "(x: string | null | undefined) => x is null | undefined",
        "(x: string) => boolean",
    ),
    (
        "looseNotNull",
        "(x: string | null | undefined) => x is string",
        "(x: string) => boolean",
    ),
    (
        "looseLit",
        "(x: string | number) => x is \"a\"",
        "(x: string | number) => x is \"a\"",
    ),
    (
        "isCls",
        "(x: Cls | string) => x is Cls",
        "(x: Cls | string) => x is Cls",
    ),
    (
        "inOp",
        "(x: Foo | Bar) => x is Foo",
        "(x: Foo | Bar) => x is Foo",
    ),
    (
        "isFooDisc",
        "(x: Foo | Bar) => x is Foo",
        "(x: Foo | Bar) => x is Foo",
    ),
    (
        "discNot",
        "(x: Foo | Bar) => x is Bar",
        "(x: Foo | Bar) => x is Bar",
    ),
    (
        "discAnd",
        "(x: Foo | Bar | null) => x is Bar",
        "(x: Foo | Bar) => boolean",
    ),
    // Loose, `x === null` keeps `string` on the false edge.
    (
        "andBoth",
        "(x: string | number | null) => x is string",
        "(x: string | number) => boolean",
    ),
    (
        "orBoth",
        "(x: string | number | boolean) => x is string | number",
        "(x: string | number | boolean) => x is string | number",
    ),
    (
        "isNum",
        "(x: string | number) => x is number",
        "(x: string | number) => x is number",
    ),
    (
        "notStr",
        "(x: string | number) => x is number",
        "(x: string | number) => x is number",
    ),
    // The FIRST parameter the test narrows names the predicate; a
    // `boolean` parameter never does.
    (
        "twoParams",
        "(x: unknown, y: unknown) => y is string",
        "(x: unknown, y: unknown) => y is string",
    ),
    (
        "secondOnly",
        "(x: boolean, y: string | number) => y is string",
        "(x: boolean, y: string | number) => y is string",
    ),
    (
        "boolFirst",
        "(x: boolean | string) => x is true",
        "(x: boolean | string) => x is true",
    ),
    (
        "eqLiteral",
        "(x: \"a\" | \"b\" | \"c\") => x is \"a\"",
        "(x: \"a\" | \"b\" | \"c\") => x is \"a\"",
    ),
    (
        "eqNumber",
        "(x: number | string) => x is 1",
        "(x: number | string) => x is 1",
    ),
    (
        "numLitEq",
        "(x: 1 | 2 | 3) => x is 2",
        "(x: 1 | 2 | 3) => x is 2",
    ),
    (
        "anyParam",
        "(x: any) => x is string",
        "(x: any) => x is string",
    ),
    (
        "parenthesized",
        "(x: unknown) => x is string",
        "(x: unknown) => x is string",
    ),
    (
        "notNot",
        "(x: unknown) => x is string",
        "(x: unknown) => x is string",
    ),
    // A same-file type guard called on the parameter.
    (
        "wrapPred",
        "(x: unknown) => x is Foo",
        "(x: unknown) => x is Foo",
    ),
    // A `this` receiver is not a parameter position.
    (
        "withThis",
        "(this: unknown, x: unknown) => x is string",
        "(this: unknown, x: unknown) => x is string",
    ),
    // Narrows the body established before the return count.
    (
        "narrowedBefore",
        "(x: string | number | null) => x is string",
        "(x: string | number) => x is string",
    ),
    // A member write does not assign the parameter.
    (
        "memberWrite",
        "(x: { a: number; } | null) => x is { a: number; }",
        "(x: { a: number; }) => boolean",
    ),
    // An unreachable `throw` is not a return; a nested function's return
    // is not this function's.
    (
        "throwAfter",
        "(x: unknown) => x is string",
        "(x: unknown) => x is string",
    ),
    (
        "nestedFnDecl",
        "(x: unknown) => x is string",
        "(x: unknown) => x is string",
    ),
    (
        "numKey",
        "(x: { a: string; } | { b: number; }) => x is { a: string; }",
        "(x: { a: string; } | { b: number; }) => x is { a: string; }",
    ),
];

/// Every measured inference, in both null policies.
#[test]
fn an_unannotated_boolean_return_infers_the_checker_type_predicate() {
    let names: Vec<&str> = INFERS.iter().map(|(name, _, _)| *name).collect();
    let host = host_with(&source_with_wrappers(&names));
    for (name, strict, loose) in INFERS {
        assert_complete(&host, STRICT_ROOT, name);
        assert_complete(&host, LOOSE_ROOT, name);
        assert_prints(&host, STRICT_ROOT, &format!("sig_{name}"), strict);
        assert_predicate(&host, LOOSE_ROOT, &format!("sig_{name}"), loose);
    }
}

/// `(function, print)` where the checker infers NO predicate, measured on
/// 7.0.2 under both null policies: a second `return` (reachable or not),
/// an assigned parameter (a closure's write included), no positional
/// parameter, a `boolean` parameter, a false edge that does not narrow the
/// true-edge type to `never` (both parameters, an opaque conjunct, a
/// member read), a true edge that changes nothing, a literal return, a return annotation, an implicit return.
const DECLINES: &[(&str, &str)] = &[
    ("multiReturn", "(x: unknown) => boolean"),
    ("unreachableSecond", "(x: unknown) => boolean"),
    ("reassigned", "(x: unknown) => boolean"),
    ("closureWrite", "(x: unknown) => boolean"),
    ("thisRecv", "(this: unknown) => boolean"),
    ("boolParam", "(x: boolean) => boolean"),
    ("bothParams", "(x: unknown, y: unknown) => boolean"),
    (
        "twoFirst",
        "(x: string | number, y: string | number) => boolean",
    ),
    (
        "falseNotNarrow",
        "(x: string | number, n: number) => boolean",
    ),
    ("strNum", "(x: string | number) => boolean"),
    ("litTrue", "(x: \"a\" | \"b\") => boolean"),
    ("neverFalse", "(x: string) => boolean"),
    ("relational", "(x: number) => boolean"),
    ("literalTrue", "(x: unknown) => boolean"),
    ("annotated", "(x: unknown) => boolean"),
    ("noParams", "() => boolean"),
];

#[test]
fn inference_declines_wherever_the_checker_declines() {
    let names: Vec<&str> = DECLINES.iter().map(|(name, _)| *name).collect();
    let host = host_with(&source_with_wrappers(&names));
    for (name, printed) in DECLINES {
        for root in [STRICT_ROOT, LOOSE_ROOT] {
            assert_complete(&host, root, name);
            assert_predicate(&host, root, &format!("sig_{name}"), printed);
        }
        assert_prints(&host, STRICT_ROOT, &format!("sig_{name}"), printed);
    }
    // An implicit return: `boolean | undefined` under strict null checks,
    // and still no predicate where the loose join erases to `boolean`.
    assert_prints(
        &host_with(&source_with_wrappers(&["implicitEnd"])),
        STRICT_ROOT,
        "sig_implicitEnd",
        "(x: unknown) => boolean | undefined",
    );
    assert_predicate(
        &host_with(&source_with_wrappers(&["implicitEnd"])),
        LOOSE_ROOT,
        "sig_implicitEnd",
        "(x: unknown) => boolean",
    );
}

/// A function VALUE returned from a body infers its own predicate. Measured
/// on 7.0.2 (`ReturnType<typeof f>`): `(x: string | number) => x is
/// number`, `(x: Foo | Bar) => x is Bar`, `(x: unknown) => boolean` (two
/// returns), and an object-literal method `(x: unknown) => x is string`.
#[test]
fn a_returned_function_value_infers_its_own_predicate() {
    let host = host_with(SOURCE);
    for root in [STRICT_ROOT, LOOSE_ROOT] {
        assert_prints(
            &host,
            root,
            "nestedArrow",
            "(x: string | number) => x is number",
        );
        assert_prints(&host, root, "nestedFn", "(x: Foo | Bar) => x is Bar");
        assert_prints(&host, root, "nestedMulti", "(x: unknown) => boolean");
        assert_prints(&host, root, "sigObjMethod", "(x: unknown) => x is string");
    }
}

/// A returned test the guard vocabulary cannot read DEGRADES a `boolean`
/// return rather than publishing it predicate-less. Measured on 7.0.2:
/// `Array.isArray(x)` over `string | string[]` is `x is string[]`, a call
/// handing the parameter to an EXPORTED same-file guard (whose signature
/// set this file cannot close) is `x is Foo`, `typeof x === "object" &&
/// "s" in x` over `Foo | Bar |
/// number` is `x is Bar` and `typeof x === "number" || "n" in x` over it
/// is `x is number | Foo` (a `typeof` test does not classify interface
/// arms), an aliased condition `const r = typeof x === "string"; return
/// r` is `x is string`, and `x === y` over two parameters is `boolean` — a
/// reference comparison this vocabulary does not carry either way.
#[test]
fn an_unreadable_returned_test_degrades_the_boolean_return() {
    let host = host_with(SOURCE);
    for name in [
        "isArr",
        "wrapExported",
        "hasKind",
        "orIn",
        "viaLocal",
        "eqParams",
    ] {
        assert_degrades(&host, STRICT_ROOT, name);
    }
}

/// The inferred predicate is a real part of the signature: the relation
/// reads it and conditional inference infers from it. Measured on 7.0.2
/// (each `A extends B ? 1 : 0`): `typeof isStr` against `(x: unknown) => x
/// is string` is `1`, `typeof multiReturn` against it is `0`, and
/// `typeof isStr extends (x: any) => x is infer U ? U : never` is `string`.
#[test]
fn the_relation_reads_an_inferred_predicate() {
    let source = format!(
        "{SOURCE}\
export function relIsStr() {{ const v: typeof isStr extends (x: unknown) => x is string ? 1 : 0 = null as any; return v; }}\n\
export function relMulti() {{ const v: typeof multiReturn extends (x: unknown) => x is string ? 1 : 0 = null as any; return v; }}\n\
export function inferIsStr() {{ const v: typeof isStr extends (x: any) => x is infer U ? U : never = null as any; return v; }}\n"
    );
    let host = host_with(&source);
    assert_prints_type(&host, "relIsStr", "1");
    assert_prints_type(&host, "relMulti", "0");
    assert_prints_type(&host, "inferIsStr", "string");
}

/// [`assert_prints`] for a non-function answer in the strict project.
fn assert_prints_type(host: &VerterHost, symbol: &str, printed: &str) {
    assert_prints(host, STRICT_ROOT, symbol, printed);
}

/// The predicate's subject is the parameter's POSITION, counted without a
/// `this` receiver, and its target is the true-edge type.
#[test]
fn an_inferred_predicate_names_the_positional_parameter() {
    let host = host_with(&source_with_wrappers(&["withThis", "twoParams"]));
    for (symbol, position) in [("sig_withThis", 0), ("sig_twoParams", 1)] {
        observe(&host, STRICT_ROOT, symbol, |dispatch, _, node| {
            let graph = dispatch.graph();
            let signature = match graph.node_data(node).as_deref() {
                Some(SemanticNodeData::Object(surface)) => surface.call_signatures[0],
                _ => node,
            };
            let Some(SemanticNodeData::Signature { predicate, .. }) =
                graph.node_data(signature).as_deref().cloned()
            else {
                panic!(
                    "`{symbol}` is a signature; measured `{}`",
                    render_node(dispatch, node, 0)
                );
            };
            let predicate = predicate.expect("an inferred predicate");
            assert_eq!(predicate.subject, PredicateSubject::Parameter(position));
            assert!(!predicate.asserts);
            assert_eq!(
                predicate
                    .ty
                    .and_then(|target| graph.node_data(target).as_deref().cloned()),
                Some(SemanticNodeData::Primitive(PrimitiveKind::String))
            );
        });
    }
}
