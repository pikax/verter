//! @ai-generated - Lexical-authority regression tests for the demand-sliced
//! `FlowReturn` evaluator.
//!
//! Every case here is oracle-anchored against `tsc 7.0.2 --strict
//! --declaration`. They characterise ONE invariant class: the content
//! lowering resolves every identifier through the SAME lexical authority
//! the demand plan uses (the `FunctionBodySkeleton`), so a
//! function-local binding can never silently fall through to a
//! file-scope (or cross-file imported) value of the same name; a
//! resolved local the content half cannot model fails CLOSED instead of
//! publishing a warm-admissible wrong answer.
//!
//! Plus the return-position literal rules (single fresh contributor
//! widens, a multi-contributor join does not), the declared-type
//! assignment rules (`getTypeAtFlowAssignment` /
//! `getAssignmentReducedType`), block-scoped `using`, and the labeled
//! statement's inner-rail propagation.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{PrimitiveName, TypeExpr};

const R5_OTHER: &str = "/ws/flow-r5-other.ts";
const R5_OTHER_SOURCE: &str = r#"
export declare const importedValue: "IMPORTED";
"#;

const R5_CANONICAL: &str = "/ws/flow-r5.ts";

/// Every file-scope name below is the LEAK BAIT: a function-local
/// binding of the same name must never resolve to it.
const R5_FIXTURE: &str = r#"
import { importedValue } from "/ws/flow-r5-other";

export declare const a: "hello";
export declare const b: "hello";
export declare const n: "hello";
export declare const C: "outer";
export declare const g: "outer";
export declare const E: "outer";
export declare const ns: "outer";
export declare const res: () => { close(): void };
export declare const obj: { wv: number };

// ── Lexical-authority leaks ───────────────────────────────────────────
export function r5DestructuredConst() {
  const { a } = { a: 1 };
  return a;
}

export function r5DestructuredParam({ b }: { b: number }) {
  return b;
}

export function r5CaptureParam(n: number) {
  return () => n;
}

export function r5CaptureLocal() {
  const a = 1;
  return () => a;
}

export function r5LocalClass() {
  class C {}
  return C;
}

export function r5NestedFnRead() {
  function g() {
    return 1;
  }
  return g;
}

export function r5LocalEnum() {
  enum E {
    A,
  }
  return E;
}

export function r5LocalNamespace() {
  namespace ns {
    export const inner = 1;
  }
  return ns;
}

export function r5CrossFileLeak() {
  class importedValue {}
  return importedValue;
}

export function r5FreeNameStillResolves() {
  {
    const importedValue = 1;
  }
  return importedValue;
}

export function r5BlockLetShadowsParam(p: string) {
  {
    let p = 1;
    return p;
  }
}

// ── Labeled-statement inner rails ─────────────────────────────────────
export function r5LabeledBlockVar() {
  outer: {
    var w = 1;
  }
  return w;
}

export function r5UnlabeledBlockVar() {
  {
    var w2 = 1;
  }
  return w2;
}

export function r5LabeledLoopVar(f: boolean) {
  outer: while (f) {
    var lv = 1;
  }
  return lv;
}

export function r5LabeledIfVar(f: boolean) {
  outer: if (f) {
    var iv = 1;
  }
  return iv;
}

export function r5LabeledSwitchVar(f: number) {
  outer: switch (f) {
    case 1:
      var sv = 1;
  }
  return sv;
}

export function r5LabeledTryVar() {
  outer: try {
    var tv = 1;
  } finally {
  }
  return tv;
}

// ── `using` is block-scoped ───────────────────────────────────────────
export function r5UsingInLoop(f: boolean) {
  while (f) {
    using u = res();
  }
  return 1;
}

// ── Flag folding on the call-on-binding read ──────────────────────────
export function r5CallOnConditionalVar(flag: boolean, cb: () => 1 | 2) {
  if (flag) var cb: () => 1 | 2 = () => 1;
  return cb();
}

export function r5SwitchHelper(value: number) {
  switch (value) {
    case 1:
      return "a";
    default:
      return "b";
  }
}

export function r5CallOnFailedInit(v: number) {
  const q = r5SwitchHelper(v);
  return q();
}

// ── Declared-type assignment rules ────────────────────────────────────
export function r5DeclaredUnknownLet() {
  let du: unknown = 1;
  return du;
}

export function r5DeclaredLiteralLet() {
  let dl: "s" = "s";
  return dl;
}

export function r5DeclaredNumberLet() {
  let dn: number = 1;
  return dn;
}

export function r5DeclaredUnionConst() {
  const cv: string | number = "s";
  return cv;
}

export function r5DeclaredUnionLet() {
  let un: string | number = "s";
  return un;
}

export function r5DeclaredNumericUnionLet() {
  let nv: 1 | 2 = 1;
  return nv;
}

export function r5DeclaredObjectUnion() {
  let ov: { a: number } | { b: string } = { a: 1 };
  return ov;
}

// ── Return-position literal rules ─────────────────────────────────────
export function r5ArrowBodyLiteral() {
  const cb = () => 1;
  return cb;
}

export function r5ArrowBodyConstAssert() {
  const cb = () => 1 as const;
  return cb;
}

export function r5ObjectMethodArrow() {
  return { m: () => 1 };
}

export function r5MultiReturnLiterals(c: boolean) {
  if (c) return 1;
  return 0;
}

export function r5MultiReturnSameLiteral(c: boolean) {
  if (c) return 1;
  return 1;
}

export function r5SingleReturnLiteral() {
  return 1;
}

export function r5ConstReadMulti(c: boolean) {
  if (c) {
    const bb = 1;
    return bb;
  }
  return 2;
}

export function r5ConstAssertReturn() {
  return 1 as const;
}

export function r5ObjectLiteralMember() {
  return { b: 1 };
}

export function r5ObjectConstAssertMember() {
  return { b: 1 as const };
}

// ── Structural widening at the PRODUCER, aggregate widening at the JOIN ─
export function r5ArrayLiteralJoin(c: boolean) {
  if (c) return [1];
  return [0];
}

export function r5ArrayLiteralSingle() {
  return [1];
}

export function r5ArrayConstElement() {
  return [1 as const];
}

export function r5ArrayAsConst() {
  return [1] as const;
}

export function r5ConditionalReturn(c: boolean) {
  return c ? 1 : 2;
}

export function r5ParenLiteralReturn() {
  return (1);
}

export function r5SatisfiesReturn() {
  return 1 satisfies number;
}

export function r5AsLiteralReturn() {
  return 1 as 1;
}

export function r5DedupFreshThenPinned(c: boolean) {
  if (c) return 1;
  return 1 as const;
}

export function r5DedupPinnedThenFresh(c: boolean) {
  if (c) return 1 as const;
  return 1;
}

export function r5CapturedWideningConst() {
  const x = 1;
  return { a: x, b: () => x };
}

// ── A mutual flow component with a DEGRADED member ────────────────────
export function r5MutualA(c: boolean) {
  if (c) return 1;
  return r5MutualB(c);
}

export function r5MutualB(c: boolean) {
  let z = 1;
  z = 2;
  return r5MutualA(!!z);
}

// ── Value space vs TYPE space ─────────────────────────────────────────
// The leaf answer's TYPE names resolve in TYPE space. A function-local
// binding shadows the module type alias only when its kind DECLARES a
// type: `class` / `enum` (and the two forms illegal in a function body,
// `namespace` / `import =`). `const` / `let` / `var` / a parameter / a
// nested function declaration declare a VALUE only, so the module type
// alias still governs — reading them off the value inventory fails
// closed on a name that never shadowed anything.
export type Info = { tag: "info" };
export type Res = { tag: "res" };

export function bCtrlNoLocal(x: unknown) {
  return x as Info;
}

export function bCtrlOtherLocal(x: unknown) {
  const other = 1;
  return x as Res;
}

export function bConst(x: unknown) {
  const Info = 1;
  return x as Info;
}

export function bLet(x: unknown) {
  let Info = 1;
  Info = 2;
  return x as Info;
}

export function bVar(x: unknown) {
  var Info = 1;
  return x as Info;
}

export function bParam(Info: number, x: unknown) {
  return x as Info;
}

export function bFn(x: unknown) {
  function Info() {}
  return x as Info;
}

export function bClass(x: unknown) {
  class Info {}
  return x as Info;
}

export function bEnum(x: unknown) {
  enum Info {
    A,
  }
  return x as Info;
}

// The type-space lookup must keep walking OUTWARD past a value-only
// region: the nearest region binding `Info` binds it in VALUE space only,
// but an OUTER region of the SAME frame declares the class. A lookup that
// stops at the nearest any-space region reads the module alias.
export function bClassOuterRegion(x: unknown) {
  class Info {}
  {
    const Info = 1;
    return x as Info;
  }
}

export function capConst() {
  const Info = 1;
  return (x: unknown) => x as Info;
}

export function capParam(Info: number) {
  return (x: unknown) => x as Info;
}

export function capClass() {
  class Info {}
  return (x: unknown) => x as Info;
}

export function capClassOuterRegion() {
  class Info {}
  {
    const Info = 1;
    return (x: unknown) => x as Info;
  }
}
"#;

fn make_r5_host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    for (canonical, source) in [(R5_OTHER, R5_OTHER_SOURCE), (R5_CANONICAL, R5_FIXTURE)] {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: crate::LanguageRegistry::global()
                .classify_static(canonical)
                .static_resolution(),
            aliases: Vec::new(),
        });
    }
    host
}

fn with_dispatch<R>(
    host: &Arc<VerterHost>,
    f: impl FnOnce(&ProjectSemanticDispatch<'_>) -> R,
) -> R {
    let store_view = host.resolver_store_view_read().into_owned_view();
    let overlay = Arc::new(crate::resolver_core::CanonicalCompletionOverlay::new());
    let host_ctx = crate::resolver_core::HostResolverContext::new(host, &store_view, overlay);
    let dispatch = ProjectSemanticDispatch::new(&host_ctx);
    f(&dispatch)
}

fn r5_key(dispatch: &ProjectSemanticDispatch<'_>, name: &str) -> FlowReturnKey {
    FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(R5_CANONICAL),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(R5_CANONICAL),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
    }
}

/// One evaluated function: its projected return type, its typed
/// degradation, and the family memo's candidate count (0 = ReturnOnly,
/// 1 = warm-admitted).
struct R5Outcome {
    ty: TypeExpr,
    degradation: Option<crate::semantic_query::FlowReturnDegradation>,
    candidates: usize,
}

fn r5_eval(host: &Arc<VerterHost>, name: &str) -> Option<R5Outcome> {
    with_dispatch(host, |dispatch| {
        let key = r5_key(dispatch, name);
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
        else {
            return None;
        };
        let ty = host
            .project_node_to_type_expr_for_test(result.return_type)
            .expect("a flow return value projects");
        let candidates = dispatch
            .graph()
            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
        Some(R5Outcome {
            ty,
            degradation: result.degradation,
            candidates,
        })
    })
}

/// Assert one function evaluates CLEAN (no degradation), warm-admissible
/// (one candidate), and to exactly `expected`.
#[track_caller]
fn assert_clean_warm(host: &Arc<VerterHost>, name: &str, expected: TypeExpr) {
    let outcome = r5_eval(host, name).unwrap_or_else(|| panic!("{name} must produce a value"));
    assert_eq!(outcome.degradation, None, "{name} must evaluate clean");
    assert_eq!(outcome.ty, expected, "{name} return type");
    assert_eq!(
        outcome.candidates, 1,
        "{name} must warm-admit exactly one candidate"
    );
}

/// Assert one function produces NO value (a typed `FlowReturnFailure`
/// through `Error(Miss)`) and admits nothing.
#[track_caller]
fn assert_fails_closed(host: &Arc<VerterHost>, name: &str) {
    with_dispatch(host, |dispatch| {
        let key = r5_key(dispatch, name);
        assert!(
            matches!(
                dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
                QueryResult::Error(QueryError::Miss)
            ),
            "{name} must fail closed with a typed no-value failure"
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            0,
            "{name} must admit nothing"
        );
    });
}

/// Assert one function produces a DEGRADED SUCCESS (a usable value with
/// a typed reason) that admits nothing.
#[track_caller]
fn assert_degraded(
    host: &Arc<VerterHost>,
    name: &str,
    expected: crate::semantic_query::FlowReturnDegradation,
) {
    let outcome = r5_eval(host, name).unwrap_or_else(|| panic!("{name} must produce a value"));
    assert_eq!(
        outcome.degradation,
        Some(expected),
        "{name} must carry its typed degradation"
    );
    assert_eq!(
        outcome.candidates, 0,
        "{name} degraded success is ReturnOnly"
    );
}

fn number() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::Number)
}

fn string_lit(value: &str) -> TypeExpr {
    TypeExpr::Literal(verter_type_expr::LiteralValue::String(value.to_string()))
}

fn number_lit(value: f64) -> TypeExpr {
    TypeExpr::Literal(verter_type_expr::LiteralValue::Number(value))
}

/// The return type of a projected zero-parameter function expression.
#[track_caller]
fn function_return(expr: &TypeExpr) -> &TypeExpr {
    let TypeExpr::Function(function) = expr else {
        panic!("expected a function type, got {expr:?}");
    };
    function
        .return_type
        .as_deref()
        .unwrap_or_else(|| panic!("expected an authored return type in {expr:?}"))
}

// ──────────────────────────────────────────────────────────────────────
// #1 — ONE lexical authority
// ──────────────────────────────────────────────────────────────────────

/// A function-local binding the CONTENT half cannot model must never
/// fall through to the file-scope `typeof` leaf: the name is resolved —
/// it is NOT free — so the leaf would bind an unrelated module-scope (or
/// cross-file imported) value of the same name, cleanly and warm.
///
/// Mutation recipe: resolving identifiers against a private per-frame
/// inventory instead of the skeleton republishes each of these as the
/// file-scope bait value (`"hello"` / `"outer"` / `"IMPORTED"`), clean and
/// warm-admitted.
#[test]
fn flow_return_unmodelable_local_binding_never_falls_through_to_file_scope() {
    let host = make_r5_host();
    for name in [
        // A destructuring declarator element (`const { a } = …`).
        "r5DestructuredConst",
        // A destructured formal parameter (`({ b }: …)`).
        "r5DestructuredParam",
        // A local `class` declaration's name.
        "r5LocalClass",
        // A hoisted nested function declaration's name read as a value.
        "r5NestedFnRead",
        // A local `enum` declaration's name.
        "r5LocalEnum",
        // A local `namespace` declaration's name.
        "r5LocalNamespace",
    ] {
        assert_fails_closed(&host, name);
    }
}

/// The cross-file proof: a local `class importedValue {}` shadows the
/// IMPORTED `importedValue`. A content half that cannot classify the
/// local name resolves the read in FILE OWNER SCOPE and publishes the
/// other file's value — clean, warm, and wrong.
#[test]
fn flow_return_local_binding_never_resolves_to_a_cross_file_import() {
    let host = make_r5_host();
    assert_fails_closed(&host, "r5CrossFileLeak");
}

/// The positive control: a name whose ONLY local binding is confined to
/// a sibling block is genuinely FREE at the return, so the file-scope
/// (imported) value is the correct answer. The fail-closed rail above
/// must not swallow it.
#[test]
fn flow_return_genuinely_free_name_still_resolves_through_the_file_scope() {
    let host = make_r5_host();
    assert_clean_warm(&host, "r5FreeNameStillResolves", string_lit("IMPORTED"));
}

/// Closure capture: a nested function value's read of an ENCLOSING
/// binding resolves through the enclosing frame's lexical authority,
/// never through a same-named file-scope declaration.
///
/// A captured PARAMETER is always available (the evaluator seeds the
/// nested frame with every enclosing parameter by name) — tsc 7.0.2:
/// `r5CaptureParam(n: number): () => number`. A captured LOCAL depends
/// on the demand plan having selected a definition for it, and the
/// planner does not walk nested function bodies, so it currently FAILS
/// CLOSED (tsc says `() => number`; the honest partial is no value at
/// all). Either way the file-scope `n` / `a` is never bound.
#[test]
fn flow_return_nested_function_captures_the_enclosing_binding_not_the_file_scope() {
    let host = make_r5_host();
    let outcome = r5_eval(&host, "r5CaptureParam").expect("r5CaptureParam evaluates");
    assert_eq!(outcome.degradation, None, "r5CaptureParam evaluates clean");
    assert_eq!(function_return(&outcome.ty), &number());
    assert_eq!(outcome.candidates, 1, "r5CaptureParam admits warm");
    assert_fails_closed(&host, "r5CaptureLocal");
}

/// A block-scoped `let` SHADOWS a same-named parameter: the local wins.
/// tsc 7.0.2: `r5BlockLetShadowsParam(p: string): number`.
///
/// Mutation recipe: testing the parameter list before the local scope
/// publishes the parameter's `string`.
#[test]
fn flow_return_block_local_shadows_a_same_named_parameter() {
    let host = make_r5_host();
    assert_clean_warm(&host, "r5BlockLetShadowsParam", number());
}

// ──────────────────────────────────────────────────────────────────────
// #2 — the labeled statement lowers its body
// ──────────────────────────────────────────────────────────────────────

/// A return-free LABELED statement is fall-through transparent but its
/// body still lowers: every inner rail (hoisted `var` scoping, the
/// return-free-loop `var` fail-close, the conditional-`var` degradation,
/// `switch` / `try` / `with`) applies exactly as it does for the
/// unlabeled twin. Before the fix the labeled arm emitted a bare
/// `TransparentLoop` and NEVER lowered its body, so every construct
/// nested under a label bypassed all of them.
///
/// tsc 7.0.2 (`--strict`): each of these is `number` (the loop / if /
/// switch shapes additionally report "used before being assigned"),
/// which is exactly what each unlabeled twin already fails closed on.
#[test]
fn flow_return_labeled_statement_body_reaches_every_inner_rail() {
    let host = make_r5_host();
    // Unconditional block: the hoisted `var` reaches the function scope
    // — clean `number`, exactly like the unlabeled twin.
    assert_clean_warm(&host, "r5UnlabeledBlockVar", number());
    assert_clean_warm(&host, "r5LabeledBlockVar", number());
    // A return-free loop declaring a `var` escapes the loop: the typed
    // loop rail fails closed.
    assert_fails_closed(&host, "r5LabeledLoopVar");
    // A conditional `var` has no single reaching definition: degraded.
    assert_degraded(
        &host,
        "r5LabeledIfVar",
        crate::semantic_query::FlowReturnDegradation::ConditionalVarDefinition,
    );
    // `switch` / `try` stay unsupported under a label.
    assert_fails_closed(&host, "r5LabeledSwitchVar");
    assert_fails_closed(&host, "r5LabeledTryVar");
}

// ──────────────────────────────────────────────────────────────────────
// #8 — `using` / `await using` are BLOCK-scoped
// ──────────────────────────────────────────────────────────────────────

/// `using` / `await using` declare BLOCK-scoped bindings (like `const`),
/// not function-scoped `var`s. Classifying them as `var` makes a
/// return-free loop containing one trip the "a `var` escapes the loop"
/// fail-close. tsc 7.0.2: `r5UsingInLoop(f: boolean): number`.
#[test]
fn flow_return_using_declaration_is_block_scoped_not_a_hoisted_var() {
    let host = make_r5_host();
    assert_clean_warm(&host, "r5UsingInLoop", number());
}

// ──────────────────────────────────────────────────────────────────────
// #4 — a local READ always folds its membership flags
// ──────────────────────────────────────────────────────────────────────

/// Reading a local for a CALL folds the same membership flags a value
/// read does: a conditionally-defined `var` degrades, and a binding
/// whose initializer failed degrades. Before the fix the call site took
/// the bound node WITHOUT the flags, so
/// `r5CallOnConditionalVar` published the literal `1` clean and warm
/// where tsc 7.0.2 says `1 | 2`.
#[test]
fn flow_return_call_on_binding_folds_the_read_membership_flags() {
    let host = make_r5_host();
    assert_degraded(
        &host,
        "r5CallOnConditionalVar",
        crate::semantic_query::FlowReturnDegradation::ConditionalVarDefinition,
    );
    assert_degraded(
        &host,
        "r5CallOnFailedInit",
        crate::semantic_query::FlowReturnDegradation::FailedBindingInitializer,
    );
}

// ──────────────────────────────────────────────────────────────────────
// #5 / #6 — the declared type governs an annotated declarator
// ──────────────────────────────────────────────────────────────────────

/// `getTypeAtFlowAssignment`: an annotated declarator whose declared
/// type is NOT a union takes the DECLARED type verbatim — never the
/// initializer's literal, never the widened initializer. tsc 7.0.2:
/// `unknown`, `"s"`, `number`.
#[test]
fn flow_return_non_union_declared_type_supplies_the_binding_verbatim() {
    let host = make_r5_host();
    assert_clean_warm(
        &host,
        "r5DeclaredUnknownLet",
        TypeExpr::Primitive(PrimitiveName::Unknown),
    );
    assert_clean_warm(&host, "r5DeclaredLiteralLet", string_lit("s"));
    assert_clean_warm(&host, "r5DeclaredNumberLet", number());
}

/// `getAssignmentReducedType`: an annotated declarator whose declared
/// type IS a union takes the union of the DECLARED constituents the
/// initializer is comparable to — made of declared constituents, never
/// the initializer's own (fresh or widened) type. tsc 7.0.2: `string`,
/// `string`, `1`, `{ a: number }`.
#[test]
fn flow_return_union_declared_type_reduces_to_the_comparable_constituents() {
    let host = make_r5_host();
    assert_clean_warm(
        &host,
        "r5DeclaredUnionConst",
        TypeExpr::Primitive(PrimitiveName::String),
    );
    assert_clean_warm(
        &host,
        "r5DeclaredUnionLet",
        TypeExpr::Primitive(PrimitiveName::String),
    );
    assert_clean_warm(&host, "r5DeclaredNumericUnionLet", number_lit(1.0));
    let outcome = r5_eval(&host, "r5DeclaredObjectUnion").expect("evaluates");
    assert_eq!(outcome.degradation, None);
    let TypeExpr::Object(object) = &outcome.ty else {
        panic!("expected an object type, got {:?}", outcome.ty);
    };
    assert_eq!(
        object.properties.len(),
        1,
        "the reduced arm is the single comparable declared constituent: {:?}",
        outcome.ty
    );
}

// ──────────────────────────────────────────────────────────────────────
// #9 / #10 — the return-position literal rules
// ──────────────────────────────────────────────────────────────────────

/// An expression-bodied arrow's synthesized return is a RETURN position
/// like any other: a single fresh literal widens. tsc 7.0.2:
/// `r5ArrowBodyLiteral(): () => number`,
/// `r5ArrowBodyConstAssert(): () => 1`,
/// `r5ObjectMethodArrow(): { m: () => number }`.
#[test]
fn flow_return_expression_bodied_arrow_widens_a_fresh_literal() {
    let host = make_r5_host();
    let outcome = r5_eval(&host, "r5ArrowBodyLiteral").expect("evaluates");
    assert_eq!(outcome.degradation, None);
    assert_eq!(function_return(&outcome.ty), &number());
    let outcome = r5_eval(&host, "r5ArrowBodyConstAssert").expect("evaluates");
    assert_eq!(outcome.degradation, None);
    assert_eq!(
        function_return(&outcome.ty),
        &number_lit(1.0),
        "a const assertion is not a fresh literal and never widens"
    );
    let outcome = r5_eval(&host, "r5ObjectMethodArrow").expect("evaluates");
    assert_eq!(outcome.degradation, None);
    let member = super::flow_return_tests::object_prop(&outcome.ty, "m");
    assert_eq!(function_return(member), &number());
}

/// Literal widening at the return join is a SINGLE-contributor rule:
/// tsc aggregates the return-expression types (deduplicated, plus the
/// `undefined` arm), and only a lone contributor widens. tsc 7.0.2:
/// `r5MultiReturnLiterals(c): 0 | 1`,
/// `r5MultiReturnSameLiteral(c): number` (deduplicated to one),
/// `r5SingleReturnLiteral(): number`,
/// `r5ConstReadMulti(c): 1 | 2`,
/// `r5ConstAssertReturn(): 1`.
#[test]
fn flow_return_multi_contributor_literal_join_does_not_widen() {
    let host = make_r5_host();
    let outcome = r5_eval(&host, "r5MultiReturnLiterals").expect("evaluates");
    assert_eq!(outcome.degradation, None);
    let TypeExpr::Union(members) = &outcome.ty else {
        panic!("expected a union, got {:?}", outcome.ty);
    };
    assert_eq!(members.len(), 2, "{:?}", outcome.ty);
    assert!(members.contains(&number_lit(0.0)) && members.contains(&number_lit(1.0)));
    // Deduplication collapses two identical literal contributors to one,
    // which then widens.
    assert_clean_warm(&host, "r5MultiReturnSameLiteral", number());
    assert_clean_warm(&host, "r5SingleReturnLiteral", number());
    assert_clean_warm(&host, "r5ConstAssertReturn", number_lit(1.0));
    let outcome = r5_eval(&host, "r5ConstReadMulti").expect("evaluates");
    assert_eq!(outcome.degradation, None);
    let TypeExpr::Union(members) = &outcome.ty else {
        panic!("expected a union, got {:?}", outcome.ty);
    };
    assert!(
        members.contains(&number_lit(1.0)) && members.contains(&number_lit(2.0)),
        "a widening-literal `const` read stays pinned in a multi-contributor join: {:?}",
        outcome.ty
    );
}

/// Object-literal MEMBER widening is independent of the return join:
/// a fresh member literal always widens, a const-asserted member never
/// does. tsc 7.0.2: `{ b: number }` and `{ b: 1 }`.
#[test]
fn flow_return_object_member_literals_widen_independently_of_the_join() {
    let host = make_r5_host();
    let outcome = r5_eval(&host, "r5ObjectLiteralMember").expect("evaluates");
    assert_eq!(
        super::flow_return_tests::object_prop(&outcome.ty, "b"),
        &number()
    );
    let outcome = r5_eval(&host, "r5ObjectConstAssertMember").expect("evaluates");
    assert_eq!(
        super::flow_return_tests::object_prop(&outcome.ty, "b"),
        &number_lit(1.0)
    );
}

// ──────────────────────────────────────────────────────────────────────
// #11 — the two widening axes: STRUCTURAL at the producer, AGGREGATE at
//       the join
// ──────────────────────────────────────────────────────────────────────

/// STRUCTURAL widening belongs to the PRODUCER, not the join. An array
/// literal's element type widens at lowering time, unconditionally —
/// the decision is not aggregate-dependent and the interned node carries
/// no freshness bit, so a join-side recursive widener could not tell
/// `[1]` from `[1 as const]`. tsc 7.0.2:
/// `r5ArrayLiteralJoin(c): number[]` (TWO arms, still widened),
/// `r5ArrayLiteralSingle(): number[]`,
/// `r5ArrayConstElement(): 1[]`,
/// `r5ArrayAsConst(): readonly [1]`.
///
/// Mutation recipe: routing the return position through the
/// literal-preserving short-circuit (a single `preserve_literal` axis)
/// disables the element widen and publishes `1[]` / `0 | 1[]`.
#[test]
fn flow_return_array_element_widening_is_a_producer_rule_not_a_join_rule() {
    let host = make_r5_host();
    let number_array = TypeExpr::Array {
        element: Arc::new(number()),
        readonly: false,
    };
    assert_clean_warm(&host, "r5ArrayLiteralJoin", number_array.clone());
    assert_clean_warm(&host, "r5ArrayLiteralSingle", number_array);
    assert_clean_warm(
        &host,
        "r5ArrayConstElement",
        TypeExpr::Array {
            element: Arc::new(number_lit(1.0)),
            readonly: false,
        },
    );
    // `[1] as const` is a const assertion: a READONLY TUPLE of the pinned
    // literal, never an array and never widened.
    let outcome = r5_eval(&host, "r5ArrayAsConst").expect("evaluates");
    assert_eq!(outcome.degradation, None);
    let TypeExpr::Tuple { elements, readonly } = &outcome.ty else {
        panic!("expected a readonly tuple, got {:?}", outcome.ty);
    };
    assert!(readonly, "`as const` produces a READONLY tuple");
    assert_eq!(elements.len(), 1, "{:?}", outcome.ty);
    assert_eq!(elements[0].ty, number_lit(1.0), "{:?}", outcome.ty);
}

/// The transparent producer arms propagate the caller's top-level policy
/// instead of hardcoding a widen. A return-position conditional is a
/// union of TWO fresh literals — an aggregate of two, which tsc never
/// widens. tsc 7.0.2: `r5ConditionalReturn(c): 1 | 2`,
/// `r5ParenLiteralReturn(): number` (one contributor, widened at the
/// join).
///
/// Mutation recipe: hardcoding `Widen` in the conditional arm collapses
/// this to `number`.
#[test]
fn flow_return_conditional_arms_propagate_the_top_level_literal_policy() {
    let host = make_r5_host();
    let outcome = r5_eval(&host, "r5ConditionalReturn").expect("evaluates");
    assert_eq!(outcome.degradation, None);
    let TypeExpr::Union(members) = &outcome.ty else {
        panic!("expected `1 | 2`, got {:?}", outcome.ty);
    };
    assert_eq!(members.len(), 2, "{:?}", outcome.ty);
    assert!(
        members.contains(&number_lit(1.0)) && members.contains(&number_lit(2.0)),
        "a union of two fresh literals is an aggregate of TWO: {:?}",
        outcome.ty
    );
    assert_eq!(outcome.candidates, 1);
    assert_clean_warm(&host, "r5ParenLiteralReturn", number());
}

/// FRESHNESS is a syntactic classification of the return ARGUMENT, and
/// `satisfies` is transparent to it: `1 satisfies number` is still the
/// bare literal `1`, so the lone-contributor join widens it. An `as`
/// assertion — even to the literal type itself — PINS. tsc 7.0.2:
/// `r5SatisfiesReturn(): number`, `r5AsLiteralReturn(): 1`.
///
/// Mutation recipe: unwrapping `TSAsExpression` alongside
/// `TSSatisfiesExpression` republishes `r5AsLiteralReturn` as `number`.
#[test]
fn flow_return_satisfies_is_freshness_transparent_and_as_is_not() {
    let host = make_r5_host();
    assert_clean_warm(&host, "r5SatisfiesReturn", number());
    assert_clean_warm(&host, "r5AsLiteralReturn", number_lit(1.0));
}

/// The aggregate freshness fold runs over EVERY contributor, including
/// the ones deduplication drops. `1` and `1 as const` intern to the SAME
/// node — that is why the second dedupes — but only the first is FRESH,
/// so the aggregate is not all-fresh and must not widen. Folding after
/// the dedup `continue` makes the answer depend on which contributor came
/// first. tsc 7.0.2: `1` for BOTH orders.
///
/// Mutation recipe: folding `all_fresh` after the `continue` publishes
/// `number` for `r5DedupFreshThenPinned` and `1` for its reverse — the
/// same aggregate, two answers.
#[test]
fn flow_return_freshness_folds_over_deduplicated_contributors_in_both_orders() {
    let host = make_r5_host();
    assert_clean_warm(&host, "r5DedupFreshThenPinned", number_lit(1.0));
    assert_clean_warm(&host, "r5DedupPinnedThenFresh", number_lit(1.0));
}

/// A widening-literal `const` read is widened at EVERY read site the
/// frame models — the direct read and the CAPTURED read inside a nested
/// function value alike. The nested frame is seeded with the enclosing
/// frame's widening-local set, so a capture that skipped the widen would
/// publish a pinned literal from a set that has no other consumer.
/// tsc 7.0.2: `r5CapturedWideningConst(): { a: number; b: () => number }`.
///
/// Mutation recipe: matching only the direct-read carrier republishes
/// `b` as `() => 1`.
#[test]
fn flow_return_captured_widening_literal_const_read_widens_like_a_direct_read() {
    let host = make_r5_host();
    let outcome = r5_eval(&host, "r5CapturedWideningConst").expect("evaluates");
    assert_eq!(outcome.degradation, None);
    assert_eq!(
        super::flow_return_tests::object_prop(&outcome.ty, "a"),
        &number(),
        "the DIRECT read widens"
    );
    assert_eq!(
        function_return(super::flow_return_tests::object_prop(&outcome.ty, "b")),
        &number(),
        "the CAPTURED read widens identically"
    );
    assert_eq!(outcome.candidates, 1);
}

// ──────────────────────────────────────────────────────────────────────
// #12 — the degradation rail survives the component discharge
// ──────────────────────────────────────────────────────────────────────

/// The FIRST-demanded member of a mutual flow component whose other
/// member is DEGRADED must carry that degradation and admit NOTHING —
/// whichever member is demanded first.
///
/// `r5MutualB` observes an `UnappliedWriteEffect`; `r5MutualA` joins
/// `r5MutualB`'s discharged return, so A's result is built from a
/// degraded contributor and is itself degraded. Before the fix, A's
/// evaluation reached the discharge as a hold-only `EmptyCycle`, whose
/// construction DROPPED the observed degradation; the discharge then
/// resurrected it from its hold targets and stamped it `Complete` with
/// `degradation: None`, so the publish gate — which refuses only a
/// degradation it can SEE — admitted it WARM. Demanding B first took the
/// other path and reported the degradation honestly: the same key, two
/// values and two warmths, chosen by demand order.
///
/// Each order needs its OWN host: `SemanticGraphStore` is host-owned and
/// outlives any one `ProjectSemanticDispatch`, so reversing the demands
/// on a single host re-reads the first order's state instead of
/// executing the second.
///
/// Mutation recipe: seeding the discharge's degradation from `current[i]`
/// (which is `None` for a failed member) instead of from the entry's own
/// outcome restores the warm publication in the A-first order only.
#[test]
fn flow_return_degraded_component_member_is_order_independent_and_never_warms() {
    for first in ["r5MutualA", "r5MutualB"] {
        let host = make_r5_host();
        let outcome = r5_eval(&host, first)
            .unwrap_or_else(|| panic!("{first} must produce a value when demanded first"));
        assert_eq!(
            outcome.degradation,
            Some(crate::semantic_query::FlowReturnDegradation::UnappliedWriteEffect),
            "{first}: the component's observed degradation must survive the discharge"
        );
        assert_eq!(outcome.ty, number(), "{first} return type");
        assert_eq!(
            outcome.candidates, 0,
            "{first}: a degraded component is ReturnOnly — it must never warm"
        );
    }
}

/// The frame's TYPE-space names are classified against TYPE-declaring
/// bindings only — never against the value inventory.
///
/// `answer_names_frame_bound` walks the leaf answer's referenced names in
/// two spaces. The value roots (`typeof x…`) are a value question and
/// resolve through the `FunctionBodySkeleton`, which is a VALUE
/// inventory. Routing the TYPE names through the same authority conflates
/// the two namespaces: a local `const Info = 1` makes `x as Info` fail
/// closed even though `Info` in type position still names the module
/// alias, while the genuinely type-declaring `class Info {}` /
/// `enum Info {}` must keep failing closed.
///
/// Oracle (`tsgo --strict --declaration --emitDeclarationOnly`):
///
/// ```text
/// bCtrlNoLocal(x: unknown): Info      bClass(x: unknown): {}
/// bCtrlOtherLocal(x: unknown): Res    bEnum(x: unknown): Info   // TS4060: private name
/// bConst / bLet / bVar: Info          bClassOuterRegion(x: unknown): {}
/// bParam(Info: number, …): Info       capConst(): (x: unknown) => Info
/// bFn(x: unknown): Info               capParam(Info: number): (x: unknown) => Info
///                                     capClass(): (x: unknown) => {}
///                                     capClassOuterRegion(): (x: unknown) => {}
/// ```
///
/// `bClass` / `bEnum` / `capClass` print the LOCAL declaration's type
/// (`{}` structurally; `Info` under TS4060 "private name" for the enum),
/// which no owner-scope resolution can supply — those fail closed here.
/// Every other row is the module alias, which the owner-scope leaf
/// lowering resolves correctly and must be allowed to publish.
///
/// The `…OuterRegion` rows pin the SECOND half of the rule: the type-space
/// lookup must keep walking OUTWARD past a region that binds the name in
/// VALUE space only. `class Info {}` at the frame root with `const Info =
/// 1` in an inner block still owns `Info` in type space at that inner
/// block — `resolveName` with a Type meaning skips a scope whose symbol
/// carries no type meaning and continues outward. A lookup that stops at
/// the nearest ANY-space region sees only the `const`, reports "not
/// frame-bound", and publishes the module alias clean and warm.
///
/// Mutation recipe: routing `names.type_names` back through
/// `resolve_name` (the value inventory) flips every value-space row to
/// `Error(Miss)`; dropping the type-space filter entirely (treating no
/// local as type-declaring) flips `bClass` / `bEnum` / `capClass` to a
/// warm wrong answer; filtering the nearest ANY-space region's binding set
/// by kind instead of walking the region chain in type space flips
/// `bClassOuterRegion` / `capClassOuterRegion` to a warm wrong answer.
#[test]
fn flow_return_type_space_names_are_not_classified_against_the_value_inventory() {
    let host = make_r5_host();
    let info = || TypeExpr::Ref {
        name: Arc::from("Info"),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    };

    // Controls: no local at all, and a local under a DIFFERENT name.
    assert_clean_warm(&host, "bCtrlNoLocal", info());
    assert_clean_warm(
        &host,
        "bCtrlOtherLocal",
        TypeExpr::Ref {
            name: Arc::from("Res"),
            type_arguments: Arc::from(Vec::new().into_boxed_slice()),
        },
    );

    // VALUE-space local kinds: no type-space shadow, so the owner-scope
    // answer stands.
    for name in ["bConst", "bLet", "bVar", "bParam", "bFn"] {
        assert_clean_warm(&host, name, info());
    }

    // TYPE-declaring local kinds: the frame owns the name in type space,
    // so the owner-scope answer is the wrong one and must fail closed.
    // `bClassOuterRegion` declares the class in an OUTER region of the
    // same frame, behind a value-only `const` in the reading region.
    for name in ["bClass", "bEnum", "bClassOuterRegion"] {
        assert_fails_closed(&host, name);
    }

    // The CAPTURE half: a nested function value resolves the same two
    // spaces through the enclosing frames' capture scope, which collapses
    // every captured kind into one bit. A captured `const` / parameter is
    // value-only; a captured `class` declares a type.
    for name in ["capConst", "capParam"] {
        let outcome = r5_eval(&host, name).unwrap_or_else(|| panic!("{name} must produce a value"));
        assert_eq!(outcome.degradation, None, "{name} must evaluate clean");
        let TypeExpr::Function(function) = &outcome.ty else {
            panic!("{name} returns a function type, got {:?}", outcome.ty);
        };
        let return_type = function
            .return_type
            .as_ref()
            .unwrap_or_else(|| panic!("{name}'s function type carries a return"));
        assert_eq!(
            **return_type,
            info(),
            "{name}: the captured value-space binding must not shadow the module type alias"
        );
    }
    assert_fails_closed(&host, "capClass");
    assert_fails_closed(&host, "capClassOuterRegion");
}
