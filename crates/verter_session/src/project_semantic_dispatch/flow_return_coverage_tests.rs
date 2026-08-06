//! @ai-generated - Coverage-expansion probes for the demand-sliced
//! `FlowReturn` substrate: the axes six adversarial reviews each recorded
//! in their closing "axes I did not test" section.
//!
//! Every expected value here is anchored against
//! `tsgo 7.0.0-dev.20260526.1 --noEmit --strict --ignoreConfig` through
//! one of two probe forms:
//!
//! - the two-step WRAPPER probe
//!   (`declare const w: ReturnType<typeof f>; const p: null = w;`) for a
//!   concrete return. The one-step `const x: null = f()` form silently
//!   reports nothing whenever the contextual type feeds return-type
//!   inference, and a raw call bound to `const` reads UNWIDENED literals
//!   (`1`, `true`) where the wrapper reads the widened `number` /
//!   `boolean`;
//! - the bidirectional SIGNATURE-IDENTITY probe
//!   (`const ok: <U>(x: U) => Expected = f;` plus a deliberately wrong
//!   `bad:` twin) for a GENERIC return whose binders must survive.
//!   `ReturnType<typeof genericFn>` erases the clause and collapses most
//!   of these to `never`, so it cannot discriminate them at all.
//!
//! Shape assertions run on the GRAPH NODE wherever the projected
//! `TypeExpr` cannot discriminate (a surviving `TypeParam` binder, a
//! `DeclRef` to a module-scope twin, and a deferred `BareRef` all raise
//! to `TypeExpr::Ref { name }`), and every row pins `degradation` plus
//! the family memo's `slot_candidate_count` (0 = `ReturnOnly`,
//! 1 = warm-admitted).
//!
//! `#[ignore]`d rows are CANARIES: each asserts the checker's answer,
//! fails today, and carries the verbatim failure plus the owning layer in
//! its doc comment. They are not aspirational stubs — every body is
//! discriminating, and every one was run un-ignored to confirm it fails
//! for the documented reason.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput, SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{LiteralValue, PrimitiveName, TopLevelOwnerId, TypeExpr};

// ──────────────────────────────────────────────────────────────────────
// Fixtures
// ──────────────────────────────────────────────────────────────────────

const LEAF: &str = "/ws/cov/leaf.ts";
const LEAF_SRC: &str = r#"
export declare function idf<T>(x: T): T;

export function leafInstExpr() {
  return idf<string>;
}

export function leafNewTarget() {
  return new.target;
}

export class LeafSuperBase {
  m(): number {
    return 1;
  }
}
export class LeafSuperDerived extends LeafSuperBase {
  m(): number {
    return super.m();
  }
}

export class LeafPrivIn {
  #x = 1;
  static has(o: object) {
    return #x in o;
  }
}

export class LeafPrivField {
  #x = 1;
  read() {
    return this.#x;
  }
}

export function leafClassExpr() {
  return class {};
}

export function leafImportExpr() {
  return import("/ws/cov/xf/dep");
}

export function* leafGenerator() {
  yield 1;
  return "done";
}

export function leafAssign() {
  let a = 1;
  return (a = 2);
}

export function leafUpdate() {
  let a = 1;
  return a++;
}

export function leafBigInt() {
  return 1n;
}

export function leafRegExp() {
  return /a/;
}

export function leafTemplate() {
  return `x${1}`;
}
"#;

const JSX: &str = "/ws/cov/jsx.tsx";
const JSX_SRC: &str = r#"
declare global {
  namespace JSX {
    interface Element {
      __e: true;
    }
    interface IntrinsicElements {
      div: Record<string, unknown>;
    }
  }
}
export function jsxElem() {
  return <div />;
}
export function jsxFrag() {
  return <></>;
}
export declare function jsxHelper(n: number): "H";
export function jsxAttrCall() {
  return <div data-x={jsxHelper(1)} />;
}
"#;

const CALLS: &str = "/ws/cov/calls.ts";
const CALLS_SRC: &str = r#"
export class CtorC {
  constructor(public v: number) {}
}
export function callNew() {
  return new CtorC(1);
}

export declare const maybeFn: (() => number) | undefined;
export function callOptional() {
  return maybeFn?.();
}

export declare function tag(strings: TemplateStringsArray, ...v: number[]): boolean;
export function callTagged() {
  return tag`a${1}b`;
}

export declare function plainFn(this: void, a: number): string;
export function callDotCall() {
  return plainFn.call(undefined, 1);
}
export function callDotApply() {
  return plainFn.apply(undefined, [1]);
}
export function callDotBind() {
  return plainFn.bind(undefined);
}

export declare function restFn(...xs: number[]): "rest";
export function callRest() {
  return restFn(1, 2, 3);
}

export declare function thisFn(this: { z: number }, a: number): "this";
export function callThisParam() {
  return thisFn.call({ z: 1 }, 1);
}

export declare function asyncSrc(): Promise<number>;
export async function callAwait() {
  return await asyncSrc();
}
export async function callAsyncPlain() {
  return 1;
}
export async function* callAsyncGen() {
  yield 1;
}

export declare function ovlAmbient(a: string): "S";
export declare function ovlAmbient(a: number): "N";
export function callAmbientOverload() {
  return ovlAmbient("a");
}

export declare const ctorSig: { new (a: number): { q: string } };
export function callCtorSigNew() {
  return new ctorSig(1);
}
"#;

const TL: &str = "/ws/cov/tlevel.ts";
const TL_SRC: &str = r#"
export interface HasQ {
  q: string;
}

export function tlPlainMember(x: HasQ) {
  return x.q;
}
export function tlConstrainedMember<T extends HasQ>(x: T) {
  return x.q;
}
export function tlConstrainedIndexed<T extends HasQ>(x: T) {
  return x["q"];
}
export function tlConstrainedWhole<T extends HasQ>(x: T) {
  return x;
}

export function tlInfer<T>(x: T) {
  return null as unknown as T extends Array<infer E> ? E : never;
}

export function tlConditional<T>(x: T) {
  return null as unknown as T extends string ? "yes" : "no";
}

export function tlMapped<T>(x: T) {
  return null as unknown as { [K in keyof T]: number };
}

export function tlKeyof<T>(x: T) {
  return null as unknown as keyof T;
}

export function tlTemplateLit<T extends string>(x: T) {
  return null as unknown as `pre-${T}`;
}

export function ovlImpl(a: string): "IS";
export function ovlImpl(a: number): "IN";
export function ovlImpl(a: string | number): "IS" | "IN" {
  return typeof a === "string" ? "IS" : "IN";
}

export function ovlGen<T>(a: T): { g: T };
export function ovlGen<T, U>(a: T, b: U): { g: T; h: U };
export function ovlGen(a: unknown, b?: unknown): unknown {
  return { g: a, h: b };
}
export function tlCallOvlGen() {
  return ovlGen("a", 1);
}

export const sym: unique symbol = Symbol("s");
export const objSymKey = {
  [sym]() {
    return "symval";
  },
};
export function tlCallSymKeyed() {
  return objSymKey[sym]();
}

export type OmitSrc = {
  keep(): "kept";
  drop(): "dropped";
};
export declare const omitted: Omit<OmitSrc, "drop">;
export function tlCallThroughOmit() {
  return omitted.keep();
}

export function tlObjReturn() {
  return { m: "mv", n: { deep: true } };
}
"#;

const GEO: &str = "/ws/cov/geometry.ts";
const GEO_SRC: &str = r#"
export class GeoClass {
  method() {
    return 1;
  }
  get accessor() {
    return "g";
  }
  set accessor(v: string) {}
  field = () => 2;
  static staticMethod() {
    return "sm";
  }
  constructor(public p: number = 0) {}
}

export const geoObj = {
  objMethod() {
    return "om";
  },
  objArrow: () => "oa",
  get objGet() {
    return true;
  },
};

export function geoDefaultParamArrow(cb = () => 7) {
  return cb;
}

export function geoReturnedArrow() {
  return () => 7;
}

export function geoIifeInside() {
  return (function () {
    return "inner";
  })();
}

function geoDecorator(value: unknown, _ctx: unknown) {
  return value;
}
export class GeoDecorated {
  @geoDecorator
  decorated() {
    return "dec";
  }
}
"#;

// ── Cross-file graph ──────────────────────────────────────────────────

const XF_DEP: &str = "/ws/cov/xf/dep.ts";
const XF_DEP_SRC: &str = r#"
export function depGeneric<T>(x: T) {
  return { g: x };
}
export function depConcrete() {
  return "dep";
}
export declare const depVal: { m(): "dm" };
export type DepAlias = { a: "aa" };
export declare const depAliased: DepAlias;
export interface Widget {
  a: string;
}
export declare const widget: Widget;
"#;

const XF_BARREL: &str = "/ws/cov/xf/barrel.ts";
const XF_BARREL_SRC: &str = r#"
export * from "/ws/cov/xf/dep";
export { depConcrete as reexported } from "/ws/cov/xf/dep";
export type { DepAlias as AliasedType } from "/ws/cov/xf/dep";
"#;

const XF_AUG: &str = "/ws/cov/xf/aug.ts";
const XF_AUG_SRC: &str = r#"
declare module "/ws/cov/xf/dep" {
  interface Widget {
    b: number;
  }
}
export {};
"#;

const XF_MAIN: &str = "/ws/cov/xf/main.ts";
const XF_MAIN_SRC: &str = r#"
import { depGeneric, depConcrete, depVal, depAliased, widget } from "/ws/cov/xf/barrel";
import { reexported } from "/ws/cov/xf/barrel";
import "/ws/cov/xf/aug";

export function xfCallGenericValueRoute() {
  return depGeneric("s");
}
export function xfCallConcrete() {
  return depConcrete();
}
export function xfCallMember() {
  return depVal.m();
}
export function xfReadAliased() {
  return depAliased;
}
export function xfCallReexported() {
  return reexported();
}
export function xfAugmentedMember() {
  return widget.b;
}
"#;

const XF_SCC_A: &str = "/ws/cov/xf/scca.ts";
const XF_SCC_A_SRC: &str = r#"
import { sccB } from "/ws/cov/xf/sccb";
export function sccA(n: number) {
  if (n <= 0) return 0;
  return sccB(n - 1);
}
"#;

const XF_SCC_B: &str = "/ws/cov/xf/sccb.ts";
const XF_SCC_B_SRC: &str = r#"
import { sccA } from "/ws/cov/xf/scca";
export function sccB(n: number) {
  if (n <= 0) return 1;
  return sccA(n - 1);
}
"#;

// ── Carrier fixtures ──────────────────────────────────────────────────

const VUE: &str = "/ws/cov/Setup.vue";
const VUE_SRC: &str = r#"<script lang="ts">
export function moduleFn() {
  return "mod";
}
export function moduleHelper(): "helped" {
  return "helped";
}
</script>
<script setup lang="ts">
const props = defineProps<{ msg: string; count: number }>();
export function setupLit() {
  return 7;
}
export function setupLocal() {
  const v = "loc";
  return v;
}
export function setupCrossOwnerCall() {
  return moduleHelper();
}
export function setupPropsMember() {
  return props.msg;
}
</script>
<template><div>{{ msg }}</div></template>
"#;

const SVELTE: &str = "/ws/cov/Runes.svelte";
const SVELTE_SRC: &str = r#"<script lang="ts">
let { msg, count }: { msg: string; count: number } = $props();
export function svLit() {
  return 7;
}
export function svLocal() {
  const v = "loc";
  return v;
}
export function svPropsRead() {
  return msg;
}
</script>
<div>{msg}</div>
"#;

// ──────────────────────────────────────────────────────────────────────
// Harness
// ──────────────────────────────────────────────────────────────────────

fn lang(canonical: &str) -> crate::FileLanguage {
    crate::LanguageRegistry::global()
        .classify_static(canonical)
        .static_resolution()
}

fn host_with(files: &[(&str, &str)]) -> Arc<VerterHost> {
    let host = Arc::new(VerterHost::new_standalone(HostConfig::default()));
    for (canonical, source) in files {
        let file_language = if canonical.ends_with(".vue") {
            crate::FileLanguage::vue()
        } else {
            lang(canonical)
        };
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some((*canonical).to_string()),
            input_id: (*canonical).to_string(),
            source: Arc::from(*source),
            file_language,
            aliases: Vec::new(),
        });
    }
    host
}

/// Every plain-TypeScript fixture in one host.
fn ts_host() -> Arc<VerterHost> {
    host_with(&[
        (LEAF, LEAF_SRC),
        (JSX, JSX_SRC),
        (CALLS, CALLS_SRC),
        (TL, TL_SRC),
        (GEO, GEO_SRC),
        (XF_DEP, XF_DEP_SRC),
        (XF_BARREL, XF_BARREL_SRC),
        (XF_AUG, XF_AUG_SRC),
        (XF_MAIN, XF_MAIN_SRC),
        (XF_SCC_A, XF_SCC_A_SRC),
        (XF_SCC_B, XF_SCC_B_SRC),
    ])
}

fn carrier_host() -> Arc<VerterHost> {
    host_with(&[(VUE, VUE_SRC), (SVELTE, SVELTE_SRC)])
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

/// The full key, every axis explicit.
fn key_full(
    dispatch: &ProjectSemanticDispatch<'_>,
    canonical: &str,
    owner: TopLevelOwnerId,
    name: &str,
    part: FunctionPartIdentity,
    overload_ordinal: u32,
) -> FlowReturnKey {
    FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(canonical),
            owner,
            Arc::from(name),
            part,
            overload_ordinal,
        ),
        normalized_type_args: Arc::from(Vec::new().into_boxed_slice()),
        context: dispatch.flow_return_context_for(canonical),
        demand: crate::semantic_query::ReturnProjectionDemand::whole_return(),
        input: crate::semantic_query::FlowInputContext::empty(),
    }
}

/// The canonical production point of a top-level `function` declaration
/// in an ordinary file.
fn key_of(dispatch: &ProjectSemanticDispatch<'_>, canonical: &str, name: &str) -> FlowReturnKey {
    key_full(
        dispatch,
        canonical,
        TopLevelOwnerId::ordinary_file(),
        name,
        FunctionPartIdentity::DeclarationBody,
        0,
    )
}

fn member_part(ordinal: u32) -> FunctionPartIdentity {
    FunctionPartIdentity::Member {
        member_path: Arc::from(vec![ordinal].into_boxed_slice()),
    }
}

/// One evaluated function's PUBLIC outcome, with every gate pinned.
#[derive(Debug, PartialEq)]
enum Outcome {
    /// A value: the projected return type, the typed degradation, and
    /// the family memo's candidate count.
    Value {
        ty: TypeExpr,
        degradation: Option<FlowReturnDegradation>,
        candidates: usize,
    },
    /// A typed no-value failure through `Error(Miss)`.
    Miss,
    /// Anything else the dispatch returned (never expected).
    Other(String),
}

fn eval_key_on(
    host: &Arc<VerterHost>,
    dispatch: &ProjectSemanticDispatch<'_>,
    key: FlowReturnKey,
) -> Outcome {
    match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))) {
        QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) => {
            let Some(ty) = host.project_node_to_type_expr_for_test(result.return_type) else {
                return Outcome::Other("the value did not project".to_string());
            };
            let candidates = dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
            Outcome::Value {
                ty,
                degradation: result.degradation,
                candidates,
            }
        }
        QueryResult::Error(QueryError::Miss) => Outcome::Miss,
        other => Outcome::Other(format!("{other:?}")),
    }
}

fn eval(host: &Arc<VerterHost>, canonical: &str, name: &str) -> Outcome {
    with_dispatch(host, |dispatch| {
        let key = key_of(dispatch, canonical, name);
        eval_key_on(host, dispatch, key)
    })
}

fn eval_in(host: &Arc<VerterHost>, canonical: &str, owner: TopLevelOwnerId, name: &str) -> Outcome {
    with_dispatch(host, |dispatch| {
        let key = key_full(
            dispatch,
            canonical,
            owner,
            name,
            FunctionPartIdentity::DeclarationBody,
            0,
        );
        eval_key_on(host, dispatch, key)
    })
}

fn eval_part(
    host: &Arc<VerterHost>,
    canonical: &str,
    name: &str,
    part: FunctionPartIdentity,
    overload_ordinal: u32,
) -> Outcome {
    with_dispatch(host, |dispatch| {
        let key = key_full(
            dispatch,
            canonical,
            TopLevelOwnerId::ordinary_file(),
            name,
            part,
            overload_ordinal,
        );
        eval_key_on(host, dispatch, key)
    })
}

/// Assert one function evaluates CLEAN (no degradation), warm-admissible
/// (exactly one candidate), and to exactly `expected`.
#[track_caller]
fn assert_clean_warm(host: &Arc<VerterHost>, canonical: &str, name: &str, expected: TypeExpr) {
    assert_eq!(
        eval(host, canonical, name),
        Outcome::Value {
            ty: expected,
            degradation: None,
            candidates: 1,
        },
        "{name}"
    );
}

/// Assert one function produces a DEGRADED SUCCESS with the given typed
/// reason, and admits NOTHING.
#[track_caller]
fn assert_degraded(
    host: &Arc<VerterHost>,
    canonical: &str,
    name: &str,
    reason: FlowReturnDegradation,
) {
    match eval(host, canonical, name) {
        Outcome::Value {
            degradation,
            candidates,
            ..
        } => {
            assert_eq!(degradation, Some(reason), "{name} degradation");
            assert_eq!(candidates, 0, "{name} degraded success is ReturnOnly");
        }
        other => panic!("{name} must produce a degraded value, got {other:?}"),
    }
}

/// Assert one function produces NO value and admits nothing.
#[track_caller]
fn assert_fails_closed(host: &Arc<VerterHost>, canonical: &str, name: &str) {
    assert_eq!(eval(host, canonical, name), Outcome::Miss, "{name}");
}

fn number() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::Number)
}

fn string() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::String)
}

fn boolean() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::Boolean)
}

fn string_lit(value: &str) -> TypeExpr {
    TypeExpr::Literal(LiteralValue::String(value.to_string()))
}

/// A bare named type reference with no type arguments.
fn type_ref(name: &str) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    }
}

/// The discriminating GRAPH-NODE shape of one answer. The PROJECTED
/// surface cannot tell a surviving `TypeParam` binder, a `DeclRef` to a
/// module-scope twin, and a deferred `BareRef` apart — all three raise to
/// `TypeExpr::Ref { name }`.
#[derive(Debug, PartialEq, Eq)]
enum NodeShape {
    TypeParam(String),
    DeclRef(String),
    BareRef(String),
    Primitive(PrimitiveKind),
    Opaque,
    Other(String),
}

fn node_shape(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> NodeShape {
    let Some(data) = dispatch.graph().node_data(node) else {
        return NodeShape::Other("<no node>".to_string());
    };
    if let Some((name, _)) = data.bare_ref_head() {
        return NodeShape::BareRef(name.to_string());
    }
    match data.as_ref() {
        SemanticNodeData::Primitive(kind) => NodeShape::Primitive(*kind),
        SemanticNodeData::TypeParam { display_name, .. } => {
            NodeShape::TypeParam(display_name.to_string())
        }
        SemanticNodeData::DeclRef { identity } => {
            NodeShape::DeclRef(identity.decl_name.to_string())
        }
        SemanticNodeData::Opaque(_) => NodeShape::Opaque,
        other => NodeShape::Other(format!("{other:?}")),
    }
}

/// Evaluate one function under the CLEAN + WARM contract and hand its
/// flow-return GRAPH NODE to `pick`.
#[track_caller]
fn flow_node<R>(
    host: &Arc<VerterHost>,
    canonical: &str,
    name: &str,
    pick: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId) -> R,
) -> R {
    with_dispatch(host, |dispatch| {
        let key = key_of(dispatch, canonical, name);
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
        else {
            panic!("{name} must produce a value");
        };
        assert_eq!(result.degradation, None, "{name} must evaluate clean");
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            1,
            "{name} must warm-admit exactly one candidate"
        );
        pick(dispatch, result.return_type)
    })
}

/// The `check` node of a `Conditional` answer.
#[track_caller]
fn conditional_check(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> SemanticNodeId {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Conditional { check, .. }) => *check,
        other => panic!("expected a Conditional answer, got {other:?}"),
    }
}

/// One named member of a projected object answer.
#[track_caller]
fn projected_member<'a>(ty: &'a TypeExpr, key: &str) -> &'a TypeExpr {
    let TypeExpr::Object(object) = ty else {
        panic!("expected an object answer, got {ty:?}");
    };
    object
        .properties
        .iter()
        .find_map(|property| match property {
            verter_type_expr::ObjectMember::Property(p) if p.key.as_string() == Some(key) => {
                Some(&p.ty)
            }
            _ => None,
        })
        .unwrap_or_else(|| panic!("member `{key}` must be present in {ty:?}"))
}

/// The authored return type of a projected function answer.
#[track_caller]
fn projected_function_return(ty: &TypeExpr) -> &TypeExpr {
    let TypeExpr::Function(function) = ty else {
        panic!("expected a function answer, got {ty:?}");
    };
    function
        .return_type
        .as_deref()
        .unwrap_or_else(|| panic!("expected an authored return type in {ty:?}"))
}

#[track_caller]
fn value_of(host: &Arc<VerterHost>, canonical: &str, name: &str) -> TypeExpr {
    match eval(host, canonical, name) {
        Outcome::Value { ty, .. } => ty,
        other => panic!("{name} must produce a value, got {other:?}"),
    }
}

// ──────────────────────────────────────────────────────────────────────
// PRIORITY 1 — carrier surfaces (`.vue` / `.svelte`)
//
// Nothing in the six reviews ever exercised the flow rail through a
// framework carrier: every probe was a plain `.ts` file. Both adapters
// are first-class here.
// ──────────────────────────────────────────────────────────────────────

/// A `<script setup>` function serves the flow rail under the INSTANCE
/// owner, and ONLY under it. The owner axis is real identity, not
/// decoration: the same name under `Module(0)` is a typed no-value
/// outcome, never the setup block's answer.
///
/// Oracle: the projected TS analogues — `function setupLit() { return 7; }`
/// is `number` under `ReturnType<typeof …>` (the wrapper widens the fresh
/// literal; a raw `const x = setupLit()` would read `7`), and
/// `function setupLocal() { const v = "loc"; return v; }` is `string`.
#[test]
fn vue_script_setup_functions_serve_under_the_instance_owner_only() {
    let host = carrier_host();
    assert_eq!(
        eval_in(&host, VUE, TopLevelOwnerId::instance(0), "setupLit"),
        Outcome::Value {
            ty: number(),
            degradation: None,
            candidates: 1,
        }
    );
    assert_eq!(
        eval_in(&host, VUE, TopLevelOwnerId::instance(0), "setupLocal"),
        Outcome::Value {
            ty: string(),
            degradation: None,
            candidates: 1,
        }
    );
    // The MODULE owner does not answer for a setup-block function.
    assert_eq!(
        eval_in(&host, VUE, TopLevelOwnerId::module(0), "setupLit"),
        Outcome::Miss
    );
    assert_eq!(
        eval_in(&host, VUE, TopLevelOwnerId::module(0), "setupLocal"),
        Outcome::Miss
    );
}

/// A Vue `<script>` (module-block) function serves under the MODULE
/// owner, and only under it — the mirror of the setup-block rule. The two
/// blocks of ONE `.vue` file are two distinct flow-rail owners, so a
/// same-named function in each would be two keys, never one.
///
/// Oracle: `function moduleFn() { return "mod"; }` is `string` (widened).
#[test]
fn vue_module_script_functions_serve_under_the_module_owner_only() {
    let host = carrier_host();
    assert_eq!(
        eval_in(&host, VUE, TopLevelOwnerId::module(0), "moduleFn"),
        Outcome::Value {
            ty: string(),
            degradation: None,
            candidates: 1,
        }
    );
    assert_eq!(
        eval_in(&host, VUE, TopLevelOwnerId::instance(0), "moduleFn"),
        Outcome::Miss
    );
}

/// A `<script setup>` function calling a `<script>`-module helper
/// resolves ACROSS the two carrier owners: the setup frame's lexical
/// authority sees the module block's declaration, and the call routes
/// through the shared direct-call carrier to the helper's DECLARED
/// return.
///
/// Oracle: `moduleHelper(): "helped"` is annotated, so the call site
/// reads the declared literal — an annotated return is not a fresh
/// literal, so it does NOT widen.
#[test]
fn vue_setup_call_of_a_module_block_helper_crosses_the_owner_boundary() {
    let host = carrier_host();
    assert_eq!(
        eval_in(
            &host,
            VUE,
            TopLevelOwnerId::instance(0),
            "setupCrossOwnerCall"
        ),
        Outcome::Value {
            ty: string_lit("helped"),
            degradation: None,
            candidates: 1,
        }
    );
}

/// A `.svelte` `<script>` function serves under the INSTANCE owner — the
/// SAME owner Vue's `<script setup>` uses, and NOT the owner Vue's plain
/// `<script>` uses. The adapter asymmetry is deliberate (Svelte's
/// instance script is the component body; Vue's plain `<script>` is the
/// module block) and is pinned here so a registry change that flips it
/// cannot pass silently.
///
/// Oracle: the projected TS analogues — `function svLit() { return 7; }`
/// is `number`; `function svLocal() { const v = "loc"; return v; }` is
/// `string`.
#[test]
fn svelte_script_functions_serve_under_the_instance_owner_not_the_module_owner() {
    let host = carrier_host();
    assert_eq!(
        eval_in(&host, SVELTE, TopLevelOwnerId::instance(0), "svLit"),
        Outcome::Value {
            ty: number(),
            degradation: None,
            candidates: 1,
        }
    );
    assert_eq!(
        eval_in(&host, SVELTE, TopLevelOwnerId::instance(0), "svLocal"),
        Outcome::Value {
            ty: string(),
            degradation: None,
            candidates: 1,
        }
    );
    assert_eq!(
        eval_in(&host, SVELTE, TopLevelOwnerId::module(0), "svLit"),
        Outcome::Miss
    );
    assert_eq!(
        eval_in(&host, SVELTE, TopLevelOwnerId::module(0), "svLocal"),
        Outcome::Miss
    );
}

/// CANARY — a `defineProps<{ msg: string }>()` payload member read from a
/// `<script setup>` function must resolve to the payload's member type.
///
/// Oracle: the projected TS analogue is
/// `function f(props: { msg: string; count: number }) { return props.msg; }`,
/// whose tsgo answer is `string`
/// (`Type 'string' is not assignable to type 'null'.`).
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed
///   left: Value { ty: Unknown(UnknownValue { raw: "semanticMiss", provenance: CompatibilityProjection }), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(String), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the flow evaluator's MEMBER-READ arm
/// (`flow_slice_content::lower_leaf` routes a `StaticMemberExpression`
/// into the leaf fall-through; the evaluator in
/// `project_semantic_dispatch::flow_return` has no projection over the
/// frame-bound root's own annotation). This is NOT carrier-specific — the
/// identical `Opaque(Miss)` lands for a plain `.ts` member read off an
/// annotated parameter (see
/// `member_read_off_an_annotated_parameter_resolves_to_the_member_type`).
/// Note `candidates: 1`: the opaque miss is admitted WARM with
/// `degradation: None`, so nothing downstream marks the enclosing result
/// partial.
#[test]
#[ignore = "a member read off an annotated binding evaluates to Opaque(Miss) and is admitted warm: the flow evaluator has no member projection over a frame-bound leaf root"]
fn vue_define_props_member_read_resolves_to_the_payload_member_type() {
    let host = carrier_host();
    assert_eq!(
        eval_in(&host, VUE, TopLevelOwnerId::instance(0), "setupPropsMember"),
        Outcome::Value {
            ty: string(),
            degradation: None,
            candidates: 1,
        }
    );
}

/// CANARY — a `$props()`-destructured binding read from a `.svelte`
/// instance script must resolve to its destructuring annotation.
///
/// Oracle: the projected TS analogue is
/// `function f(p: { msg: string; count: number }) { const { msg } = p; return msg; }`
/// — tsgo `string`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed
///   left: Value { ty: Unknown(UnknownValue { raw: "semanticMiss", provenance: CompatibilityProjection }), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(String), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the same evaluator arm as the Vue twin. The `$props()`
/// destructuring binds `msg` as a carrier-scope destructured `let`, which
/// the evaluator answers with an opaque miss rather than the
/// annotation's member.
#[test]
#[ignore = "a `$props()`-destructured binding read evaluates to Opaque(Miss) and is admitted warm: the flow evaluator has no destructuring-element arm for a carrier-scope binding"]
fn svelte_runes_props_binding_read_resolves_to_its_annotation() {
    let host = carrier_host();
    assert_eq!(
        eval_in(&host, SVELTE, TopLevelOwnerId::instance(0), "svPropsRead"),
        Outcome::Value {
            ty: string(),
            degradation: None,
            candidates: 1,
        }
    );
}

// ──────────────────────────────────────────────────────────────────────
// PRIORITY 2 — demand modes
//
// Every prior probe used `whole_return()` + `FlowInputContext::empty()`.
// ──────────────────────────────────────────────────────────────────────

/// A MEMBER-PROJECTION demand over an OBJECT-returning function IS
/// modeled: it evaluates the demanded member, cleanly, and admits warm
/// under its own candidate slot. The pre-existing coverage only probed a
/// member demand against a PRIMITIVE-returning function, where the
/// fail-closed arm fires for a different reason (there is no member to
/// project at all), so the modeled arm itself was never exercised.
///
/// Oracle: `function tlObjReturn() { return { m: "mv", n: { deep: true } }; }`
/// — `ReturnType<typeof tlObjReturn>["m"]` is `string` (the fresh object
/// literal's property widens).
#[test]
fn member_projection_demand_over_an_object_return_serves_the_demanded_member() {
    let host = ts_host();
    with_dispatch(&host, |dispatch| {
        let whole = key_of(dispatch, TL, "tlObjReturn");
        let mut member = whole.clone();
        member.demand = crate::semantic_query::ReturnProjectionDemand {
            point: {
                let mut point = crate::semantic_query::demand::Demand::identity();
                point.projection.path =
                    crate::semantic_query::demand::ProjectionPath::from_segments([
                        crate::semantic_query::PathSegment::Member(
                            crate::semantic_query::PropertyKey::identifier(Arc::from("m")),
                        ),
                    ]);
                point
            },
        };
        assert_ne!(whole, member, "the demand axis is identity");

        assert_eq!(
            eval_key_on(&host, dispatch, member),
            Outcome::Value {
                ty: string(),
                degradation: None,
                candidates: 1,
            },
            "the member-projection demand must serve exactly `m`"
        );
    });
}

/// A member-projection demand naming a member the return does NOT carry
/// fails CLOSED — never an `undefined`, never a fabricated member, and
/// never the whole return silently widened back in.
#[test]
fn member_projection_demand_for_an_absent_member_fails_closed() {
    let host = ts_host();
    with_dispatch(&host, |dispatch| {
        let mut key = key_of(dispatch, TL, "tlObjReturn");
        key.demand = crate::semantic_query::ReturnProjectionDemand {
            point: {
                let mut point = crate::semantic_query::demand::Demand::identity();
                point.projection.path =
                    crate::semantic_query::demand::ProjectionPath::from_segments([
                        crate::semantic_query::PathSegment::Member(
                            crate::semantic_query::PropertyKey::identifier(Arc::from("absent")),
                        ),
                    ]);
                point
            },
        };
        assert_eq!(
            eval_key_on(&host, dispatch, key.clone()),
            Outcome::Miss,
            "an absent member is a typed no-value outcome"
        );
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            0,
            "an absent member admits nothing"
        );
    });
}

/// A NON-EMPTY contextual input point fails CLOSED as an unmodeled demand
/// point. The input axis is key identity that production never populates
/// today, so the honest outcome is a typed no-value failure — never the
/// empty-input result served under a different input identity (which
/// would let one re-entry's answer mask another's).
#[test]
fn non_empty_contextual_input_point_fails_closed() {
    let host = ts_host();
    with_dispatch(&host, |dispatch| {
        let mut key = key_of(dispatch, TL, "tlObjReturn");
        let contextual =
            dispatch
                .graph()
                .intern_node(crate::semantic_query::SemanticNodeData::Primitive(
                    crate::semantic_query::PrimitiveKind::Number,
                ));
        key.input = crate::semantic_query::FlowInputContext {
            contextual_parameters: Arc::from(vec![contextual].into_boxed_slice()),
        };
        assert_eq!(eval_key_on(&host, dispatch, key.clone()), Outcome::Miss);
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            0,
            "an unmodeled input point admits nothing"
        );
    });
}

/// CANARY — a MULTI-SEGMENT path demand (`["n"]["deep"]`) over the same
/// object-returning function projects path-precisely.
///
/// Oracle: `ReturnType<typeof tlObjReturn>["n"]["deep"]` is `boolean`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed
///   left: Miss
///  right: Value { ty: Primitive(Boolean), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: `project_semantic_dispatch::flow_return`'s
/// `flow_demanded_member_name`, which destructures the demand path as
/// `let [segment] = path else { return None }` — a single-key gate, not a
/// path walk — and hands the caller the fail-closed arm for anything
/// longer. The SINGLE-segment demand is modeled and serves
/// (`member_projection_demand_over_an_object_return_serves_the_demanded_member`
/// passes), so the failure is specifically the second hop: the demand
/// plan carries one member key and the deeper segment is dropped, and the
/// evaluation falls into the `UnmodeledDemandPoint` fail-closed arm. This
/// is the fail-SAFE direction — never a silently truncated one-hop answer
/// served for a two-hop demand.
#[test]
#[ignore = "only a SINGLE-segment member demand is modeled: a two-segment path demand falls into the UnmodeledDemandPoint fail-closed arm"]
fn multi_segment_path_demand_projects_path_precisely() {
    let host = ts_host();
    with_dispatch(&host, |dispatch| {
        let mut key = key_of(dispatch, TL, "tlObjReturn");
        key.demand = crate::semantic_query::ReturnProjectionDemand {
            point: {
                let mut point = crate::semantic_query::demand::Demand::identity();
                point.projection.path =
                    crate::semantic_query::demand::ProjectionPath::from_segments([
                        crate::semantic_query::PathSegment::Member(
                            crate::semantic_query::PropertyKey::identifier(Arc::from("n")),
                        ),
                        crate::semantic_query::PathSegment::Member(
                            crate::semantic_query::PropertyKey::identifier(Arc::from("deep")),
                        ),
                    ]);
                point
            },
        };
        assert_eq!(
            eval_key_on(&host, dispatch, key),
            Outcome::Value {
                ty: boolean(),
                degradation: None,
                candidates: 1,
            }
        );
    });
}

// ──────────────────────────────────────────────────────────────────────
// PRIORITY 3 — cross-file
// ──────────────────────────────────────────────────────────────────────

/// A callee reached through a BARREL (`export *`) and a callee reached
/// through an ALIASED re-export (`export { x as y }`) both resolve on the
/// symbolic call carrier.
///
/// Oracle: both `ReturnType<typeof xfCallConcrete>` and
/// `ReturnType<typeof xfCallReexported>` are `string` (the callee returns
/// a fresh `"dep"` literal, widened at its own return position).
#[test]
fn barrel_and_aliased_reexport_hops_serve_the_flow_rail() {
    let host = ts_host();
    assert_clean_warm(&host, XF_MAIN, "xfCallConcrete", string());
    assert_clean_warm(&host, XF_MAIN, "xfCallReexported", string());
}

/// A MEMBER CALL on an imported ambient value resolves through the
/// symbolic route to the member's declared return.
///
/// Oracle: `ReturnType<typeof xfCallMember>` is `"dm"` (a declared
/// literal return does not widen).
#[test]
fn imported_ambient_value_member_call_resolves_cross_file() {
    let host = ts_host();
    assert_clean_warm(&host, XF_MAIN, "xfCallMember", string_lit("dm"));
}

/// An imported ALIAS type name read as a value's type stays a SHALLOW
/// `Ref` — the shallow-by-default publication rule holds across the file
/// boundary, so the alias body is never eagerly inlined into the flow
/// answer.
///
/// Oracle: `ReturnType<typeof xfReadAliased>` prints as `DepAlias` — tsc
/// keeps the alias name too.
#[test]
fn imported_alias_type_stays_a_shallow_ref_across_the_file_boundary() {
    let host = ts_host();
    assert_clean_warm(&host, XF_MAIN, "xfReadAliased", type_ref("DepAlias"));
}

/// A CROSS-FILE mutually recursive component fails CLOSED — matching tsc,
/// which refuses to infer it at all.
///
/// Oracle: tsgo reports, for BOTH members,
/// `TS7023: 'sccA' implicitly has return type 'any' because it does not
/// have a return type annotation and is referenced directly or indirectly
/// in one of its return expressions.` The checker's answer is therefore
/// "no inferred type"; a typed no-value outcome is the faithful analogue,
/// and publishing `number` (the base arm alone) would be a warm answer
/// tsc explicitly declines to give.
#[test]
fn cross_file_mutual_recursion_fails_closed_like_tsc_declines_to_infer() {
    let host = ts_host();
    assert_fails_closed(&host, XF_SCC_A, "sccA");
    assert_fails_closed(&host, XF_SCC_B, "sccB");
}

/// CANARY — an imported GENERIC callee must infer its type argument from
/// the call site.
///
/// Oracle: `ReturnType<typeof xfCallGenericValueRoute>` is
/// `{ g: string; }`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: the callee's `T` must be inferred as `string` from the call argument
///   left: Primitive(Unknown)
///  right: Primitive(String)
/// ```
///
/// Owning layer: the call carrier's argument-driven type inference.
/// Adjacent to — but distinct from — the recorded "explicit type
/// arguments collapse to `unknown`" debt: here the type argument is
/// IMPLICIT and inferable from the sole call argument, and it still lands
/// as `unknown`, warm. NOT a cross-file defect: the same-file twin
/// (`same_file_generic_callee_infers_its_type_argument_from_the_call_site`)
/// fails identically, which isolates the missing capability to the shared
/// carrier rather than to the barrel / import hop.
#[test]
#[ignore = "an imported generic callee's IMPLICIT type argument is not inferred from the call argument: the instantiation collapses to `unknown` and is admitted warm"]
fn imported_generic_callee_infers_its_type_argument_from_the_call_site() {
    let host = ts_host();
    let ty = value_of(&host, XF_MAIN, "xfCallGenericValueRoute");
    assert_eq!(
        projected_member(&ty, "g"),
        &string(),
        "the callee's `T` must be inferred as `string` from the call argument"
    );
}

/// CANARY — a member contributed by a cross-file `declare module`
/// AUGMENTATION must be readable from the flow rail.
///
/// Oracle: with `declare module "…/dep" { interface Widget { b: number } }`
/// in scope, tsgo types `function xfAugmentedMember() { return widget.b; }`
/// as `number`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: xfAugmentedMember
///   left: Value { ty: Unknown(UnknownValue { raw: "semanticMiss", provenance: CompatibilityProjection }), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(Number), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the flow evaluator's member-read arm — the SAME
/// `Opaque(Miss)` an UN-augmented member read produces, so this row does
/// not yet discriminate the augmentation stitch itself. It becomes the
/// augmentation-specific canary the moment the member-read arm lands: at
/// that point a passing un-augmented read plus a failing augmented read
/// isolates the stitch.
#[test]
#[ignore = "blocked behind the member-read arm: `widget.b` evaluates to Opaque(Miss) before the augmentation stitch is ever consulted"]
fn cross_file_module_augmentation_member_is_readable_from_the_flow_rail() {
    let host = ts_host();
    assert_clean_warm(&host, XF_MAIN, "xfAugmentedMember", number());
}

// ──────────────────────────────────────────────────────────────────────
// PRIORITY 4 — leaf arms never instantiated
// ──────────────────────────────────────────────────────────────────────

/// A template-literal EXPRESSION in return position widens to `string`.
///
/// Oracle: `ReturnType<typeof leafTemplate>` is `string`.
#[test]
fn template_literal_expression_return_is_string() {
    let host = ts_host();
    assert_clean_warm(&host, LEAF, "leafTemplate", string());
}

/// An `UpdateExpression` (`a++`) in return position carries a WRITE
/// EFFECT the evaluator does not apply, so the result is a DEGRADED
/// SUCCESS that admits nothing. This is the fail-safe half of the
/// `UpdateExpression` arm; the value half is the canary below.
#[test]
fn update_expression_return_degrades_as_an_unapplied_write_effect() {
    let host = ts_host();
    assert_degraded(
        &host,
        LEAF,
        "leafUpdate",
        FlowReturnDegradation::UnappliedWriteEffect,
    );
}

/// CANARY — an `UpdateExpression` in return position is `number`.
///
/// Oracle: `ReturnType<typeof leafUpdate>` is `number`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: leafUpdate
///   left: Value { ty: Primitive(Any), degradation: Some(UnappliedWriteEffect), candidates: 0 }
///  right: Value { ty: Primitive(Number), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: `flow_slice_content`'s leaf arm — `UpdateExpression`
/// falls into `lower_leaf` and no prefix/postfix numeric rule exists. The
/// `ReturnOnly` degradation makes this one fail-SAFE today, unlike the
/// warm `any` rows below.
#[test]
#[ignore = "UpdateExpression has no numeric leaf rule: it evaluates to `any` and degrades as UnappliedWriteEffect"]
fn update_expression_return_is_number() {
    let host = ts_host();
    assert_clean_warm(&host, LEAF, "leafUpdate", number());
}

/// CANARY — a `TSInstantiationExpression` (`f<string>` with no call) in
/// return position is the INSTANTIATED signature.
///
/// Oracle: `ReturnType<typeof leafInstExpr>` is `(x: string) => string`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// expected a function answer, got Primitive(Any)
/// ```
///
/// Owning layer: `flow_slice_content::lower_leaf` —
/// `TSInstantiationExpression` sits in the leaf fall-through set with no
/// instantiation rule. Note that the resulting `any` is admitted WARM
/// with `degradation: None`.
#[test]
#[ignore = "TSInstantiationExpression has no leaf rule: the instantiated signature evaluates to `any` and is admitted warm"]
fn instantiation_expression_return_is_the_instantiated_signature() {
    let host = ts_host();
    let ty = value_of(&host, LEAF, "leafInstExpr");
    assert_eq!(
        projected_function_return(&ty),
        &string(),
        "`idf<string>` returns `string`"
    );
}

/// CANARY — a `BigIntLiteral` in return position is `bigint`.
///
/// Oracle: `ReturnType<typeof leafBigInt>` is `bigint`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: leafBigInt
///   left: Value { ty: Primitive(Any), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(BigInt), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the flow evaluator's literal rules — `BigIntLiteral` has
/// no arm, so the shallow leaf answers `any` and it is admitted WARM.
#[test]
#[ignore = "BigIntLiteral has no literal rule: it evaluates to `any` and is admitted warm"]
fn bigint_literal_return_is_bigint() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        LEAF,
        "leafBigInt",
        TypeExpr::Primitive(PrimitiveName::BigInt),
    );
}

/// CANARY — a `RegExpLiteral` in return position is `RegExp`.
///
/// Oracle: `ReturnType<typeof leafRegExp>` is `RegExp`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: leafRegExp
///   left: Value { ty: Primitive(Any), degradation: None, candidates: 1 }
///  right: Value { ty: Ref { name: "RegExp", type_arguments: [] }, degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the flow evaluator's literal rules — no `RegExpLiteral`
/// arm; the intrinsic `RegExp` lib type is never consulted. Admitted WARM.
#[test]
#[ignore = "RegExpLiteral has no literal rule: it evaluates to `any` and is admitted warm"]
fn regexp_literal_return_is_the_regexp_lib_type() {
    let host = ts_host();
    assert_clean_warm(&host, LEAF, "leafRegExp", type_ref("RegExp"));
}

/// CANARY — an `AssignmentExpression` (`(a = 2)`) in return position is
/// the assigned value's type.
///
/// Oracle: `ReturnType<typeof leafAssign>` is `number`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: leafAssign
///   left: Value { ty: Primitive(Any), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(Number), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: `flow_slice_content::lower_leaf`. Note the contrast with
/// `leafUpdate`: the assignment form produces the SAME `any` but is
/// admitted WARM with no degradation, while the update form degrades.
#[test]
#[ignore = "AssignmentExpression has no leaf rule: it evaluates to `any` and — unlike UpdateExpression — is admitted warm with no degradation"]
fn assignment_expression_return_is_the_assigned_type() {
    let host = ts_host();
    assert_clean_warm(&host, LEAF, "leafAssign", number());
}

/// CANARY — a `ClassExpression` in return position is the anonymous
/// class's constructor type, not `any`.
///
/// Oracle: `ReturnType<typeof leafClassExpr>` is
/// `typeof (Anonymous class)`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left != right` failed: leafClassExpr must not be `any`
///   left: Primitive(Any)
///  right: Primitive(Any)
/// ```
///
/// Owning layer: `flow_slice_content::lower_leaf`. The assertion is
/// negative because the typed IR has no stable spelling for an anonymous
/// class's constructor type; `!= any` still discriminates exactly the
/// defect — it fails today and passes on any real answer.
#[test]
#[ignore = "ClassExpression has no leaf rule: it evaluates to `any` and is admitted warm"]
fn class_expression_return_is_not_any() {
    let host = ts_host();
    assert_ne!(
        value_of(&host, LEAF, "leafClassExpr"),
        TypeExpr::Primitive(PrimitiveName::Any),
        "leafClassExpr must not be `any`"
    );
}

/// CANARY — an `ImportExpression` (dynamic `import(...)`) in return
/// position is a `Promise` of the module's namespace type.
///
/// Oracle: `ReturnType<typeof leafImportExpr>` is
/// `Promise<typeof import("…/dep")>`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left != right` failed: leafImportExpr must not be `any`
///   left: Primitive(Any)
///  right: Primitive(Any)
/// ```
///
/// Owning layer: `flow_slice_content::lower_leaf`. Negative assertion for
/// the same reason as the class-expression row — a module-namespace type
/// has no stable typed-IR spelling here.
#[test]
#[ignore = "ImportExpression has no leaf rule: the dynamic-import promise evaluates to `any` and is admitted warm"]
fn dynamic_import_expression_return_is_not_any() {
    let host = ts_host();
    assert_ne!(
        value_of(&host, LEAF, "leafImportExpr"),
        TypeExpr::Primitive(PrimitiveName::Any),
        "leafImportExpr must not be `any`"
    );
}

/// CANARY — a `MetaProperty` (`new.target`) in return position is not
/// `any`.
///
/// Oracle: `ReturnType<typeof leafNewTarget>` prints as
/// `() => typeof leafNewTarget` — `new.target` inside `f` is typed as
/// `typeof f`, so the wrapper's answer is the function type itself.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left != right` failed: leafNewTarget must not be `any`
///   left: Primitive(Any)
///  right: Primitive(Any)
/// ```
///
/// Owning layer: `flow_slice_content::lower_leaf` — `MetaProperty` is in
/// the leaf fall-through set with no `new.target` rule, so the shallow
/// pass answers `any`, admitted warm.
#[test]
#[ignore = "MetaProperty (`new.target`) has no arm: it evaluates to `any` and is admitted warm"]
fn meta_property_new_target_return_is_not_any() {
    let host = ts_host();
    assert_ne!(
        value_of(&host, LEAF, "leafNewTarget"),
        TypeExpr::Primitive(PrimitiveName::Any),
        "leafNewTarget must not be `any`"
    );
}

/// CANARY — a `super.m()` call in a derived class method resolves to the
/// base member's declared return.
///
/// Oracle: `ReturnType<typeof LeafSuperDerived.prototype.m>` is `number`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed
///   left: Value { ty: Primitive(Any), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(Number), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the flow evaluator's call carrier — a `Super` callee
/// root has no arm, so the shallow leaf's `any` is published warm rather
/// than routed to the base class's member.
#[test]
#[ignore = "a `super.m()` callee root has no arm: the call evaluates to `any` and is admitted warm"]
fn super_method_call_return_resolves_to_the_base_member() {
    let host = ts_host();
    assert_eq!(
        eval_part(&host, LEAF, "LeafSuperDerived", member_part(0), 0),
        Outcome::Value {
            ty: number(),
            degradation: None,
            candidates: 1,
        }
    );
}

/// CANARY — a `PrivateInExpression` (`#x in o`) in return position is
/// `boolean`.
///
/// Oracle: `ReturnType<typeof LeafPrivIn.has>` is `boolean`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed
///   left: Value { ty: Primitive(Any), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(Boolean), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: `flow_slice_content::lower_leaf` — `PrivateInExpression`
/// is in the leaf fall-through set with no `in`-operator rule. Member
/// ordinal 1 is the static `has`; ordinal 0 is the `#x` field, which
/// correctly misses because a field is not a callable member.
#[test]
#[ignore = "PrivateInExpression has no leaf rule: the `in` test evaluates to `any` and is admitted warm"]
fn private_in_expression_return_is_boolean() {
    let host = ts_host();
    assert_eq!(
        eval_part(&host, LEAF, "LeafPrivIn", member_part(1), 0),
        Outcome::Value {
            ty: boolean(),
            degradation: None,
            candidates: 1,
        }
    );
}

/// CANARY — a `PrivateFieldExpression` (`this.#x`) in return position is
/// the field's type.
///
/// Oracle: `ReturnType<typeof LeafPrivField.prototype.read>` is `number`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed
///   left: Value { ty: Primitive(Any), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(Number), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: `flow_slice_content::lower_leaf` —
/// `PrivateFieldExpression` is in the leaf fall-through set; the
/// enclosing class's private-field table is never consulted.
#[test]
#[ignore = "PrivateFieldExpression has no leaf rule: `this.#x` evaluates to `any` and is admitted warm"]
fn private_field_expression_return_is_the_field_type() {
    let host = ts_host();
    assert_eq!(
        eval_part(&host, LEAF, "LeafPrivField", member_part(1), 0),
        Outcome::Value {
            ty: number(),
            degradation: None,
            candidates: 1,
        }
    );
}

/// CANARY — a `function*` generator's return type is
/// `Generator<Yield, Return, Next>`, not the bare `return` expression's
/// type.
///
/// Oracle: `ReturnType<typeof leafGenerator>` is
/// `Generator<number, string, unknown>`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left != right` failed: a generator's return must not be the bare `return` type
///   left: Primitive(String)
///  right: Primitive(String)
/// ```
///
/// Owning layer: the flow evaluator publishes the raw `return`
/// contributor join with NO generator wrapping — no `is_generator`
/// consultation exists anywhere on the flow path. This is a WARM WRONG
/// answer, not merely an imprecise one: a consumer reading
/// `ReturnType<typeof leafGenerator>` gets `string` where the language
/// says `Generator<number, string, unknown>`.
#[test]
#[ignore = "generator functions are not wrapped: the flow rail publishes the bare `return` join as the function's return type, warm"]
fn generator_return_is_wrapped_in_generator() {
    let host = ts_host();
    assert_ne!(
        value_of(&host, LEAF, "leafGenerator"),
        string(),
        "a generator's return must not be the bare `return` type"
    );
}

/// CANARY — a JSX element / fragment in return position is the configured
/// `JSX.Element`.
///
/// Oracle: with `declare global { namespace JSX { interface Element … } }`
/// in scope, tsgo types all three of `ReturnType<typeof jsxElem>`,
/// `ReturnType<typeof jsxFrag>` and `ReturnType<typeof jsxAttrCall>` as
/// `Element`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left != right` failed: jsxElem must not be `any`
///   left: Primitive(Any)
///  right: Primitive(Any)
/// ```
///
/// Owning layer: `flow_slice_content::lower_leaf` — `JSXElement` and
/// `JSXFragment` are in the leaf fall-through set. The third case is the
/// interesting one: `jsxAttrCall` embeds a CALL inside a JSX attribute,
/// and that call is swallowed by the same `any` rather than reaching the
/// `UnmodeledCallPosition` fail-closed rail the call-carrier gate
/// promises for a call with no structural arm.
#[test]
#[ignore = "JSXElement / JSXFragment have no leaf rule: they evaluate to `any` and are admitted warm, and a call embedded in a JSX attribute never reaches the call-carrier gate"]
fn jsx_element_fragment_and_attribute_call_returns_are_not_any() {
    let host = ts_host();
    for name in ["jsxElem", "jsxFrag", "jsxAttrCall"] {
        assert_ne!(
            value_of(&host, JSX, name),
            TypeExpr::Primitive(PrimitiveName::Any),
            "{name} must not be `any`"
        );
    }
}

// ──────────────────────────────────────────────────────────────────────
// PRIORITY 5 — call forms outside the modeled vocabulary
// ──────────────────────────────────────────────────────────────────────

/// A REST / variadic callee resolves normally — the parameter shape does
/// not block the direct-call carrier.
///
/// Oracle: `ReturnType<typeof callRest>` is `"rest"`.
#[test]
fn rest_parameter_callee_resolves_on_the_direct_call_carrier() {
    let host = ts_host();
    assert_clean_warm(&host, CALLS, "callRest", string_lit("rest"));
}

/// `.call` / `.apply` / `.bind` callees, and a `this`-parameter callee
/// reached through `.call`, all degrade as an UNREPRESENTABLE CALLEE and
/// admit NOTHING. This is the fail-safe arm: the substrate declines to
/// model the `Function.prototype` indirection rather than publishing the
/// bare member's own return.
///
/// Oracle (for the record — the answers the fail-closed arm declines to
/// produce): `string`, `string`, `(a: number) => string`, and `"this"`
/// respectively.
#[test]
fn call_apply_bind_and_this_parameter_callees_degrade_as_unrepresentable() {
    let host = ts_host();
    for name in [
        "callDotCall",
        "callDotApply",
        "callDotBind",
        "callThisParam",
    ] {
        assert_degraded(
            &host,
            CALLS,
            name,
            FlowReturnDegradation::UnrepresentableCallee,
        );
    }
}

/// An AMBIENT OVERLOAD GROUP callee degrades as an unrepresentable callee
/// and admits nothing. The declaration index keeps one entry per name
/// while the language picks the first MATCHING signature, so the
/// substrate declines rather than publishing whichever entry the index
/// happens to hold.
///
/// Oracle (for the record): `ReturnType<typeof callAmbientOverload>` is
/// `"S"`.
#[test]
fn ambient_overload_group_callee_degrades_rather_than_guessing_a_signature() {
    let host = ts_host();
    assert_degraded(
        &host,
        CALLS,
        "callAmbientOverload",
        FlowReturnDegradation::UnrepresentableCallee,
    );
}

/// A GENERIC overload group callee degrades identically — the pair
/// `<T>(a: T)` / `<T, U>(a: T, b: U)` is not resolved by arity.
///
/// Oracle (for the record): `ReturnType<typeof tlCallOvlGen>` is
/// `{ g: string; h: number; }`.
#[test]
fn generic_overload_group_callee_degrades_as_unrepresentable() {
    let host = ts_host();
    assert_degraded(
        &host,
        TL,
        "tlCallOvlGen",
        FlowReturnDegradation::UnrepresentableCallee,
    );
}

/// CANARY — a `new` expression's return is the constructed instance type.
///
/// Oracle: `ReturnType<typeof callNew>` is `CtorC`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: callNew
///   left: Value { ty: Primitive(Any), degradation: None, candidates: 1 }
///  right: Value { ty: Ref { name: "CtorC", type_arguments: [] }, degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: `flow_slice_content::lower_leaf` — `NewExpression` is in
/// the leaf fall-through set. Critically it is admitted WARM: the
/// call-carrier gate's `UnreducedCallValue` → `UnmodeledCallPosition`
/// fail-closed rail is never reached, because the shallow leaf answers a
/// plain `any` that carries no call-return carrier to detect.
#[test]
#[ignore = "NewExpression has no construct-call arm: it evaluates to `any` and is admitted warm instead of failing closed as an unmodeled call position"]
fn construct_expression_return_is_the_instance_type() {
    let host = ts_host();
    assert_clean_warm(&host, CALLS, "callNew", type_ref("CtorC"));
}

/// CANARY — a CONSTRUCT-SIGNATURE call (`new ctorSig(1)` where `ctorSig`
/// is a value carrying a `new (…)` signature) returns the signature's
/// instance type.
///
/// Oracle: `ReturnType<typeof callCtorSigNew>` is `{ q: string; }`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left != right` failed: callCtorSigNew must not be `any`
///   left: Primitive(Any)
///  right: Primitive(Any)
/// ```
///
/// Owning layer: same as the `new` row above — the construct-signature
/// group is never consulted.
#[test]
#[ignore = "construct signatures are never consulted: `new ctorSig(1)` evaluates to `any` and is admitted warm"]
fn construct_signature_call_return_is_the_signature_instance_type() {
    let host = ts_host();
    assert_ne!(
        value_of(&host, CALLS, "callCtorSigNew"),
        TypeExpr::Primitive(PrimitiveName::Any),
        "callCtorSigNew must not be `any`"
    );
}

/// CANARY — an OPTIONAL-CHAINED call (`maybeFn?.()`) returns the callee's
/// return unioned with `undefined`.
///
/// Oracle: `ReturnType<typeof callOptional>` is `number | undefined`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left != right` failed: callOptional must not be `any`
///   left: Primitive(Any)
///  right: Primitive(Any)
/// ```
///
/// Owning layer: `flow_slice_content::lower_leaf` — `ChainExpression` is
/// in the leaf fall-through set, so the optional call never reaches the
/// call carrier and the `| undefined` arm is never synthesised.
#[test]
#[ignore = "ChainExpression has no optional-call arm: `f?.()` evaluates to `any` and is admitted warm"]
fn optional_chained_call_return_unions_undefined() {
    let host = ts_host();
    assert_ne!(
        value_of(&host, CALLS, "callOptional"),
        TypeExpr::Primitive(PrimitiveName::Any),
        "callOptional must not be `any`"
    );
}

/// CANARY — a TAGGED TEMPLATE call returns the tag function's return.
///
/// Oracle: `ReturnType<typeof callTagged>` is `boolean`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: callTagged
///   left: Value { ty: Primitive(Any), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(Boolean), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: `flow_slice_content::lower_leaf` —
/// `TaggedTemplateExpression` is in the leaf fall-through set, and (like
/// `NewExpression`) the `any` it produces is admitted WARM rather than
/// routed to the call-carrier fail-closed rail.
#[test]
#[ignore = "TaggedTemplateExpression has no call arm: the tag call evaluates to `any` and is admitted warm"]
fn tagged_template_call_return_is_the_tag_return() {
    let host = ts_host();
    assert_clean_warm(&host, CALLS, "callTagged", boolean());
}

/// CANARY — an `async function`'s return type is `Promise<T>`, not `T`.
///
/// Oracle: `ReturnType<typeof callAsyncPlain>` is `Promise<number>` for
/// `async function callAsyncPlain() { return 1; }`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left != right` failed: an async function's return must not be the bare body type
///   left: Primitive(Number)
///  right: Primitive(Number)
/// ```
///
/// Owning layer: the flow evaluator publishes the raw `return`
/// contributor join with NO `Promise` wrapping — `is_async` is not
/// consulted anywhere on the flow path (neither
/// `verter_session/src/flow_slice_content.rs`,
/// `verter_session/src/project_semantic_dispatch/flow_return*.rs`, nor
/// `verter_semantic/src/analysis/flow/**` reference it). This is a WARM
/// WRONG answer at a public consumer boundary: a downstream
/// `ReturnType<typeof asyncFn>` reads `number` where the language says
/// `Promise<number>`, and the enclosing composition is never marked
/// partial because `degradation` is `None`.
#[test]
#[ignore = "async functions are not wrapped: the flow rail publishes the bare body join as the function's return type, warm"]
fn async_function_return_is_wrapped_in_promise() {
    let host = ts_host();
    assert_ne!(
        value_of(&host, CALLS, "callAsyncPlain"),
        number(),
        "an async function's return must not be the bare body type"
    );
}

/// CANARY — an `async function*` returns `AsyncGenerator<…>`.
///
/// Oracle: `ReturnType<typeof callAsyncGen>` is
/// `AsyncGenerator<number, void, unknown>`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left != right` failed: an async generator's return must not be `void`
///   left: Primitive(Void)
///  right: Primitive(Void)
/// ```
///
/// Owning layer: the same missing wrapping as the async and generator
/// rows — the body's fall-through `void` is published warm.
#[test]
#[ignore = "async generators are not wrapped: the flow rail publishes the body's fall-through `void`, warm"]
fn async_generator_return_is_wrapped_in_async_generator() {
    let host = ts_host();
    assert_ne!(
        value_of(&host, CALLS, "callAsyncGen"),
        TypeExpr::Primitive(PrimitiveName::Void),
        "an async generator's return must not be `void`"
    );
}

/// CANARY — an AWAITED call inside an `async` function.
///
/// Oracle: `ReturnType<typeof callAwait>` is `Promise<number>` for
/// `async function callAwait() { return await asyncSrc(); }`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left != right` failed: callAwait must not be `any`
///   left: Primitive(Any)
///  right: Primitive(Any)
/// ```
///
/// Owning layer: TWO independent gaps compose here — `AwaitExpression` is
/// in `lower_leaf`'s fall-through set (so the awaited call never reaches
/// the call carrier and never unwraps its `Promise`), and the enclosing
/// `async` is never re-wrapped. Fixing only the async wrapping would turn
/// this into `Promise<any>`, still wrong.
#[test]
#[ignore = "AwaitExpression has no leaf rule and the enclosing async is not wrapped: the awaited call evaluates to `any` and is admitted warm"]
fn awaited_call_return_is_the_awaited_value_wrapped_again() {
    let host = ts_host();
    assert_ne!(
        value_of(&host, CALLS, "callAwait"),
        TypeExpr::Primitive(PrimitiveName::Any),
        "callAwait must not be `any`"
    );
}

// ──────────────────────────────────────────────────────────────────────
// PRIORITY 6 — type-level axes
// ──────────────────────────────────────────────────────────────────────

/// A CONSTRAINED clause parameter (`<T extends HasQ>`) survives the
/// whole-return route as a BINDER carrying its constraint — not as a
/// `DeclRef` to a module-scope twin, and not erased to the constraint.
///
/// Asserted on the GRAPH NODE: a surviving `TypeParam` and a `DeclRef` to
/// a same-named module declaration both project to
/// `TypeExpr::Ref { name: "T" }`, so the projection alone cannot
/// discriminate.
///
/// Oracle: `<T extends HasQ>(x: T) => T` — the identity of the clause is
/// exactly what `const ok: <U extends HasQ>(x: U) => U = tlConstrainedWhole;`
/// accepts. (Return-position bivariance makes the `bad:` twin compile
/// here, which is precisely why the discriminating assertion is the graph
/// node.)
#[test]
fn constrained_clause_parameter_survives_the_whole_return_as_a_binder() {
    let host = ts_host();
    flow_node(&host, TL, "tlConstrainedWhole", |dispatch, node| {
        assert_eq!(
            node_shape(dispatch, node),
            NodeShape::TypeParam("T".to_string()),
            "the constrained binder must survive as a TypeParam node"
        );
    });
    // The constraint travels with it.
    let ty = value_of(&host, TL, "tlConstrainedWhole");
    let TypeExpr::TypeParameter(param) = &ty else {
        panic!("expected a projected TypeParameter, got {ty:?}");
    };
    assert_eq!(
        param.constraint.as_deref(),
        Some(&type_ref("HasQ")),
        "the clause constraint must ride the binder"
    );
}

/// A return-position CONDITIONAL over an unbound clause parameter stays
/// OPEN — the check node is the surviving binder, and neither branch
/// collapses.
///
/// Oracle (bidirectional signature identity):
/// `const ok: <U>(x: U) => U extends string ? "yes" : "no" = tlConditional;`
/// compiles, while `const bad: <U>(x: U) => "yes" = tlConditional;` fails
/// with `TS2322: Type '<T>(x: T) => T extends string ? "yes" : "no"' is
/// not assignable to type '<U>(x: U) => "yes"'.`
#[test]
fn return_position_conditional_stays_open_over_its_clause_binder() {
    let host = ts_host();
    flow_node(&host, TL, "tlConditional", |dispatch, node| {
        let check = conditional_check(dispatch, node);
        assert_eq!(
            node_shape(dispatch, check),
            NodeShape::TypeParam("T".to_string()),
            "the conditional's check must be the surviving binder"
        );
    });
    let ty = value_of(&host, TL, "tlConditional");
    let TypeExpr::Conditional {
        true_type,
        false_type,
        ..
    } = &ty
    else {
        panic!("expected a Conditional, got {ty:?}");
    };
    assert_eq!(true_type.as_ref(), &string_lit("yes"));
    assert_eq!(false_type.as_ref(), &string_lit("no"));
}

/// A return-position `infer` inside a conditional keeps its `Infer`
/// binder in BOTH the `extends` clause and the true branch.
///
/// Oracle (bidirectional signature identity):
/// `const ok: <U>(x: U) => U extends Array<infer E> ? E : never = tlInfer;`
/// compiles, while `const bad: <U>(x: U) => U = tlInfer;` fails with
/// `TS2322: Type '<T>(x: T) => T extends (infer E)[] ? E : never' is not
/// assignable to type '<U>(x: U) => U'.`
#[test]
fn return_position_infer_keeps_its_binder_in_both_positions() {
    let host = ts_host();
    let ty = value_of(&host, TL, "tlInfer");
    let TypeExpr::Conditional {
        check,
        extends,
        true_type,
        false_type,
    } = &ty
    else {
        panic!("expected a Conditional, got {ty:?}");
    };
    assert!(
        matches!(check.as_ref(), TypeExpr::TypeParameter(p) if p.name == "T"),
        "the check must stay the clause binder, got {check:?}"
    );
    let TypeExpr::Array { element, .. } = extends.as_ref() else {
        panic!("expected an Array extends clause, got {extends:?}");
    };
    assert!(
        matches!(element.as_ref(), TypeExpr::Infer { name } if name == "E"),
        "the `infer E` binder must survive in the extends clause"
    );
    assert!(
        matches!(true_type.as_ref(), TypeExpr::Infer { name } if name == "E"),
        "the true branch must reference the same `infer` binder"
    );
    assert_eq!(
        false_type.as_ref(),
        &TypeExpr::Primitive(PrimitiveName::Never)
    );
}

/// A return-position MAPPED type keeps its own `K` parameter and its
/// `keyof T` source over the clause binder.
///
/// Oracle (bidirectional signature identity):
/// `const ok: <U>(x: U) => { [K in keyof U]: number } = tlMapped;`
/// compiles, while `const bad: <U>(x: U) => U = tlMapped;` fails with
/// `TS2322: Type '<T>(x: T) => { [K in keyof T]: number; }' is not
/// assignable to type '<U>(x: U) => U'.`
#[test]
fn return_position_mapped_type_keeps_its_parameter_and_keyof_source() {
    let host = ts_host();
    let ty = value_of(&host, TL, "tlMapped");
    let TypeExpr::Mapped {
        parameter,
        source,
        value,
        ..
    } = &ty
    else {
        panic!("expected a Mapped type, got {ty:?}");
    };
    assert_eq!(parameter, "K");
    let TypeExpr::KeyOf(inner) = source.as_ref() else {
        panic!("expected a KeyOf source, got {source:?}");
    };
    assert!(
        matches!(inner.as_ref(), TypeExpr::TypeParameter(p) if p.name == "T"),
        "the mapped source must be `keyof T` over the clause binder"
    );
    assert_eq!(value.as_ref(), &number());
}

/// A return-position `keyof T` stays a `KeyOf` over the surviving binder.
///
/// Oracle (bidirectional signature identity):
/// `const ok: <U>(x: U) => keyof U = tlKeyof;` compiles, while
/// `const bad: <U>(x: U) => string = tlKeyof;` fails with
/// `TS2322: Type '<T>(x: T) => keyof T' is not assignable to type
/// '<U>(x: U) => string'.`
#[test]
fn return_position_keyof_stays_open_over_its_clause_binder() {
    let host = ts_host();
    let ty = value_of(&host, TL, "tlKeyof");
    let TypeExpr::KeyOf(inner) = &ty else {
        panic!("expected a KeyOf, got {ty:?}");
    };
    assert!(
        matches!(inner.as_ref(), TypeExpr::TypeParameter(p) if p.name == "T"),
        "keyof must range over the surviving binder"
    );
}

/// A return-position TEMPLATE-LITERAL type keeps its quasis and its
/// binder-valued expression slot.
///
/// Oracle (bidirectional signature identity):
/// ``const ok: <U extends string>(x: U) => `pre-${U}` = tlTemplateLit;``
/// compiles, while
/// `const bad: <U extends string>(x: U) => U = tlTemplateLit;` fails with
/// ``TS2322: Type '<T extends string>(x: T) => `pre-${T}`' is not
/// assignable to type '<U extends string>(x: U) => U'.``
#[test]
fn return_position_template_literal_type_keeps_its_binder_slot() {
    let host = ts_host();
    let ty = value_of(&host, TL, "tlTemplateLit");
    let TypeExpr::TemplateLiteral {
        quasis,
        expressions,
    } = &ty
    else {
        panic!("expected a TemplateLiteral type, got {ty:?}");
    };
    assert_eq!(quasis, &vec!["pre-".to_string(), String::new()]);
    assert_eq!(expressions.len(), 1);
    assert!(
        matches!(&expressions[0], TypeExpr::TypeParameter(p) if p.name == "T"),
        "the template's expression slot must hold the surviving binder"
    );
}

/// An OVERLOAD GROUP's ordinal axis: each BODILESS signature is a typed
/// no-value outcome (there is no body to flow-analyse), and only the
/// implementation ordinal serves. `overload_ordinal > 0` was never
/// exercised before this row.
///
/// Oracle: `ovlImpl`'s implementation is annotated `"IS" | "IN"`, so the
/// union is the language's answer for the implementation signature —
/// which the public overload projection then hides, a projection-time
/// rule rather than a flow-rail one.
#[test]
fn overload_group_ordinals_serve_only_the_implementation() {
    let host = ts_host();
    for ordinal in [0u32, 1] {
        assert_eq!(
            eval_part(
                &host,
                TL,
                "ovlImpl",
                FunctionPartIdentity::DeclarationBody,
                ordinal
            ),
            Outcome::Miss,
            "bodiless overload signature #{ordinal} has no flow body"
        );
    }
    assert_eq!(
        eval_part(
            &host,
            TL,
            "ovlImpl",
            FunctionPartIdentity::DeclarationBody,
            2
        ),
        Outcome::Value {
            ty: TypeExpr::Union(Arc::from(
                vec![string_lit("IS"), string_lit("IN")].into_boxed_slice()
            )),
            degradation: None,
            candidates: 1,
        },
        "the implementation ordinal serves the body join"
    );
}

/// A member call reached through the `Omit` utility resolves
/// path-precisely to the surviving member's return — the source type is
/// never whole-materialised.
///
/// Oracle: `ReturnType<typeof tlCallThroughOmit>` is `"kept"`.
#[test]
fn member_call_through_the_omit_utility_resolves_path_precisely() {
    let host = ts_host();
    assert_clean_warm(&host, TL, "tlCallThroughOmit", string_lit("kept"));
}

/// CANARY — a SYMBOL-KEYED member call resolves to the member's return.
///
/// Oracle: `ReturnType<typeof tlCallSymKeyed>` is `string` (the object
/// method's fresh `"symval"` literal widens at its own return position).
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: tlCallSymKeyed
///   left: Value { ty: Primitive(Any), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(String), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the flow evaluator's computed-member arm — a
/// `unique symbol` key is a `ComputedMemberExpression`, which falls into
/// `lower_leaf` with no key-resolution rule, and the resulting `any` is
/// admitted warm rather than routed through the call-carrier gate.
#[test]
#[ignore = "a `unique symbol` computed member key has no resolution rule: the member call evaluates to `any` and is admitted warm"]
fn symbol_keyed_member_call_resolves_to_the_member_return() {
    let host = ts_host();
    assert_clean_warm(&host, TL, "tlCallSymKeyed", string());
}

/// CANARY — a member read off an ANNOTATED parameter resolves to the
/// member's type. This is the ROOT of the `Opaque(Miss)` family: the two
/// carrier rows and the augmentation row above are the same defect seen
/// through different front doors.
///
/// Oracle: `function tlPlainMember(x: HasQ) { return x.q; }` — tsgo
/// `ReturnType<typeof tlPlainMember>` is `string`
/// (`Type 'string' is not assignable to type 'null'.`).
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: tlPlainMember
///   left: Value { ty: Unknown(UnknownValue { raw: "semanticMiss", provenance: CompatibilityProjection }), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(String), degradation: None, candidates: 1 }
/// ```
///
/// The graph node is `Opaque(Miss)` and the candidate count is 1: the
/// miss is admitted WARM with `degradation: None`, so no consumer
/// downstream sees a partial. `CLAUDE.md`'s Stub Prevention section names
/// exactly this shape ("an always-`Opaque(Miss)` resolve is a nop").
///
/// Owning layer: the `SliceExpr::FrameShadowed` arm of
/// `project_semantic_dispatch::flow_return`'s evaluator. `x.q` lowers
/// through `flow_slice_content::lower_leaf` to a leaf answer naming
/// `typeof x`, and because the frame BINDS `x` the leaf is wrapped as
/// `FrameShadowed { shadowed: [Value("x")] }`. The arm's guard
/// (`owner_scope_answers_name`) is the fail-closed test for the case
/// where the OWNER scope has a same-named twin; here it does not, so the
/// guard passes and the inner leaf evaluates unchanged — resolving
/// `typeof x` in owner scope, where nothing answers, to `Opaque(Miss)`.
/// The arm's own comment calls that "its own typed miss carrier is the
/// honest answer"; the defect is not the value but the ADMISSION — a
/// complete, `degradation: None`, warm-admitted result, so
/// `execute_function_return_source` never folds the cache-read rails and
/// an enclosing composition warms with an opaque interior.
#[test]
#[ignore = "a member read off an annotated parameter evaluates to Opaque(Miss) and is admitted warm: the flow evaluator has no member projection over a frame-bound leaf root"]
fn member_read_off_an_annotated_parameter_resolves_to_the_member_type() {
    let host = ts_host();
    assert_clean_warm(&host, TL, "tlPlainMember", string());
}

/// CANARY — a member read off a CONSTRAINED clause parameter resolves
/// through the constraint.
///
/// Oracle (bidirectional signature identity): tsc resolves
/// `<T extends HasQ>(x: T) => x.q` as returning `string`, NOT `T["q"]`.
/// `const ok: <U extends HasQ>(x: U) => string = tlConstrainedMember;`
/// compiles;
/// `const bad: <U extends HasQ>(x: U) => number = tlConstrainedMember;`
/// fails with `TS2322: Type '<T extends HasQ>(x: T) => string' is not
/// assignable to type '<U extends HasQ>(x: U) => number'.`
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: tlConstrainedMember
///   left: Value { ty: Unknown(UnknownValue { raw: "semanticMiss", provenance: CompatibilityProjection }), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(String), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the same member arm as the unconstrained row — the
/// constraint is never reached, so this row does not yet discriminate
/// constraint resolution specifically.
#[test]
#[ignore = "blocked behind the member-read arm: `x.q` over a constrained binder evaluates to Opaque(Miss) before the constraint is consulted"]
fn constrained_clause_parameter_member_read_resolves_through_the_constraint() {
    let host = ts_host();
    assert_clean_warm(&host, TL, "tlConstrainedMember", string());
}

/// A COMPUTED member read off a constrained clause parameter (`x["q"]`)
/// fails CLOSED — a distinct outcome from the static member read's warm
/// opaque, and the architecturally correct one.
///
/// Oracle (for the record — the answer the fail-closed arm declines to
/// produce): tsc resolves `<T extends HasQ>(x: T) => x["q"]` as `string`
/// (`const bad: <U extends HasQ>(x: U) => number = tlConstrainedIndexed;`
/// fails with `TS2322: Type '<T extends HasQ>(x: T) => string' is not
/// assignable to type '<U extends HasQ>(x: U) => number'.`).
#[test]
fn computed_member_read_over_a_constrained_binder_fails_closed() {
    let host = ts_host();
    assert_fails_closed(&host, TL, "tlConstrainedIndexed");
}

// ──────────────────────────────────────────────────────────────────────
// PRIORITY 7 — span / anchor geometry
//
// The reviews reasoned about OXC `Function.span` for each of these
// positions but never enumerated them empirically. A recorded span that
// preceded its anchor would clamp under `saturating_sub` and select the
// WRONG body; these rows assert the observable consequence — every
// geometry either serves its own body correctly or fails closed, and none
// of them serves a neighbour's.
// ──────────────────────────────────────────────────────────────────────

/// Class-member geometry. Ordinals in declaration order: `method` (0),
/// `get accessor` (1), `set accessor` (2), `field = () => …` (3),
/// `static staticMethod` (4), `constructor` (5).
///
/// Oracle: `ReturnType<typeof GeoClass.prototype.method>` is `number`;
/// `typeof GeoClass.prototype.accessor` is `string`;
/// `ReturnType<typeof GeoClass.prototype.field>` is `number`. A setter
/// and a constructor have no return to project.
#[test]
fn class_member_geometry_serves_each_body_or_fails_closed() {
    let host = ts_host();
    assert_eq!(
        eval_part(&host, GEO, "GeoClass", member_part(0), 0),
        Outcome::Value {
            ty: number(),
            degradation: None,
            candidates: 1,
        },
        "instance method"
    );
    assert_eq!(
        eval_part(&host, GEO, "GeoClass", member_part(1), 0),
        Outcome::Value {
            ty: string(),
            degradation: None,
            candidates: 1,
        },
        "getter"
    );
    assert_eq!(
        eval_part(&host, GEO, "GeoClass", member_part(2), 0),
        Outcome::Miss,
        "a setter has no return to serve"
    );
    assert_eq!(
        eval_part(&host, GEO, "GeoClass", member_part(3), 0),
        Outcome::Value {
            ty: number(),
            degradation: None,
            candidates: 1,
        },
        "class-field arrow initialiser"
    );
    assert_eq!(
        eval_part(&host, GEO, "GeoClass", member_part(5), 0),
        Outcome::Miss,
        "a constructor has no return to serve"
    );
}

/// A static method's geometry serves its OWN body — it does not clamp
/// back onto the preceding field initialiser's.
///
/// Oracle: `ReturnType<typeof GeoClass.staticMethod>` is `string` (a
/// fresh `"sm"` literal, widened). The preceding member (`field`) returns
/// `number`, so a clamped anchor would surface as `number` here.
#[test]
fn static_method_geometry_serves_its_own_body_not_a_preceding_members() {
    let host = ts_host();
    assert_eq!(
        eval_part(&host, GEO, "GeoClass", member_part(4), 0),
        Outcome::Value {
            ty: string(),
            degradation: None,
            candidates: 1,
        }
    );
}

/// Object-literal geometry: `objMethod` (0) serves, `objArrow` (1) fails
/// closed, `objGet` (2) serves. The arrow-valued PROPERTY is a real
/// coverage gap, but it is the fail-CLOSED kind — it never publishes a
/// neighbour's body.
///
/// Oracle: `ReturnType<typeof geoObj.objMethod>` is `string`;
/// `ReturnType<typeof geoObj.objArrow>` is `string`;
/// `typeof geoObj.objGet` is `boolean`.
#[test]
fn object_literal_geometry_serves_methods_and_getters_and_fails_closed_on_arrow_properties() {
    let host = ts_host();
    assert_eq!(
        eval_part(&host, GEO, "geoObj", member_part(0), 0),
        Outcome::Value {
            ty: string(),
            degradation: None,
            candidates: 1,
        },
        "object-literal method"
    );
    assert_eq!(
        eval_part(&host, GEO, "geoObj", member_part(1), 0),
        Outcome::Miss,
        "an arrow-valued object property is not served — and does not serve a neighbour's body"
    );
    assert_eq!(
        eval_part(&host, GEO, "geoObj", member_part(2), 0),
        Outcome::Value {
            ty: boolean(),
            degradation: None,
            candidates: 1,
        },
        "object-literal getter"
    );
}

/// A DECORATED class member serves its own body — the decorator's own
/// span does not displace the member's anchor.
///
/// Oracle: `ReturnType<typeof GeoDecorated.prototype.decorated>` is
/// `string`.
#[test]
fn decorated_class_member_geometry_serves_its_own_body() {
    let host = ts_host();
    assert_eq!(
        eval_part(&host, GEO, "GeoDecorated", member_part(0), 0),
        Outcome::Value {
            ty: string(),
            degradation: None,
            candidates: 1,
        }
    );
}

/// An IIFE inside a function body resolves to the immediately-invoked
/// function's own return.
///
/// Oracle: `ReturnType<typeof geoIifeInside>` is `string`.
#[test]
fn iife_inside_a_body_resolves_to_the_invoked_functions_return() {
    let host = ts_host();
    assert_clean_warm(&host, GEO, "geoIifeInside", string());
}

/// A RETURNED arrow widens its own fresh literal at its own return
/// position — the widening rule applies inside a nested function value,
/// not only at the root frame.
///
/// Oracle: `ReturnType<typeof geoReturnedArrow>` is `() => number`.
#[test]
fn a_returned_arrows_fresh_literal_widens_at_its_own_return_position() {
    let host = ts_host();
    let ty = value_of(&host, GEO, "geoReturnedArrow");
    assert_eq!(
        projected_function_return(&ty),
        &number(),
        "the nested arrow's fresh literal widens"
    );
}

/// CANARY — a DEFAULT-PARAMETER arrow's fresh literal widens too.
///
/// Oracle: `ReturnType<typeof geoDefaultParamArrow>` is `() => number`
/// for `function geoDefaultParamArrow(cb = () => 7) { return cb; }`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: the default-parameter arrow's fresh literal must widen
///   left: Literal(Number(7.0))
///  right: Primitive(Number)
/// ```
///
/// Owning layer: the return-position widening rule. The IDENTICAL arrow
/// widens correctly when it is RETURNED
/// (`a_returned_arrows_fresh_literal_widens_at_its_own_return_position`
/// passes) but not when it is a parameter DEFAULT — so the widening is
/// applied on the returned-value path only, and the default-initialiser
/// path that feeds the parameter's inferred type is missed. Admitted
/// WARM.
#[test]
#[ignore = "a default-parameter arrow's fresh literal is not widened: the parameter's inferred type keeps `() => 7` where the language says `() => number`"]
fn a_default_parameter_arrows_fresh_literal_widens_too() {
    let host = ts_host();
    let ty = value_of(&host, GEO, "geoDefaultParamArrow");
    assert_eq!(
        projected_function_return(&ty),
        &number(),
        "the default-parameter arrow's fresh literal must widen"
    );
}

// ──────────────────────────────────────────────────────────────────────
// PRIORITY 8 — concurrency and cache
// ──────────────────────────────────────────────────────────────────────

/// Two threads racing ONE `FlowReturnKey` agree on the answer, and the
/// once-per-content-version `FunctionFlowGraphStore` builds the graph
/// exactly once — the singleflight on `get_or_build` collapses the race
/// onto one materialisation.
#[test]
fn concurrent_demands_on_one_flow_return_key_agree_and_build_the_graph_once() {
    let host = ts_host();
    let store = host.project_type_store();
    let before = store.flow_slice().graphs().build_count();

    let outcomes: Vec<Outcome> = std::thread::scope(|scope| {
        let handles: Vec<_> = (0..8)
            .map(|_| {
                let host = Arc::clone(&host);
                scope.spawn(move || eval(&host, TL, "tlObjReturn"))
            })
            .collect();
        handles
            .into_iter()
            .map(|handle| handle.join().expect("no racing demand may panic"))
            .collect()
    });

    let first = &outcomes[0];
    assert!(
        matches!(
            first,
            Outcome::Value {
                degradation: None,
                ..
            }
        ),
        "the raced key must produce a clean value, got {first:?}"
    );
    for outcome in &outcomes[1..] {
        assert_eq!(outcome, first, "every racing demand must agree");
    }
    assert_eq!(
        store.flow_slice().graphs().build_count() - before,
        1,
        "the flow graph must be built exactly once across the race"
    );
}

/// A second demand re-plans over the memoized bundle rather than
/// re-lowering the body: the graph build count does not move.
#[test]
fn a_second_demand_reuses_the_memoized_flow_graph() {
    let host = ts_host();
    let store = host.project_type_store();
    let first = eval(&host, TL, "tlObjReturn");
    let after_cold = store.flow_slice().graphs().build_count();
    let second = eval(&host, TL, "tlObjReturn");
    assert_eq!(first, second);
    assert_eq!(
        store.flow_slice().graphs().build_count(),
        after_cold,
        "a warm demand must not rebuild the flow graph"
    );
}

/// A CONTENT EDIT to the owning file supersedes the warm answer — the
/// flow rail's content-version rooting holds across an upsert, and the
/// stale bundle is not re-served.
#[test]
fn a_content_edit_supersedes_the_warm_flow_answer() {
    let host = host_with(&[(TL, TL_SRC)]);
    assert_eq!(
        projected_member(&value_of(&host, TL, "tlObjReturn"), "m"),
        &string(),
        "the cold answer reads the original body"
    );

    let edited = TL_SRC.replace(
        "return { m: \"mv\", n: { deep: true } };",
        "return { m: 5, n: { deep: true } };",
    );
    assert_ne!(edited, TL_SRC, "the edit must actually apply");
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(TL.to_string()),
        input_id: TL.to_string(),
        source: Arc::from(edited.as_str()),
        file_language: lang(TL),
        aliases: Vec::new(),
    });

    assert_eq!(
        projected_member(&value_of(&host, TL, "tlObjReturn"), "m"),
        &number(),
        "the edited body must supersede the warm answer"
    );
}

// ──────────────────────────────────────────────────────────────────────
// Late additions: the remaining reachable P4 / P5 / P8 axes
// ──────────────────────────────────────────────────────────────────────

const EXTRA: &str = "/ws/cov/extra.ts";
const EXTRA_SRC: &str = r#"
export class SuperCtorBase {
  constructor(public v: number) {}
}
export class SuperCtorDerived extends SuperCtorBase {
  constructor() {
    super(1);
  }
  read() {
    return this.v;
  }
}
export function localGeneric<T>(x: T) {
  return { g: x };
}
export function localGenericInfer() {
  return localGeneric("s");
}
"#;

/// A DERIVED constructor containing a `super(...)` call is not served:
/// the constructor position has no return to project, so the demand is a
/// typed no-value outcome. `super()` never reaches — and never needs — a
/// call carrier here, and the sibling instance method (ordinal 1) is
/// served independently, so the constructor's miss is a position rule,
/// not a whole-class failure.
///
/// Oracle: a constructor has no return type in TypeScript;
/// `ReturnType<typeof SuperCtorDerived>` is a type error, not a type.
#[test]
fn a_derived_constructor_with_a_super_call_is_not_served() {
    let host = host_with(&[(EXTRA, EXTRA_SRC)]);
    assert_eq!(
        eval_part(&host, EXTRA, "SuperCtorDerived", member_part(0), 0),
        Outcome::Miss,
        "a constructor has no return to serve"
    );
    // The sibling instance method IS reached (its own value is the
    // `this`-member canary below, not this row's subject).
    assert!(
        matches!(
            eval_part(&host, EXTRA, "SuperCtorDerived", member_part(1), 0),
            Outcome::Value { .. }
        ),
        "the sibling instance method is still served"
    );
}

/// CANARY — a `this.<field>` read inside an instance method resolves to
/// the field's declared type.
///
/// Oracle: `ReturnType<typeof SuperCtorDerived.prototype.read>` is
/// `number` — the parameter-property `public v: number` inherited from
/// the base constructor.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed
///   left: Value { ty: Primitive(Any), degradation: None, candidates: 1 }
///  right: Value { ty: Primitive(Number), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the flow evaluator's member arm again, this time with a
/// `this` root rather than a parameter root. Note the answer differs from
/// the parameter-root family: `this.v` lands on a WARM `any` rather than
/// the warm `Opaque(Miss)` a parameter root produces, so the two roots
/// take different paths to the same missing capability.
#[test]
#[ignore = "a `this.<field>` read has no member arm: it evaluates to `any` and is admitted warm"]
fn this_field_read_inside_an_instance_method_resolves_to_the_field_type() {
    let host = host_with(&[(EXTRA, EXTRA_SRC)]);
    assert_eq!(
        eval_part(&host, EXTRA, "SuperCtorDerived", member_part(1), 0),
        Outcome::Value {
            ty: number(),
            degradation: None,
            candidates: 1,
        }
    );
}

/// CANARY — a SAME-FILE generic callee infers its type argument from the
/// call site. This is the isolating twin of
/// `imported_generic_callee_infers_its_type_argument_from_the_call_site`:
/// both fail identically, which proves the collapse is NOT a cross-file
/// hop defect but the shared call carrier's missing argument-driven
/// inference.
///
/// Oracle: `ReturnType<typeof localGenericInfer>` is `{ g: string; }`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed: the same-file callee's `T` must be inferred as `string`
///   left: Primitive(Unknown)
///  right: Primitive(String)
/// ```
///
/// Owning layer: the direct-call carrier's instantiation — every free
/// clause parameter instantiates at `unknown`, warm, with no argument
/// inference step.
#[test]
#[ignore = "a same-file generic callee's IMPLICIT type argument is not inferred from the call argument: the instantiation collapses to `unknown` and is admitted warm"]
fn same_file_generic_callee_infers_its_type_argument_from_the_call_site() {
    let host = host_with(&[(EXTRA, EXTRA_SRC)]);
    let ty = value_of(&host, EXTRA, "localGenericInfer");
    assert_eq!(
        projected_member(&ty, "g"),
        &string(),
        "the same-file callee's `T` must be inferred as `string`"
    );
}

/// Removing the owning file EVICTS the flow answer: a demand after
/// `remove` is a typed no-value outcome, never the stale warm value.
#[test]
fn removing_the_owning_file_evicts_the_flow_answer() {
    let host = host_with(&[(TL, TL_SRC)]);
    assert!(
        matches!(
            eval(&host, TL, "tlObjReturn"),
            Outcome::Value {
                degradation: None,
                ..
            }
        ),
        "the cold demand must warm an answer first"
    );
    assert!(
        host.remove(TL).is_some(),
        "the file must actually be removed"
    );
    assert_eq!(
        eval(&host, TL, "tlObjReturn"),
        Outcome::Miss,
        "a removed file serves no flow answer"
    );
}
