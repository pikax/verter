//! @ai-generated - The ROOT-IDENTIFIER GATE and its two lexical siblings.
//!
//! Every case is oracle-anchored against tsgo 7.0.0-dev.20260526.1 /
//! `tsgo 7.0.0-dev.20260526.1` (`--strict --declaration`). Three invariant classes:
//!
//! 1. **The root-identifier gate.** The shared shallow-pass leaf
//!    lowering has no frame — it resolves every name in FILE OWNER
//!    SCOPE. So any leaf whose answer names a binding THIS FRAME owns
//!    would publish an unrelated module-scope symbol's type, cleanly and
//!    warm. Every leaf answer routes through one gate; a frame-owned
//!    name the owner scope ANSWERS fails closed, and one the owner scope
//!    answers nothing for evaluates unchanged (its own typed miss is the
//!    honest result).
//! 2. **A named function expression's own name** binds inside its own
//!    body, so it is part of the NESTED frame's lexical inventory. It
//!    never looks free, and the outer frame's self name can no longer
//!    mint a recursion hold from inside a nested value.
//! 3. **A block-level function declaration is BLOCK-scoped** in
//!    strict-mode ESM: only `var` reaches function scope unconditionally.
//!
//! Plus the interim direct-call rail's ANNOTATION honesty: a value
//! declaration carrying both an authored annotation and an initializer is
//! typed by the annotation.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{PrimitiveName, TypeExpr};

const R6_CANONICAL: &str = "/ws/flow-r6.ts";

/// Every file-scope `*Bait` declaration is the LEAK BAIT: a frame
/// binding of the same name must never resolve to it.
const R6_FIXTURE: &str = r#"
// ── The root-identifier gate: OWNER-SCOPE LEAKS ──────────────────────
declare const CBait: { s: "OUTERSTATIC" };
export function gateStaticMemberOnLocalClass() {
  class CBait {
    static s = 1;
  }
  return CBait.s;
}

declare const objBait: { m(): "OUTERM" };
export function gateMethodCallOnLocal() {
  const objBait = {
    m() {
      return 1;
    },
  };
  return objBait.m();
}

declare const paramBait: { s: "OUTERPARAM" };
export function gateStaticMemberOnParam(paramBait: { s: number }) {
  return paramBait.s;
}

declare const condBait: "OUTERCOND";
export function gateConditionalArm(c: boolean) {
  const condBait = 1;
  return c ? condBait : 2;
}

declare const arrBait: "OUTERARR";
export function gateArrayElement() {
  const arrBait = 1;
  return [arrBait];
}

declare const nestBait: "OUTERNEST";
export function gateNestedArrowInArray() {
  const nestBait = 1;
  return [() => nestBait];
}

declare const spreadBait: { a: "OUTERSPREAD" };
export function gateObjectSpread(spreadBait: { a: number }) {
  return { ...spreadBait, x: 1 };
}

type LocalCBait = { q: string };
export function gateTypeSpaceLocalClass() {
  class LocalCBait {
    q = 1;
  }
  return null as unknown as LocalCBait;
}

// ── The root-identifier gate: UNMODELLED FORMS read THROUGH a frame
//    binding (the leaf answers a bare `any`) ───────────────────────────
declare const compBait: { x: "OUTERCOMP" };
export function gateComputedMember() {
  const compBait = { x: 1 };
  return compBait["x"];
}

declare const optBait: { y: "OUTEROPT" };
export function gateOptionalChain() {
  const optBait = { y: 1 };
  return optBait?.y;
}

declare const NewBait: { new (): { z: "OUTERNEW" } };
export function gateNewExpression() {
  class NewBait {
    z = 1;
  }
  return new NewBait();
}

declare const tagBait: (s: TemplateStringsArray) => "OUTERTAG";
export function gateTaggedTemplate() {
  const tagBait = (s: TemplateStringsArray) => 1;
  return tagBait`q`;
}

declare const privBait: { p: "OUTERPRIV" };
export function gatePrivateField() {
  class K {
    #p = 1;
    r() {
      return this.#p;
    }
  }
  return new K().r();
}

// ── Positive controls: a genuinely FREE root still resolves ──────────
declare const freeOk: { s: "FREEOK" };
export function gateFreeRootResolves() {
  return freeOk.s;
}

declare const freeSpread: { a: number };
export function gateFreeSpreadResolves() {
  return { ...freeSpread, x: 1 };
}

export function gateConditionalTestOnParam(c: boolean) {
  return c ? 1 : 2;
}

// ── A frame-owned name the OWNER SCOPE answers nothing for ───────────
export function gateUnansweredFrameNameStaysOpen(uniqueLocalName: { a: number }) {
  return { ...uniqueLocalName, x: 1 };
}

// ── Named function expression self-reference ─────────────────────────
declare const dself: "bait";
export function selfNameRead() {
  return function dself() {
    return dself;
  };
}

export function selfNameCallHoldsNothingOuter() {
  const g = function h() {
    return h();
  };
  return g;
}

// ── Block-scoped function declaration ────────────────────────────────
declare const fbait: () => "OUTERFN";
export function blockFunctionIsBlockScoped(c: boolean) {
  if (c) {
    function fbait() {
      return 1;
    }
  }
  return fbait();
}

declare const hoistBait: () => "OUTERHOISTED";
export function rootFunctionStillHoists() {
  return hoistBait();
  function hoistBait() {
    return 1;
  }
}

export function blockVarStillHoists() {
  {
    var bv = 1;
  }
  return bv;
}

// ── The interim direct-call rail honours the declarator annotation ───
export const annotatedFn: () => 42 = () => 42;
export function callAnnotatedFn() {
  return annotatedFn();
}

export const annotatedParenFn: () => 42 = (() => 42);
export function callAnnotatedParenFn() {
  return annotatedParenFn();
}

export declare const ambientFn: () => 42;
export function callAmbientFn() {
  return ambientFn();
}

export const arrowAnnotatedReturn = (): 42 => 42;
export function callArrowAnnotatedReturn() {
  return arrowAnnotatedReturn();
}

export const unannotatedFn = () => 42;
export function callUnannotatedFn() {
  return unannotatedFn();
}

export function callAnnotatedLocal() {
  const local: () => 42 = () => 42;
  return local();
}
"#;

fn make_host() -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(R6_CANONICAL.to_string()),
        input_id: R6_CANONICAL.to_string(),
        source: Arc::from(R6_FIXTURE),
        file_language: crate::LanguageRegistry::global()
            .classify_static(R6_CANONICAL)
            .static_resolution(),
        aliases: Vec::new(),
    });
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

fn r6_key(dispatch: &ProjectSemanticDispatch<'_>, name: &str) -> FlowReturnKey {
    FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(R6_CANONICAL),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            FunctionPartIdentity::DeclarationBody,
            0,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(R6_CANONICAL),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
    }
}

/// Assert one function evaluates CLEAN (no degradation), warm-admissible
/// (one candidate), and to exactly `expected`.
#[track_caller]
fn assert_clean_warm(host: &Arc<VerterHost>, name: &str, expected: TypeExpr) {
    with_dispatch(host, |dispatch| {
        let key = r6_key(dispatch, name);
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
        else {
            panic!("{name} must produce a value");
        };
        assert_eq!(result.degradation(), None, "{name} must evaluate clean");
        let ty = host
            .project_node_to_type_expr_for_test(result.return_type())
            .expect("a flow return value projects");
        assert_eq!(ty, expected, "{name} return type");
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            1,
            "{name} must warm-admit exactly one candidate"
        );
    });
}

/// Assert one function produces NO value (a typed `FlowReturnFailure`
/// through `Error(Miss)`) and admits nothing.
#[track_caller]
fn assert_fails_closed(host: &Arc<VerterHost>, name: &str) {
    with_dispatch(host, |dispatch| {
        let key = r6_key(dispatch, name);
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

/// The projected return type of one function, whatever its degradation.
#[track_caller]
fn projected(host: &Arc<VerterHost>, name: &str) -> TypeExpr {
    with_dispatch(host, |dispatch| {
        let key = r6_key(dispatch, name);
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
        else {
            panic!("{name} must produce a value");
        };
        host.project_node_to_type_expr_for_test(result.return_type())
            .expect("a flow return value projects")
    })
}

fn number() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::Number)
}

fn number_lit(value: f64) -> TypeExpr {
    TypeExpr::number_literal(value)
}

fn string_lit(value: &str) -> TypeExpr {
    TypeExpr::Literal(verter_type_expr::LiteralValue::String(value.to_string()))
}

// ──────────────────────────────────────────────────────────────────────
// #8 — ONE root-identifier gate
// ──────────────────────────────────────────────────────────────────────

/// A leaf whose ANSWER names a frame-owned binding must never bind the
/// owner-scope declaration of the same name.
///
/// Each fixture below pairs a frame binding with a file-scope `*Bait`
/// declaration. Before the gate the leaf published the BAIT's type,
/// clean and warm-admitted: `CBait.s` was `"OUTERSTATIC"` (tsgo:
/// `number`), `objBait.m()` was `"OUTERM"` (tsgo: `number`),
/// `paramBait.s` was `"OUTERPARAM"`, `c ? condBait : 2` was
/// `"OUTERCOND" | 2`, `[arrBait]` was `"OUTERARR"[]`, `[() => nestBait]`
/// was `(() => "OUTERNEST")[]`, and `{ ...spreadBait, x: 1 }` was
/// `{ a: "OUTERSPREAD"; x: number }` (tsgo: `{ a: number; x: number }`).
///
/// A CONDITIONAL expression is no longer one of the fail-closed rows: it
/// has a structural arm now, so each branch resolves through the frame's
/// own lexical authority and `c ? condBait : 2` is the checker's `1 | 2`
/// — the local, exactly. It stays in this suite as the row that proves
/// the frame binding WINS rather than merely blocking an answer: an
/// owner-scope resolution of the same name reads `"OUTERCOND" | 2`.
///
/// Mutation recipe: dropping the gate's answer half republishes every one
/// of these as the bait value, cleanly and warm.
#[test]
fn flow_return_leaf_answer_never_binds_a_frame_owned_name_in_owner_scope() {
    let host = make_host();
    for name in [
        "gateStaticMemberOnLocalClass",
        "gateMethodCallOnLocal",
        "gateStaticMemberOnParam",
        "gateArrayElement",
        "gateNestedArrowInArray",
        "gateObjectSpread",
        "gateTypeSpaceLocalClass",
    ] {
        assert_fails_closed(&host, name);
    }

    // The conditional arm RESOLVES — to the frame's own binding, never
    // the owner-scope bait.
    assert_clean_warm(
        &host,
        "gateConditionalArm",
        TypeExpr::Union(std::sync::Arc::from(vec![number_lit(1.0), number_lit(2.0)])),
    );
}

/// The other face: a form the leaf cannot model AT ALL answers a bare
/// `any`, which published CLEAN and WARM for an expression whose value
/// is a frame binding's. Each of these reads THROUGH a frame-owned
/// reference-chain root, so the `any` is a lie.
///
/// tsgo gives `number` for every one of them (`{ z: number }` for the
/// `new` case).
#[test]
fn flow_return_unmodelled_form_read_through_a_frame_binding_fails_closed() {
    let host = make_host();
    for name in [
        "gateComputedMember",
        "gateOptionalChain",
        "gateNewExpression",
        "gateTaggedTemplate",
        "gatePrivateField",
    ] {
        assert_fails_closed(&host, name);
    }
}

/// The positive controls. The gate is about names the FRAME owns, so a
/// genuinely free root still resolves through the owner scope, and a
/// frame-owned name in a position whose value the leaf never consumes
/// (a conditional TEST) is not a leak at all.
#[test]
fn flow_return_root_gate_leaves_free_roots_and_unread_positions_alone() {
    let host = make_host();
    assert_clean_warm(&host, "gateFreeRootResolves", string_lit("FREEOK"));
    assert_clean_warm(
        &host,
        "gateConditionalTestOnParam",
        TypeExpr::union(vec![number_lit(1.0), number_lit(2.0)]),
    );
    let spread = projected(&host, "gateFreeSpreadResolves");
    let TypeExpr::Object(object) = &spread else {
        panic!("a free-rooted spread still lowers structurally: {spread:?}");
    };
    assert_eq!(
        object.properties.len(),
        2,
        "the free spread keeps both members: {object:?}"
    );
}

/// A frame-owned name the OWNER SCOPE answers NOTHING for is not a leak:
/// nothing can be mis-bound, so the leaf evaluates unchanged and its own
/// typed miss carrier is the honest answer. Failing this case closed
/// instead would destroy every legitimate open-program and
/// parameter-shaped surface the substrate already serves.
#[test]
fn flow_return_frame_name_with_no_owner_scope_answer_still_evaluates() {
    let host = make_host();
    let ty = projected(&host, "gateUnansweredFrameNameStaysOpen");
    let TypeExpr::Object(object) = &ty else {
        panic!("an unanswered frame name keeps the structural surface: {ty:?}");
    };
    assert_eq!(
        object.properties.len(),
        2,
        "the spread surface survives: {object:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────
// #9 — a named function expression's own name binds in its own frame
// ──────────────────────────────────────────────────────────────────────

/// `return function dself() { return dself; }` reads the FUNCTION
/// EXPRESSION's own name. That name binds inside its own body and
/// nowhere else, so it is part of the NESTED frame's lexical inventory.
///
/// Before the fix the name was a side channel consulted only by the call
/// arm, so the READ looked FREE and resolved in file owner scope: the
/// fixture's `declare const dself: "bait"` was published as the nested
/// function's return, clean and warm (tsgo: `() => () => any` — the
/// self-reference is circular and has no recoverable body type).
#[test]
fn flow_return_named_function_expression_self_read_never_escapes_to_file_scope() {
    let host = make_host();
    assert_fails_closed(&host, "selfNameRead");
}

/// `function outer() { const g = function h() { return h(); }; }` — the
/// inner self-CALL must not hold on `outer`'s flow slot. A nested
/// function value has no slot of its own, so `DirectSelfCall` is
/// structurally unmintable there and the inner name resolves through the
/// nested frame's own inventory instead.
#[test]
fn flow_return_nested_self_call_never_holds_the_enclosing_slot() {
    let host = make_host();
    with_dispatch(&host, |dispatch| {
        let key = r6_key(dispatch, "selfNameCallHoldsNothingOuter");
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
        else {
            panic!("selfNameCallHoldsNothingOuter must produce a value");
        };
        assert_eq!(
            result.degradation(),
            Some(crate::semantic_query::FlowReturnDegradation::FailedBindingInitializer),
            "the unrecoverable inner self-call degrades where it is OBSERVED"
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            0,
            "a degraded success is ReturnOnly"
        );
    });
}

// ──────────────────────────────────────────────────────────────────────
// #10 — a block-level function declaration is BLOCK-scoped
// ──────────────────────────────────────────────────────────────────────

/// In strict-mode ESM — every carrier surface this substrate serves — a
/// block-level function declaration is BLOCK-scoped; only Annex-B
/// sloppy-mode semantics create the function-scoped alias. Hoisting it
/// unconditionally made a function-scope read resolve to the block's
/// function and FAIL CLOSED, turning a correct clean warm answer into a
/// false `Error(Miss)`.
///
/// tsgo: `blockFunctionIsBlockScoped(c: boolean): "OUTERFN"` — the read
/// at function scope sees the module-scope `fbait`.
#[test]
fn flow_return_block_level_function_declaration_does_not_reach_function_scope() {
    let host = make_host();
    assert_clean_warm(&host, "blockFunctionIsBlockScoped", string_lit("OUTERFN"));
}

/// The two hoisting controls the region gate must not disturb.
///
/// A ROOT-region function declaration still reaches function scope: the
/// read at `return hoistBait()` — written BEFORE the declaration —
/// resolves to it and takes the substrate's documented fail-closed rail
/// for a nested function declaration's own return (tsgo: `number`; the
/// exact recovery is separate substrate debt). The file-scope
/// `declare const hoistBait: () => "OUTERHOISTED"` is what makes this
/// discriminating: without the root-region hoist the name would be FREE
/// and the read would publish `"OUTERHOISTED"`, clean and warm.
///
/// A block `var` still hoists out of its block unconditionally — the
/// region gate narrows the nested-function arm ONLY.
#[test]
fn flow_return_root_function_and_block_var_still_hoist() {
    let host = make_host();
    assert_fails_closed(&host, "rootFunctionStillHoists");
    assert_clean_warm(&host, "blockVarStillHoists", number());
}

// ──────────────────────────────────────────────────────────────────────
// #12 (interim) — the direct-call rail honours the declarator annotation
// ──────────────────────────────────────────────────────────────────────

/// A value declaration carrying BOTH an authored annotation and an
/// initializer is typed by the ANNOTATION: the initializer only has to
/// be assignable to it. Before the fix the direct-call rail resolved the
/// callee through the INITIALIZER's own inferred signature, so
/// `const annotatedFn: () => 42 = () => 42; annotatedFn()` published
/// `number` — confidently and warm — while tsgo says `42`.
///
/// The three shapes that already worked are the controls: a
/// parenthesised callee, an ambient declaration with no initializer, and
/// an arrow with its own return annotation. An UNANNOTATED declarator
/// still infers from its initializer.
#[test]
fn flow_return_direct_call_rail_honours_the_declarator_annotation() {
    let host = make_host();
    for name in [
        "callAnnotatedFn",
        "callAnnotatedParenFn",
        "callAmbientFn",
        "callArrowAnnotatedReturn",
        "callAnnotatedLocal",
    ] {
        assert_clean_warm(&host, name, number_lit(42.0));
    }
    assert_clean_warm(&host, "callUnannotatedFn", number());
}
