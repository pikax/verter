//! @ai-generated - Coverage-expansion probes for the demand-sliced
//! `FlowReturn` substrate: the axes six adversarial reviews each recorded
//! in their closing "axes I did not test" section.
//!
//! Every expected value here is anchored against
//! `tsc 7.0.2 --noEmit --strict --ignoreConfig` through
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
//!
//! The recorded verbatim failure is a MEASUREMENT, so it goes stale when
//! the substrate below the canary changes even though the canary's own
//! claim does not. Whenever a change alters what these rows observe, the
//! whole parked set is re-run un-ignored and every record that moved is
//! re-captured — the header claim is only true if the records are.

use std::sync::Arc;

use super::*;
use crate::semantic_query::{
    FlowGap, FlowReturnDegradation, FlowReturnKey, SemanticQueryKey, SemanticQueryOutput,
    SemanticQueryValue,
};
use crate::types::{HostConfig, UpsertRequest};
use crate::VerterHost;
use verter_type_expr::facts::FunctionPartIdentity;
use verter_type_expr::{
    CompilerIntrinsicTypeOp, LiteralValue, PrimitiveName, TopLevelOwnerId, TypeExpr, UnknownValue,
};

// ──────────────────────────────────────────────────────────────────────
// Fixtures
// ──────────────────────────────────────────────────────────────────────

const LEAF: &str = "/ws/cov/leaf.ts";
const LEAF_SRC: &str = r#"
// The lib generator surface this standalone host has no `lib*.d.ts` for:
// the wrap resolves `Generator` through the ONE shared bare-ref resolver,
// so a file-scope declaration is the environment's stand-in.
interface Generator<T, TReturn, TNext> {}
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

export class LeafSuperGenericBase<T> {
  m(): T {
    return null as unknown as T;
  }
}
export class LeafSuperGenericDerived extends LeafSuperGenericBase<string> {
  m(): string {
    return super.m();
  }
}

export class LeafSuperStaticBase {
  static s(): string {
    return "s";
  }
}
export class LeafSuperStaticDerived extends LeafSuperStaticBase {
  static s(): string {
    return super.s();
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

// The SYNC negative control for the async-generator await: a plain
// generator does NOT await what it yields — tsgo:
// `Generator<Promise<number>, string, unknown>`.
export declare function leafAsyncSrc(): Promise<number>;
export function* leafGeneratorYieldPromise() {
  yield leafAsyncSrc();
  return "done";
}

// A SYNC generator publishes its generic yield verbatim — no awaiting, and
// no deferred `Awaited` spelling either (tsgo: `Generator<T, string, unknown>`).
export function* leafGeneratorYieldParam<T>(v: T) {
  yield v;
  return "done";
}

// A YIELDED settled call deposits its fresh literal return exactly as a
// RETURNED one does, so the lone-fresh-arm widening reads it the same way
// (tsgo: `Generator<number, string, unknown>`); two such yields are two
// distinct constituents and keep `1 | 2`.
export function* leafGeneratorYieldCall() {
  yield idf(1);
  return "done";
}
export function* leafGeneratorYieldTwoCalls() {
  yield idf(1);
  yield idf(2);
  return "done";
}

// A PARENTHESIZED statement-position yield: the outer node is a
// `ParenthesizedExpression`, not a bare `YieldExpression` — the yield
// classifier must unwrap it, or this contributor is silently dropped and
// the generator's yield parameter collapses to `never`.
export function* leafGeneratorParenthesizedYield() {
  (yield 1);
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

// A generator whose YIELD (not its return) calls a mutually recursive
// sibling: `sccGenYield`/`sccGenPartner` form one SCC component through
// the yield-position call, not through a return-position one.
export function* sccGenYield(c: boolean) {
  if (c) {
    yield sccGenPartner(c);
  }
  return 1;
}

export function sccGenPartner(c: boolean) {
  if (c) return "s";
  return sccGenYield(false);
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

const NEG: &str = "/ws/cov/nolib.ts";
const NEG_SRC: &str = r#"
// NO lib generator surface: this file deliberately declares neither
// `Generator` nor `AsyncGenerator`, so the GENERATOR paths' wrap
// lib-head resolution fails and the wrap must keep the typed gap.
// `negAsync` below is unaffected — the async wrap's `Promise` carrier is
// a registry-decided interning with no lib-head lookup of its own.
export function* negGen() {
  return 1;
}
export async function negAsync() {
  return 1;
}

// A SELF-REFERENTIAL promise alias: `negAsyncSelfRef`'s payload recurses
// back into the SAME alias, which the checker reports as TS1062 and
// recovers from with `Promise<any>`.
type NegSelfProm = Promise<NegSelfProm>;
export declare function negAsyncSrc(): NegSelfProm;
export async function negAsyncSelfRef() {
  return negAsyncSrc();
}

// A THENABLE Promise payload: the awaited relations defer a
// `then`-bearing object surface (structural thenables are out of scope —
// see the object arm of `settled_non_thenable`), so the async wrap's
// Promise-embedding branch must reach the family's honest refusal, and
// the wrap must publish the TYPED GAP rather than a fabricated
// `Promise<Thenable>` or a bare unwrapped `Thenable`.
interface NegThenable {
  then(cb: (v: number) => void): void;
}
export declare function negThenableSrc(): Promise<NegThenable>;
export async function negAsyncThenable() {
  return negThenableSrc();
}
"#;

/// A `.tsx` file with NO configured `JSX` namespace anywhere in scope —
/// the negative control for the JSX leaf: even with nothing to resolve
/// `JSX.Element` against, the leaf still publishes the honest unresolved
/// `Ref { name: "JSX.Element" }` carrier, never a fabricated `any`.
const JSX_UNCONFIGURED: &str = "/ws/cov/jsx_unconfigured.tsx";
const JSX_UNCONFIGURED_SRC: &str = r#"
export function jsxElemNoNamespace() {
  return <div />;
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

export function callSeqRest() {
  return (0, restFn(1, 2, 3));
}
export function callSeqNew() {
  return (0, new CtorC(1));
}
export function callSeqOptional() {
  return (0, maybeFn?.());
}
export function callSeqTagged() {
  return (0, tag`a${1}b`);
}
export async function callSeqAwait() {
  return (0, await asyncSrc());
}

export declare function thisFn(this: { z: number }, a: number): "this";
export function callThisParam() {
  return thisFn.call({ z: 1 }, 1);
}

export declare function asyncSrc(): Promise<number>;
// The lib async-generator surface (see LEAF_SRC's `Generator` note).
interface AsyncGenerator<T, TReturn, TNext> {}
export async function callAwait() {
  return await asyncSrc();
}
export async function callAsyncPlain() {
  return 1;
}
export async function* callAsyncGen() {
  yield 1;
}
// A bare PASSTHROUGH of an already-`Promise`-typed value: the async wrap
// must Awaited-collapse the embedded carrier before re-wrapping, so the
// published type is `Promise<number>` — never `Promise<Promise<number>>`.
export async function callAsyncPassthrough() {
  return asyncSrc();
}

// `return await 1`: the lib `Awaited` surface passes a SETTLED literal
// through verbatim, so the operand's FRESHNESS is the await's own and the
// lone fresh arm widens exactly as a bare `return 1` does — tsgo:
// `Promise<number>`, never `Promise<1>`.
export async function callAwaitFreshLiteral() {
  return await 1;
}

// The FALSIFICATION twin: TWO awaited fresh arms are two DISTINCT literal
// constituents, so the lone-fresh-arm widening must NOT fire — tsgo:
// `Promise<1 | 2>`.
export async function callAwaitFreshTernary(c: boolean) {
  return c ? await 1 : await 2;
}

// An async generator AWAITS what it yields: the yield parameter rides the
// same lib `Awaited` surface the async wrap's body join does — tsgo:
// `AsyncGenerator<number, void, unknown>`, never
// `AsyncGenerator<Promise<number>, …>`.
export async function* callAsyncGenYieldPromise() {
  yield asyncSrc();
}

// …and what it RETURNS: the same collapse applies to the body join —
// tsgo: `AsyncGenerator<number, number, unknown>`.
export async function* callAsyncGenReturnPromise() {
  yield 1;
  return asyncSrc();
}


// A GENERIC body join: the bare type parameter `T` itself. tsgo publishes
// `Promise<T>` for this shape — it does NOT spell `Awaited<T>`, and
// instantiating `T` with `Promise<string>` genuinely nests to
// `Promise<Promise<string>>` — so the wrap must neither collapse the
// parameter nor wrap it in a deferred `Awaited`.
export async function asyncGenericIdentity<T>(value: T) {
  return value;
}

// The same generic body reached through a `Promise` parameter, a union of
// the parameter and its promise, and an `await` — tsgo publishes
// `Promise<T>` for all three, collapsing exactly one promise level.
export async function asyncPromiseParam<T>(v: Promise<T>) {
  return v;
}
export async function asyncUnionParam<T>(v: T | Promise<T>) {
  return v;
}
export async function asyncAwaitParam<T>(v: T) {
  return await v;
}

// An async GENERATOR does not share the async function's generic rule:
// tsgo spells the DEFERRED `Awaited<T>` in both iteration parameters.
export async function* asyncGenYieldParam<T>(v: T) {
  yield v;
}
export async function* asyncGenReturnParam<T>(v: T) {
  return v;
}

// The awaited-relation ORACLE MATRIX (see `awaited_relation_oracle_matrix`).
// `asyncGenericIdentity` and `asyncGenYieldParam` above are its r1 / y1 rows.
interface MatrixThenable<V> {
  then(onfulfilled: (value: V) => void): void;
}
export async function matrixR2<T extends string>(v: T) {
  return v;
}
export async function matrixR3<T extends Promise<string>>(v: T) {
  return v;
}
export async function matrixR4<T extends MatrixThenable<number>>(v: T) {
  return v;
}
export async function matrixX1<T>(v: T) {
  const a = await v;
  return { a };
}
export async function matrixX2<T extends string>(v: T) {
  const a = await v;
  return { a };
}
export async function matrixX3<T extends Promise<string>>(v: T) {
  const a = await v;
  return { a };
}
export async function matrixX4<T extends MatrixThenable<number>>(v: T) {
  const a = await v;
  return { a };
}
export async function* matrixY2<T extends string>(v: T) {
  yield v;
}
export async function matrixG1<T>(v: Awaited<T>) {
  return v;
}
export async function matrixG6<T>(v: Array<Awaited<T>>) {
  return v;
}
export async function matrixG7<T>(v: Awaited<T>[]) {
  return v;
}
export function matrixG8<T>(v: Awaited<T>) {
  return v;
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

export declare const maybeObj: { b: string } | undefined;
export function callOptionalMemberRead() {
  return maybeObj?.b;
}

export declare const optProp: { b?: string };
export function callOptionalDeclaredMemberRead() {
  return optProp?.b;
}

export declare const optChainProp: { b?: { c: number } };
export function callOptionalDeclaredChainMemberRead() {
  return optChainProp?.b?.c;
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

export function tlFreeUnresolvedRead() {
  return noSuchGlobalValue;
}

export function tlMissingParamAnnotation(x: NoSuchTypeName) {
  return x;
}

export function tlMissCarrierInObjectMember(x: HasQ) {
  return { q: x.q };
}

export function tlMissCarrierInArray(x: HasQ) {
  return [x.q];
}

export function tlMissCarrierInNestedFunction(x: HasQ) {
  return () => x.q;
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
        (NEG, NEG_SRC),
        (JSX_UNCONFIGURED, JSX_UNCONFIGURED_SRC),
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
        result_contract: super::flow_solve::flow_return_result_contract_id(),
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
            let Some(ty) = host.project_node_to_type_expr_for_test(result.return_type()) else {
                return Outcome::Other("the value did not project".to_string());
            };
            let candidates = dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
            Outcome::Value {
                ty,
                degradation: result.degradation(),
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
    with_dispatch(host, |dispatch| {
        let key = key_of(dispatch, canonical, name);
        super::flow_return_lexical_tests::assert_flow_fails_closed(
            dispatch,
            name,
            dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
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

/// The published spelling of a naked type parameter.
fn type_param(name: &str) -> TypeExpr {
    TypeExpr::TypeParameter(verter_type_expr::TypeParam {
        name: name.to_string(),
        constraint: None,
        default: None,
        is_const: false,
    })
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
        assert_eq!(result.degradation(), None, "{name} must evaluate clean");
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            1,
            "{name} must warm-admit exactly one candidate"
        );
        pick(dispatch, result.return_type())
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
///   left: Value { ty: Unknown(UnknownValue { raw: "semanticMiss", provenance: CompatibilityProjection }), degradation: Some(UnresolvedValue), candidates: 0 }
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
/// Note `candidates: 0`: the opaque miss is a REFUSED value, not a warm
/// one — `a_value_reaching_a_miss_carrier_is_never_admitted_warm` owns
/// that half, and this canary owns the missing capability.
#[test]
#[ignore = "a member read off an annotated binding evaluates to Opaque(Miss) (ReturnOnly): the flow evaluator has no member projection over a frame-bound leaf root"]
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
///   left: Value { ty: Unknown(UnknownValue { raw: "semanticMiss", provenance: CompatibilityProjection }), degradation: Some(UnresolvedValue), candidates: 0 }
///  right: Value { ty: Primitive(String), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the same evaluator arm as the Vue twin. The `$props()`
/// destructuring binds `msg` as a carrier-scope destructured `let`, which
/// the evaluator answers with an opaque miss rather than the
/// annotation's member.
#[test]
#[ignore = "a `$props()`-destructured binding read evaluates to Opaque(Miss) (ReturnOnly): the flow evaluator has no destructuring-element arm for a carrier-scope binding"]
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
///   left: Value { ty: Unknown(UnknownValue { raw: "semanticMiss", provenance: CompatibilityProjection }), degradation: Some(UnresolvedValue), candidates: 0 }
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
#[ignore = "blocked behind the member-read arm: `widget.b` evaluates to Opaque(Miss) (ReturnOnly) before the augmentation stitch is ever consulted"]
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

/// A `ClassExpression` in return position is the class's constructor
/// type.
///
/// Oracle: `ReturnType<typeof leafClassExpr>` is
/// `typeof (Anonymous class)`, declaration-emitted as `{ new (): {}; }`.
///
/// The value is complete and undegraded, and still `ReturnOnly`: the
/// class's constructor and field initializers are callables no indexed
/// function position serves, so the capture family keeps its typed gap
/// over them and the result never warms.
#[test]
fn class_expression_return_is_its_constructor_type() {
    let host = ts_host();
    let instance = TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
        properties: Vec::new(),
    }));
    assert_eq!(
        eval(&host, LEAF, "leafClassExpr"),
        Outcome::Value {
            ty: TypeExpr::Object(Arc::new(verter_type_expr::ObjectExpr {
                properties: vec![verter_type_expr::ObjectMember::ConstructSignature(
                    verter_type_expr::FunctionExpr::synthetic(
                        Vec::new(),
                        Some(Arc::new(instance)),
                        Vec::new(),
                    ),
                )],
            })),
            degradation: None,
            candidates: 0,
        },
        "leafClassExpr"
    );
}

/// CANARY (fail-closed leg) — an `ImportExpression` (dynamic
/// `import(...)`) in return position is a `Promise` of the module's
/// namespace type.
///
/// Oracle: `ReturnType<typeof leafImportExpr>` is
/// `Promise<typeof import("…/dep")>`.
///
/// The fabricated `any` this row was parked against is DELETED: the
/// shared shallow pass's unmodelled-form fallback reports completeness
/// `Unmodeled`, so the form lowers to the typed
/// `FlowGap::UnmodeledExpression` and never warms. The typed
/// publication — `Promise<typeof import("m")>` through the
/// module-namespace surface — has no carrier spelling in this IR yet,
/// so the form keeps the charter's fail-closed branch.
#[test]
fn dynamic_import_expression_return_is_not_any() {
    let host = ts_host();
    assert_eq!(
        eval(&host, LEAF, "leafImportExpr"),
        Outcome::Value {
            ty: TypeExpr::Unknown(UnknownValue::compatibility_projection("unmodeledPosition")),
            degradation: Some(FlowReturnDegradation::FlowGap(FlowGap::UnmodeledExpression)),
            candidates: 0,
        },
        "leafImportExpr"
    );
}

/// CANARY (fail-closed leg) — a `MetaProperty` (`new.target`) in return
/// position is not `any`.
///
/// Oracle: `ReturnType<typeof leafNewTarget>` prints as
/// `() => typeof leafNewTarget` — `new.target` inside `f` is typed as
/// `typeof f`, so the wrapper's answer is the function type itself.
///
/// The fabricated `any` this row was parked against is DELETED (see the
/// class-expression row): the form lowers to the typed
/// `FlowGap::UnmodeledExpression` and never warms. The typed publication
/// — the containing function's own type for `new.target`, the lib
/// `ImportMeta` surface for `import.meta` — has no carrier spelling in
/// this IR yet, so the form keeps the charter's fail-closed branch.
#[test]
fn meta_property_new_target_return_is_not_any() {
    let host = ts_host();
    assert_eq!(
        eval(&host, LEAF, "leafNewTarget"),
        Outcome::Value {
            ty: TypeExpr::Unknown(UnknownValue::compatibility_projection("unmodeledPosition")),
            degradation: Some(FlowReturnDegradation::FlowGap(FlowGap::UnmodeledExpression)),
            candidates: 0,
        },
        "leafNewTarget"
    );
}

/// CANARY — a typed optional member read (`maybeObj?.b`) publishes the
/// member's type over the nullish-stripped base, `| undefined` — the
/// checker's `string | undefined` for a nullable base, clean and warm.
///
/// Oracle: `ReturnType<typeof callOptionalMemberRead>` is
/// `string | undefined`.
///
/// Verbatim failure (before the `SliceExpr::OptionalMember` carrier):
///
/// ```text
/// assertion `left == right` failed
///   left: Value { ty: Unknown(UnknownValue { raw: "unmodeledPosition", provenance: CompatibilityProjection }), degradation: Some(FlowGap(UnmodeledExpression)), candidates: 0 }
///  right: Value { ty: Union([Primitive(String), Primitive(Undefined)]), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the content half's chain arm — a member-valued chain
/// used to ride the still-`any` `OptionalAnyChain` rail (whose non-`any`
/// root degraded) or the whole-form leaf gap. The typed carrier strips
/// each optional link's nullish arms, projects the link through the one
/// shared path walk, and unions `undefined` exactly when a strip removed
/// arms; a NON-nullable base keeps the plain member type (no undefined)
/// and an `any` root keeps `any`.
#[test]
fn optional_member_read_return_is_the_stripped_member_or_undefined() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        CALLS,
        "callOptionalMemberRead",
        TypeExpr::union(vec![
            string(),
            TypeExpr::Primitive(PrimitiveName::Undefined),
        ]),
    );
    // A DECLARED-optional member (`b?: string`) folds its own absent-key
    // `undefined` into the read even over a NON-nullable base — the same
    // fold `project_segments_navigate` applies to a plain member path.
    // Before the per-link fold, this hop bypassed that authority and
    // published the bare member type clean and warm.
    assert_clean_warm(
        &host,
        CALLS,
        "callOptionalDeclaredMemberRead",
        TypeExpr::union(vec![
            string(),
            TypeExpr::Primitive(PrimitiveName::Undefined),
        ]),
    );
    // Chained declared-optional hops over a non-nullable root: each link
    // folds its own optionality independently, so the terminal read still
    // carries `| undefined` even though neither hop's base is nullish.
    assert_clean_warm(
        &host,
        CALLS,
        "callOptionalDeclaredChainMemberRead",
        TypeExpr::union(vec![
            number(),
            TypeExpr::Primitive(PrimitiveName::Undefined),
        ]),
    );
}

/// A member read through a SYNTHESIZED optional member folds the absent-key
/// `undefined` exactly like the authored one.
///
/// Oracle (the pinned TypeScript 7.0.2, `--strict`): with
/// `type Authored = { a?: 1 }` and `type Synthetic = Partial<{ a: 1 }>`,
/// `v.a` is `1 | undefined` for BOTH.
///
/// Before, the synthesized read published `1`: the optionality proof that
/// gates the fold recognised only an `Object` surface, and `Partial<…>`
/// reaches the read as a deferred mapped shell whose optional member is
/// visible only once it is reduced. One shape then answered differently by
/// origin — the section 5.9 determinism row DET-07 found it.
#[test]
fn synthesized_optional_member_read_folds_undefined_like_the_authored_one() {
    const PATH: &str = "/ws/cov/synthetic_optional.ts";
    const SRC: &str = "type Authored = { a?: 1 };\n\
                       type Synthetic = Partial<{ a: 1 }>;\n\
                       export function authored(v: Authored) { return v.a; }\n\
                       export function synthetic(v: Synthetic) { return v.a; }\n";
    let host = host_with(&[(PATH, SRC)]);
    let authored = eval(&host, PATH, "authored");
    match &authored {
        Outcome::Value {
            ty: TypeExpr::Union(arms),
            degradation: None,
            candidates: 1,
        } => {
            let one = TypeExpr::Literal(LiteralValue::Number(1.0));
            let undefined = TypeExpr::Primitive(PrimitiveName::Undefined);
            assert!(
                arms.len() == 2 && arms.contains(&one) && arms.contains(&undefined),
                "the authored read must be `1 | undefined`: {authored:?}"
            );
        }
        other => panic!("the authored read must be a clean `1 | undefined` union: {other:?}"),
    }
    assert_eq!(
        eval(&host, PATH, "synthetic"),
        authored,
        "a SYNTHESIZED optional member must read exactly like the authored one"
    );
}

/// A generic call chain `c{n}<T>(x: T) { return c{n-1}(x); }` of `levels`
/// functions over `c0`, and a witness `w` calling the last one. `c0` is
/// imported from `dep` when `import_c0`, else defined locally with `tag`.
fn generic_chain(levels: usize, import_c0: bool, tag: &str) -> String {
    let mut source = if import_c0 {
        "import { c0 } from \"./dep\";\n".to_string()
    } else {
        generic_chain_head(tag)
    };
    for level in 1..levels {
        source.push_str(&format!(
            "export function c{level}<T>(x: T) {{ return c{}(x); }}\n",
            level - 1
        ));
    }
    source.push_str(&format!(
        "export function w(v: number | string) {{ return c{}(v); }}\n",
        levels - 1
    ));
    source
}

fn generic_chain_head(tag: &str) -> String {
    format!("export function c0<T>(x: T) {{ return {{ v: x, tag: \"{tag}\" as const }}; }}\n")
}

/// The answer for `w` over a `levels`-long chain, and the connected work
/// units its demand charged.
fn generic_chain_work(levels: usize) -> (Outcome, usize) {
    let path = format!("/ws/cov/chain_{levels}.ts");
    let host = host_with(&[(path.as_str(), generic_chain(levels, false, "o").as_str())]);
    with_dispatch(&host, |dispatch| {
        let key = key_of(dispatch, &path, "w");
        let outcome = eval_key_on(&host, dispatch, key);
        (outcome, dispatch.connected_demand.work_used_for_tests())
    })
}

/// §12 hard gate "shared body-obligation consumers reuse completed return
/// work": each chain level demands its callee both generically and under
/// the call's instantiation, and each of those re-demands both forms of
/// the level below. Before a completed inline member was reusable on its
/// own transaction, every repeat re-evaluated the body and the work
/// DOUBLED per level (20504 units at eleven levels, against 306 now); the
/// host's audited entry ran out of connected-work budget at eleven. Every
/// added level must now cost the same.
///
/// Runs on the production worker stack (`host_cpu_pool`'s 8 MiB): eleven
/// levels nest 22 connected queries, deeper than a default test thread
/// holds in an unoptimized build.
#[test]
fn a_generic_call_chain_reuses_each_completed_callee() {
    let worker = std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let (short, _) = generic_chain_work(2);
            assert!(
                matches!(
                    short,
                    Outcome::Value {
                        degradation: None,
                        ..
                    }
                ),
                "the two-level chain must answer cleanly: {short:?}"
            );
            let (nine, work_nine) = generic_chain_work(9);
            let (ten, work_ten) = generic_chain_work(10);
            let (eleven, work_eleven) = generic_chain_work(11);
            for (levels, outcome) in [(9, nine), (10, ten), (11, eleven)] {
                assert_eq!(
                    outcome, short,
                    "a {levels}-level chain answers exactly like a two-level one"
                );
            }
            assert_eq!(
                work_eleven - work_ten,
                work_ten - work_nine,
                "every added level must cost the same connected work \
                 ({work_nine} / {work_ten} / {work_eleven} at 9 / 10 / 11 levels)"
            );
        })
        .expect("spawn the chain worker");
    if let Err(panic) = worker.join() {
        std::panic::resume_unwind(panic);
    }
}

/// A reused member's reads are REPLAYED into the builds demanding it, so
/// an edit to a callee in another file still reaches every consumer: the
/// callee's body fact must root whatever was built from the reused value,
/// or the second read below would serve the stale tag.
#[test]
fn a_reused_callee_still_invalidates_its_consumers_on_edit() {
    const DEP: &str = "/ws/cov/chain_dep/dep.ts";
    const MAIN: &str = "/ws/cov/chain_dep/main.ts";
    let main = generic_chain(6, true, "");
    let host = host_with(&[(DEP, &generic_chain_head("before")), (MAIN, &main)]);
    let before = eval(&host, MAIN, "w");
    assert!(
        format!("{before:?}").contains("before"),
        "the chain carries the callee's tag: {before:?}"
    );
    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(DEP.to_string()),
        input_id: DEP.to_string(),
        source: Arc::from(generic_chain_head("after").as_str()),
        file_language: lang(DEP),
        aliases: Vec::new(),
    });
    let after = eval(&host, MAIN, "w");
    assert!(
        format!("{after:?}").contains("after") && !format!("{after:?}").contains("before"),
        "an edit to the reused callee must reach the chain's answer: {after:?}"
    );
}
/// CANARY (landed) — a `super.m()` call in a derived class method
/// resolves to the base member's declared return.
///
/// Oracle: `ReturnType<typeof LeafSuperDerived.prototype.m>` is `number`.
///
/// Verbatim failure (before the `SliceCall::OnHeritage` carrier):
///
/// ```text
/// assertion `left == right` failed
///   left: Value { ty: Unknown(UnknownValue { raw: "unmodeledPosition", provenance: CompatibilityProjection }), degradation: Some(UnmodeledPosition), candidates: 0 }
///  right: Value { ty: Primitive(Number), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the content half's call lowering — a `Super` callee root
/// now lowers onto the HERITAGE surface (`SliceCall::OnHeritage`): the
/// enclosing class's `extends` expression rides as a gated value type,
/// the evaluator projects `prototype.m` off it through the ONE shared
/// path walk (only the demanded member is materialised) and hands the
/// resolved callee to the one call sink. A heritage the shallow pass
/// cannot model (a call, a mixin) keeps the fail-closed rail below, as
/// does a computed `super[k]()` link.
#[test]
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

/// CANARY — `super.s()` reaches the base's STATIC side (not the
/// instance/prototype side) when the calling member is itself static.
///
/// Oracle: `ReturnType<typeof LeafSuperStaticDerived.s>` is `string`.
#[test]
fn super_static_method_call_return_resolves_to_the_base_static_member() {
    let host = ts_host();
    assert_eq!(
        eval_part(&host, LEAF, "LeafSuperStaticDerived", member_part(0), 0),
        Outcome::Value {
            ty: string(),
            degradation: None,
            candidates: 1,
        }
    );
}

/// CANARY (fail-closed leg) — `super.m()` over a GENERIC base
/// (`extends Base<string>`) does not publish the base's unbound type
/// parameter (or any other uninstantiated answer) clean and warm. The
/// heritage carrier only ever projects the base's PROTOTYPE side
/// UNINSTANTIATED, so publishing through it here would hand back a free
/// `T`, or whatever the unbound construct-signature instantiation
/// yields — a wrong answer that is still typed and still not `any`.
/// `lower_super_call_on_heritage` fails closed (no carrier) whenever the
/// enclosing class's heritage carries `super_type_arguments`, so this
/// falls to the shared fail-closed rail — the SAME `UnmodeledPosition`
/// marker an unrepresentable callee root takes elsewhere — instead of
/// fabricating an uninstantiated member type.
///
/// Oracle: `ReturnType<typeof LeafSuperGenericDerived.prototype.m>` is
/// `string` (the checker instantiates `Base<string>` before projecting
/// `m`) — an answer this form does not yet attempt to produce.
#[test]
fn super_method_call_over_generic_base_fails_closed_not_uninstantiated() {
    let host = ts_host();
    match eval_part(&host, LEAF, "LeafSuperGenericDerived", member_part(0), 0) {
        Outcome::Value {
            ty,
            degradation,
            candidates,
        } => {
            assert_ne!(ty, string(), "must not silently produce the checker's answer via an unmodeled/uninstantiated path");
            assert_eq!(
                degradation,
                Some(FlowReturnDegradation::UnmodeledPosition),
                "generic super call must fail closed with the typed gap"
            );
            assert_eq!(candidates, 0, "a fail-closed answer must not warm");
        }
        other => panic!("expected a degraded fail-closed value, got {other:?}"),
    }
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
/// Verbatim failure (before the wrap landed):
///
/// ```text
/// assertion `left != right` failed: a generator's return must not be the bare `return` type
///   left: Primitive(String)
///  right: Primitive(String)
/// ```
///
/// Owning layer (landed): the function-kind wrap — the skeleton carries
/// the authored `generator` flag, the join defers the wrap past the
/// equation fixed point, and the publication closure instantiates the
/// resolved lib `Generator` head over `(yield join, return join,
/// unknown)`. The fixture declares the `Generator` surface this
/// standalone host has no `lib*.d.ts` for; an environment that cannot
/// resolve the head keeps the typed gap (never warm) instead.
#[test]
fn generator_return_is_wrapped_in_generator() {
    let host = ts_host();
    assert_eq!(
        value_of(&host, LEAF, "leafGenerator"),
        TypeExpr::Ref {
            name: Arc::from("Generator"),
            type_arguments: Arc::from(
                vec![
                    number(),
                    string(),
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                ]
                .into_boxed_slice()
            ),
        },
        "a generator's return is Generator<yield, return, next>"
    );
}

/// A PARENTHESIZED statement-position `(yield 1);` must classify exactly
/// like the bare `yield 1;` form — the outer `ParenthesizedExpression`
/// node must not hide the yield contributor from the generator's yield
/// join, or the parameter silently collapses to `never`.
#[test]
fn parenthesized_statement_position_yield_still_contributes_to_the_yield_join() {
    let host = ts_host();
    assert_eq!(
        value_of(&host, LEAF, "leafGeneratorParenthesizedYield"),
        TypeExpr::Ref {
            name: Arc::from("Generator"),
            type_arguments: Arc::from(
                vec![
                    number(),
                    string(),
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                ]
                .into_boxed_slice()
            ),
        },
        "a parenthesized yield must contribute number to the yield join, not collapse it to never"
    );
}

/// CANARY (landed) — a JSX element / fragment in return position is the
/// configured `JSX.Element`.
///
/// Oracle: with `declare global { namespace JSX { interface Element … } }`
/// in scope, tsgo types all three of `ReturnType<typeof jsxElem>`,
/// `ReturnType<typeof jsxFrag>` and `ReturnType<typeof jsxAttrCall>` as
/// `Element`.
///
/// The fabricated `any` this row was parked against is DELETED:
/// `JSXElement` / `JSXFragment` now lower to the whole-form leaf
/// `JSX.Element` type reference, resolved through the ONE shared
/// lowering exactly as an authored `JSX.Element` annotation is (the
/// frame gate's `Namespace`-meaning probe covers a frame-local shadow
/// of `JSX`), and an unresolvable `JSX` namespace keeps the honest
/// authored-unresolved carrier — never `any`.
///
/// `jsxAttrCall` is deliberately NOT routed through the call-position
/// fail-closed rail, even though it does embed a call: a JSX element's
/// value is `JSX.Element` and does not depend on any attribute's value,
/// so the attribute's call takes the ordinary decided-above
/// certification instead (`record_decided_above_calls`), not a
/// fail-closed verdict for the element.
#[test]
fn jsx_element_fragment_and_attribute_call_returns_are_not_any() {
    let host = ts_host();
    // Positive pin: the whole-form JSX leaf publishes the checker's
    // answer — the `JSX.Element` reference — clean and warm-admitted.
    // This discriminates the claimed typed publication from the typed
    // fail-closed gap marker (`Unknown { raw: "unmodeledPosition" }`,
    // `FlowGap::UnmodeledExpression`), which also satisfies a bare
    // `!= any` assertion but is a wholly different outcome (degraded,
    // zero candidates).
    for name in ["jsxElem", "jsxFrag"] {
        assert_clean_warm(&host, JSX, name, type_ref("JSX.Element"));
    }
    // `jsxAttrCall`'s attribute value is itself a call the surrounding
    // narrowing pass cannot model precisely, so the whole return degrades
    // with the unrelated `FlowGap::GuardNarrowing` gap (a general
    // narrowing limitation, not specific to JSX or to this charter). The
    // JSX leaf's own answer still resolves to the same typed
    // `JSX.Element` reference rather than a fabricated `any`, so the full
    // outcome is pinned exactly — still discriminating the typed leaf
    // from both `any` and the `UnmodeledExpression` gap marker.
    assert_eq!(
        eval(&host, JSX, "jsxAttrCall"),
        Outcome::Value {
            ty: type_ref("JSX.Element"),
            degradation: Some(FlowReturnDegradation::FlowGap(FlowGap::GuardNarrowing)),
            candidates: 0,
        },
        "jsxAttrCall"
    );
    // Negative control: with no `JSX` namespace configured anywhere in
    // scope, the leaf still publishes the same honest unresolved
    // `Ref { name: "JSX.Element" }` carrier — never a fabricated `any`,
    // and never silently promoted to a different shape just because the
    // reference cannot resolve further.
    assert_clean_warm(
        &host,
        JSX_UNCONFIGURED,
        "jsxElemNoNamespace",
        type_ref("JSX.Element"),
    );
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

/// An AMBIENT OVERLOAD GROUP callee resolves to the FIRST APPLICABLE
/// signature. The declaration index keeps one entry per name while the
/// language picks the first MATCHING signature — the executor picks it
/// too, never whichever entry the index happens to hold.
///
/// Oracle: `ReturnType<typeof callAmbientOverload>` is `"S"`.
#[test]
fn ambient_overload_group_callee_resolves_the_first_applicable_signature() {
    let host = ts_host();
    assert_clean_warm(&host, CALLS, "callAmbientOverload", string_lit("S"));
}

/// A GENERIC overload group callee resolves by arity and argument
/// inference — the pair `<T>(a: T)` / `<T, U>(a: T, b: U)` IS resolved by
/// the supplied arguments, and the picked signature's clause instantiates
/// from them. The member values are the checker's widened answer: the
/// fresh literal arguments close fresh inference bindings, and a fresh
/// literal fixed inside the return's structure widens at the call
/// boundary.
///
/// Oracle: `ReturnType<typeof tlCallOvlGen>` is `{ g: string; h: number; }`.
#[test]
fn generic_overload_group_callee_resolves_by_arity_and_inference() {
    let host = ts_host();
    let Outcome::Value {
        ty,
        degradation,
        candidates,
    } = eval(&host, TL, "tlCallOvlGen")
    else {
        panic!("tlCallOvlGen must produce a value");
    };
    assert_eq!(degradation, None, "tlCallOvlGen must evaluate clean");
    assert_eq!(candidates, 1, "tlCallOvlGen must warm-admit");
    assert_eq!(
        projected_member(&ty, "g"),
        &TypeExpr::Primitive(PrimitiveName::String),
        "the picked overload's `g` infers from the first argument and the \
         fresh literal widens at the call boundary"
    );
    assert_eq!(
        projected_member(&ty, "h"),
        &TypeExpr::Primitive(PrimitiveName::Number),
        "the second overload's `h` — arity picked it — infers from the \
         second argument and widens the same way"
    );
}

/// CANARY (landed) — a `new` expression's return is the constructed
/// instance type.
///
/// Oracle: `ReturnType<typeof callNew>` is `CtorC`.
///
/// The construction rides the one call carrier to the class's construct
/// signatures (`SliceCall::Construct`), so the value is the instance,
/// clean and warm.
#[test]
fn construct_expression_return_is_the_instance_type() {
    let host = ts_host();
    assert_clean_warm(&host, CALLS, "callNew", type_ref("CtorC"));
}

/// CANARY (landed) — a CONSTRUCT-SIGNATURE call (`new ctorSig(1)` where
/// `ctorSig` is a value carrying a `new (…)` signature) returns the
/// signature's instance type.
///
/// Oracle: `ReturnType<typeof callCtorSigNew>` is `{ q: string; }`.
#[test]
fn construct_signature_call_return_is_the_signature_instance_type() {
    let host = ts_host();
    let value = value_of(&host, CALLS, "callCtorSigNew");
    assert_eq!(projected_member(&value, "q"), &string());
}

/// CANARY — an OPTIONAL-CHAINED call (`maybeFn?.()`) returns the callee's
/// return unioned with `undefined`.
///
/// Oracle: `ReturnType<typeof callOptional>` is `number | undefined`.
///
/// Verbatim failure (un-ignored):
///
/// ```text
/// assertion `left == right` failed
///   left: Unknown(UnknownValue { raw: "unmodeledPosition", provenance: CompatibilityProjection })
///  right: Union([Primitive(Number), Primitive(Undefined)])
/// ```///
/// The fail-closed DISPOSITION is now POSITIONAL: the value is the typed
/// unresolved marker (projected `Unknown { raw: "unmodeledPosition" }`), the
/// result is a degraded success and nothing warms — so the row observes a
/// VALUE rather than `Miss`. The capability gap named below is unchanged.
///
/// Owning layer: the OPTIONAL-CALL capability — no arm routes `f?.()`
/// through the call carrier, so the `| undefined` arm is never
/// synthesised. The admission half is settled: the chain is a
/// `ValueDescent::UnmodeledCall` and fails closed instead of publishing
/// `any` warm.
#[test]
#[ignore = "ChainExpression has no optional-call arm: `f?.()` fails closed as an unmodeled call position"]
fn optional_chained_call_return_unions_undefined() {
    let host = ts_host();
    assert_eq!(
        value_of(&host, CALLS, "callOptional"),
        TypeExpr::Union(Arc::from(
            vec![number(), TypeExpr::Primitive(PrimitiveName::Undefined),].into_boxed_slice(),
        )),
    );
}

/// CANARY (landed) — a TAGGED TEMPLATE call returns the tag function's
/// return.
///
/// Oracle: `ReturnType<typeof callTagged>` is `boolean`.
///
/// The tagged template rides the one call carrier as a call of its tag
/// (`SliceCall::TaggedTemplate`), whose first argument is the template
/// strings and whose remaining arguments are the substitutions, so the
/// value is the tag's return, clean and warm.
#[test]
fn tagged_template_call_return_is_the_tag_return() {
    let host = ts_host();
    assert_clean_warm(&host, CALLS, "callTagged", boolean());
}

/// CANARY — an `async function`'s return type is `Promise<T>`, not `T`.
///
/// Oracle: `ReturnType<typeof callAsyncPlain>` is `Promise<number>` for
/// `async function callAsyncPlain() { return 1; }`.
///
/// Verbatim failure (before the wrap landed):
///
/// ```text
/// assertion `left != right` failed: an async function's return must not be the bare body type
///   left: Primitive(Number)
///  right: Primitive(Number)
/// ```
///
/// Owning layer (landed): the function-kind wrap — the skeleton carries
/// the authored `async` flag, the join defers the wrap past the equation
/// fixed point (so the fresh literal `1` widens to `number` first), and
/// the publication closure interns the registry-decided `Promise` builtin
/// carrier over the body join — the same identity an authored unshadowed
/// `Promise<…>` reference takes.
#[test]
fn async_function_return_is_wrapped_in_promise() {
    let host = ts_host();
    assert_eq!(
        value_of(&host, CALLS, "callAsyncPlain"),
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(vec![number()].into_boxed_slice()),
        },
        "an async function's return is Promise<body join>, warm"
    );
}

/// CANARY — an `async function*` returns `AsyncGenerator<…>`.
///
/// Oracle: `ReturnType<typeof callAsyncGen>` is
/// `AsyncGenerator<number, void, unknown>`.
///
/// Verbatim failure (before the wrap landed):
///
/// ```text
/// assertion `left != right` failed: an async generator's return must not be `void`
///   left: Primitive(Void)
///  right: Primitive(Void)
/// ```
///
/// Owning layer (landed): the generator wrap over the async-generator
/// kind — the yield join (`number`), the body's fall-through `void`, and
/// `unknown` next, over the resolved lib `AsyncGenerator` head.
#[test]
fn async_generator_return_is_wrapped_in_async_generator() {
    let host = ts_host();
    assert_eq!(
        value_of(&host, CALLS, "callAsyncGen"),
        TypeExpr::Ref {
            name: Arc::from("AsyncGenerator"),
            type_arguments: Arc::from(
                vec![
                    number(),
                    TypeExpr::Primitive(PrimitiveName::Void),
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                ]
                .into_boxed_slice()
            ),
        },
        "an async generator's return is AsyncGenerator<yield, return, next>"
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
/// assertion `left == right` failed
///   left: Unknown(UnknownValue { raw: "unmodeledPosition", provenance: CompatibilityProjection })
///  right: Ref { name: "Promise", type_arguments: [Primitive(Number)] }
/// ```///
/// The fail-closed DISPOSITION was POSITIONAL before the await arm
/// landed: the value was the typed unresolved marker (projected
/// `Unknown { raw: "unmodeledPosition" }`), a degraded success that
/// never warmed.
///
/// Owning layer (landed): TWO arms compose — the awaited call resolves
/// through the ONE call carrier (`ValueDescent::Awaited` descends both
/// halves onto the operand) and unwraps through the lib `Awaited`
/// instantiation, and the enclosing `async` re-wraps through the
/// function-kind wrap. Fixing only one arm would have published
/// `Promise<any>` (wrap without unwrap) or `number` (unwrap without
/// wrap) — both wrong.
#[test]
fn awaited_call_return_is_the_awaited_value_wrapped_again() {
    let host = ts_host();
    assert_eq!(
        value_of(&host, CALLS, "callAwait"),
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(vec![number()].into_boxed_slice()),
        },
    );
}

/// The Promise-EMBEDDING half of the async wrap's `Awaited`-collapse
/// arm: `callAwait` covers an `await` operand (the value is unwrapped
/// BEFORE the re-wrap), but a body that returns an already-`Promise`-typed
/// value directly (no `await`) exercises `materialize_flow_return_wrap`'s
/// unconditional `Awaited` dispatch over the body ITSELF — the join
/// carries `Promise<…>` at its top level, so the dispatch collapses it
/// before re-wrapping.
///
/// Oracle: `ReturnType<typeof callAsyncPassthrough>` is `Promise<number>`
/// for `async function callAsyncPassthrough() { return asyncSrc(); }` —
/// never `Promise<Promise<number>>`.
#[test]
fn async_return_of_an_already_promise_typed_value_collapses_the_embedded_carrier() {
    let host = ts_host();
    assert_eq!(
        value_of(&host, CALLS, "callAsyncPassthrough"),
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(vec![number()].into_boxed_slice()),
        },
        "a bare Promise-typed passthrough must collapse to Promise<number>, not Promise<Promise<number>>",
    );
}

/// A GENERIC async body publishes `Promise<T>` — the parameter itself,
/// never a spelled `Awaited<T>` and never a typed gap.
///
/// Oracle (tsc 7.0.2): `asyncGenericIdentity` declares `Promise<T>`, and
/// instantiating `T` with `Promise<string>` genuinely nests to
/// `Promise<Promise<string>>` — the checker does NOT collapse an
/// unresolved parameter, so publishing the collapse would be wrong and
/// gapping it would lose an answer the checker has.
#[test]
fn an_async_wrap_over_a_bare_generic_body_publishes_the_parameter() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        CALLS,
        "asyncGenericIdentity",
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(vec![type_param("T")].into_boxed_slice()),
        },
    );
}

/// A `Promise`-typed generic body collapses exactly ONE promise level, to
/// the same `Promise<T>` — tsgo: `asyncPromiseParam` is `Promise<T>`,
/// never `Promise<Promise<T>>`.
#[test]
fn an_async_wrap_over_a_promise_typed_generic_body_collapses_one_level() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        CALLS,
        "asyncPromiseParam",
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(vec![type_param("T")].into_boxed_slice()),
        },
    );
}

/// A union of a parameter and its own promise collapses to the single
/// parameter — tsgo: `asyncUnionParam` is `Promise<T>`.
#[test]
fn an_async_wrap_over_a_parameter_union_collapses_to_the_parameter() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        CALLS,
        "asyncUnionParam",
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(vec![type_param("T")].into_boxed_slice()),
        },
    );
}

/// `return await v` over a generic operand publishes the parameter
/// unchanged — tsgo: `asyncAwaitParam` is `Promise<T>`.
#[test]
fn an_awaited_generic_operand_publishes_the_parameter_unchanged() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        CALLS,
        "asyncAwaitParam",
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(vec![type_param("T")].into_boxed_slice()),
        },
    );
}

/// An async GENERATOR spells the DEFERRED `Awaited<T>` for a generic yield
/// — NOT the bare parameter the async function publishes. The two surfaces
/// have different generic rules, and tsc 7.0.2 prints them differently:
/// `AsyncGenerator<Awaited<T>, void, unknown>` here versus `Promise<T>` for
/// the async function (measured by declaration emit).
///
/// The deferred `Awaited<T>` is a `TypeExpr::IntrinsicApplication` — a
/// compiler-native operation — NOT a `Ref` spelled `Awaited`. The generator
/// used to fabricate an `InstantiationRef` carrier over a `__builtin__`
/// canonical for this; a `Ref` contributes its name to referenced-name
/// traversal, so that shape let a userland `Awaited` declaration collide
/// with the intrinsic.
#[test]
fn an_async_generator_over_a_generic_yield_spells_the_deferred_awaited() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        CALLS,
        "asyncGenYieldParam",
        TypeExpr::Ref {
            name: Arc::from("AsyncGenerator"),
            type_arguments: Arc::from(
                vec![
                    TypeExpr::IntrinsicApplication {
                        op: CompilerIntrinsicTypeOp::Awaited,
                        arguments: Arc::from(vec![type_param("T")].into_boxed_slice()),
                    },
                    TypeExpr::Primitive(PrimitiveName::Void),
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                ]
                .into_boxed_slice(),
            ),
        },
    );
}

/// …and for a generic RETURN — tsgo:
/// `AsyncGenerator<never, Awaited<T>, unknown>`.
#[test]
fn an_async_generator_over_a_generic_return_spells_the_deferred_awaited() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        CALLS,
        "asyncGenReturnParam",
        TypeExpr::Ref {
            name: Arc::from("AsyncGenerator"),
            type_arguments: Arc::from(
                vec![
                    TypeExpr::Primitive(PrimitiveName::Never),
                    TypeExpr::IntrinsicApplication {
                        op: CompilerIntrinsicTypeOp::Awaited,
                        arguments: Arc::from(vec![type_param("T")].into_boxed_slice()),
                    },
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                ]
                .into_boxed_slice(),
            ),
        },
    );
}

/// The SYNC control for both generic rules: a plain generator publishes the
/// parameter verbatim, with no collapse and no deferred spelling — tsgo:
/// `Generator<T, string, unknown>`.
#[test]
fn a_sync_generator_over_a_generic_yield_publishes_the_parameter_verbatim() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        LEAF,
        "leafGeneratorYieldParam",
        TypeExpr::Ref {
            name: Arc::from("Generator"),
            type_arguments: Arc::from(
                vec![
                    type_param("T"),
                    string(),
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                ]
                .into_boxed_slice(),
            ),
        },
    );
}

/// A YIELDED settled call widens its fresh literal return exactly as a
/// RETURNED one does — tsgo: `Generator<number, string, unknown>`.
/// Reading the freshness before evaluation, and never consulting the call's
/// own fresh-literal deposit, published `1`.
#[test]
fn a_yielded_settled_call_widens_its_fresh_literal_as_a_returned_one_does() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        LEAF,
        "leafGeneratorYieldCall",
        TypeExpr::Ref {
            name: Arc::from("Generator"),
            type_arguments: Arc::from(
                vec![
                    number(),
                    string(),
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                ]
                .into_boxed_slice(),
            ),
        },
    );
}

/// The FALSIFICATION twin: two yielded settled calls are two distinct
/// literal constituents, so the lone-fresh-arm widening must not fire —
/// tsgo: `Generator<1 | 2, string, unknown>`.
#[test]
fn two_yielded_settled_calls_keep_both_literal_constituents() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        LEAF,
        "leafGeneratorYieldTwoCalls",
        TypeExpr::Ref {
            name: Arc::from("Generator"),
            type_arguments: Arc::from(
                vec![
                    TypeExpr::union(vec![
                        TypeExpr::number_literal(2.0),
                        TypeExpr::number_literal(1.0),
                    ]),
                    string(),
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                ]
                .into_boxed_slice(),
            ),
        },
    );
}

/// `return await 1` — the AWAITED operand carries its own FRESHNESS.
///
/// The lib `Awaited` surface passes a SETTLED literal through verbatim,
/// so `return await 1` contributes exactly the fresh literal arm
/// `return 1` does and the lone-fresh-arm widening fires. Oracle:
/// `ReturnType<typeof callAwaitFreshLiteral>` is `Promise<number>`.
///
/// Classifying the whole `AwaitExpression` as PINNED (the freshness
/// mirror had no await arm) published `Promise<1>` — wrong-complete,
/// and warm.
#[test]
fn an_awaited_fresh_literal_return_widens_exactly_as_the_bare_literal_does() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        CALLS,
        "callAwaitFreshLiteral",
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(vec![number()].into_boxed_slice()),
        },
    );
}

/// The FALSIFICATION twin of the widening above: TWO awaited fresh arms
/// are two DISTINCT literal constituents, so the lone-fresh-arm rule must
/// NOT fire. Oracle: `ReturnType<typeof callAwaitFreshTernary>` is
/// `Promise<1 | 2>` — recursing freshness through `await` must not become
/// blanket widening.
#[test]
fn two_awaited_fresh_literal_arms_keep_both_literal_constituents() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        CALLS,
        "callAwaitFreshTernary",
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(
                vec![TypeExpr::union(vec![
                    TypeExpr::number_literal(2.0),
                    TypeExpr::number_literal(1.0),
                ])]
                .into_boxed_slice(),
            ),
        },
    );
}

/// An async generator AWAITS what it YIELDS: the yield parameter rides the
/// same shared lib `Awaited` surface the async wrap applies to its body
/// join. Oracle: `ReturnType<typeof callAsyncGenYieldPromise>` is
/// `AsyncGenerator<number, void, unknown>` — never
/// `AsyncGenerator<Promise<number>, void, unknown>`.
#[test]
fn an_async_generator_yield_join_is_awaited_through_the_shared_lib_surface() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        CALLS,
        "callAsyncGenYieldPromise",
        TypeExpr::Ref {
            name: Arc::from("AsyncGenerator"),
            type_arguments: Arc::from(
                vec![
                    number(),
                    TypeExpr::Primitive(PrimitiveName::Void),
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                ]
                .into_boxed_slice(),
            ),
        },
    );
}

/// …and what it RETURNS: the same collapse applies to the body join, so
/// the wrap never publishes a `Promise`-shaped TReturn. Oracle:
/// `ReturnType<typeof callAsyncGenReturnPromise>` is
/// `AsyncGenerator<number, number, unknown>`.
#[test]
fn an_async_generator_return_join_is_awaited_through_the_same_surface() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        CALLS,
        "callAsyncGenReturnPromise",
        TypeExpr::Ref {
            name: Arc::from("AsyncGenerator"),
            type_arguments: Arc::from(
                vec![
                    number(),
                    number(),
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                ]
                .into_boxed_slice(),
            ),
        },
    );
}

/// The SYNC negative control: a plain generator does NOT await what it
/// yields — the collapse belongs to the ASYNC kinds alone. Oracle:
/// `ReturnType<typeof leafGeneratorYieldPromise>` is
/// `Generator<Promise<number>, string, unknown>`.
#[test]
fn a_sync_generator_yield_join_is_never_awaited() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        LEAF,
        "leafGeneratorYieldPromise",
        TypeExpr::Ref {
            name: Arc::from("Generator"),
            type_arguments: Arc::from(
                vec![
                    TypeExpr::Ref {
                        name: Arc::from("Promise"),
                        type_arguments: Arc::from(vec![number()].into_boxed_slice()),
                    },
                    string(),
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                ]
                .into_boxed_slice(),
            ),
        },
    );
}

/// NEGATIVE LEG — a wrap whose lib head the environment cannot
/// resolve keeps the TYPED GAP and never warms.
///
/// `negGen`'s file declares no `Generator` surface, so the wrap's shared
/// bare-ref head resolution misses and the materialization publishes the
/// typed marker: a degraded success with zero warm candidates. Contrast
/// `leafGenerator` (same shape, with the surface declared), which
/// publishes `Generator<number, string, unknown>` clean and warm.
#[test]
fn generator_wrap_without_lib_surface_keeps_typed_gap_and_never_warms() {
    let host = ts_host();
    match eval(&host, NEG, "negGen") {
        Outcome::Value {
            degradation,
            candidates,
            ..
        } => {
            assert!(degradation.is_some(), "the unresolved wrap must degrade");
            assert_eq!(
                candidates, 0,
                "a gapped wrap is ReturnOnly — it must never warm"
            );
        }
        other => panic!("negGen must produce a degraded value, got {other:?}"),
    }
}

/// The Promise-EMBEDDING collapse over a STRUCTURAL thenable payload follows
/// the checker's thenable protocol, clean and warm.
///
/// `negAsyncThenable` returns `Promise<NegThenable>` where `NegThenable` is
/// `{ then(cb: (v: number) => void): void }`: the payload relation unwraps
/// the `Promise`, reaches the interface carrier, expands it, and reads the
/// promised value off the callback's first parameter. Oracle (tsc 7.0.2
/// declaration emit): `Promise<number>` — never `Promise<NegThenable>` and
/// never the typed gap this row pinned while structural thenables were
/// unmodelled. The honest-refusal leg for a `then` this reader cannot
/// enumerate is `awaited_decides_then_bearing_surfaces_by_callability`.
#[test]
fn async_wrap_awaited_collapse_over_a_thenable_payload_publishes_the_promised_value() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        NEG,
        "negAsyncThenable",
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(vec![number()].into_boxed_slice()),
        },
    );
}

/// Wrapped results admit WARM and REPLAY equal to FRESH.
///
/// The wrap is materialized inside the sealed value (the proof covers the
/// wrapped node), so the memo stores it and every later demand — the same
/// dispatch's warm read, and a FRESH dispatch over the same store —
/// publishes the identical wrapped answer.
#[test]
fn wrapped_async_return_admits_warm_and_replay_equal_to_fresh() {
    let host = ts_host();
    let expected = TypeExpr::Ref {
        name: Arc::from("Promise"),
        type_arguments: Arc::from(vec![number()].into_boxed_slice()),
    };
    // Fresh, then warm on the same dispatch.
    assert_clean_warm(&host, CALLS, "callAsyncPlain", expected.clone());
    assert_clean_warm(&host, CALLS, "callAsyncPlain", expected.clone());
    // Replay: a fresh dispatch over the same store answers warm-equal.
    assert_clean_warm(&host, CALLS, "callAsyncPlain", expected);
}

/// BOUNDED WORK: one awaited-relation build PER DEMANDED OPERAND, and
/// identical warm demand adds ZERO builds.
///
/// `callAwait` is `return await asyncSrc()` where `asyncSrc(): Promise<number>`,
/// so the cold demand measures 1: `AwaitedNormalize(Promise<number>)`, whose
/// run unwraps the payload `number` as a TAIL step in its own loop rather
/// than re-entering the family — the shape that lets a chain of any length
/// cost no query depth. A union arm is the one nested unwrap that still
/// re-enters the family, under its own memo entry.
///
/// The load-bearing half is the SECOND assertion: warm repeats add ZERO. A
/// warm family hit returns before the builder runs, so the per-family counter
/// is what makes that observable at all.
///
/// This counts `awaited_normalize_count`, NOT `instantiate_count`: the
/// unwrap used to be a lib `Instantiate` of `Awaited<T>` and is now its own
/// family, so the instantiate counter no longer moves for it. The claim
/// under test is unchanged — bounded cold work, zero warm work — and the
/// per-family counter is what makes "zero warm work" observable at all,
/// since a warm family hit returns before the builder runs.
#[test]
fn wrapped_await_demand_adds_one_cold_instantiation_and_zero_warm() {
    let host = ts_host();
    let before = host
        .project_type_store()
        .semantic_graph()
        .stats_snapshot()
        .awaited_normalize_count;
    assert_clean_warm(
        &host,
        CALLS,
        "callAwait",
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(vec![number()].into_boxed_slice()),
        },
    );
    let cold = host
        .project_type_store()
        .semantic_graph()
        .stats_snapshot()
        .awaited_normalize_count;
    assert_eq!(
        cold - before,
        1,
        "the cold awaited-unwrap demand pays exactly one build: the payload is a tail step"
    );
    // bounded-loop: three warm repeats, a fixed demand count
    for _ in 0..3 {
        assert_clean_warm(
            &host,
            CALLS,
            "callAwait",
            TypeExpr::Ref {
                name: Arc::from("Promise"),
                type_arguments: Arc::from(vec![number()].into_boxed_slice()),
            },
        );
    }
    let warm = host
        .project_type_store()
        .semantic_graph()
        .stats_snapshot()
        .awaited_normalize_count;
    assert_eq!(
        warm, cold,
        "identical warm demand must add ZERO awaited-relation builds"
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
///   left: Value { ty: Unknown(UnknownValue { raw: "unmodeledPosition", provenance: CompatibilityProjection }), degradation: Some(UnmodeledPosition), candidates: 0 }
///  right: Value { ty: Primitive(String), degradation: None, candidates: 1 }
/// ```///
/// The fail-closed DISPOSITION is now POSITIONAL: the value is the typed
/// unresolved marker (projected `Unknown { raw: "unmodeledPosition" }`), the
/// result is a degraded success and nothing warms — so the row observes a
/// VALUE rather than `Miss`. The capability gap named below is unchanged.
///
/// Owning layer: the flow evaluator's computed-member arm — a
/// `unique symbol` key is a `ComputedMemberExpression`, so the callee
/// cannot be represented and the member's own return is never reached.
/// The admission half is settled: an unrepresentable callee fails closed
/// rather than publishing the shallow leaf's `any` warm.
#[test]
#[ignore = "a `unique symbol` computed member key has no resolution rule: the member call fails closed as an unrepresentable callee"]
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
///   left: Value { ty: Unknown(UnknownValue { raw: "semanticMiss", provenance: CompatibilityProjection }), degradation: Some(UnresolvedValue), candidates: 0 }
///  right: Value { ty: Primitive(String), degradation: None, candidates: 1 }
/// ```
///
/// The graph node is `Opaque(Miss)` and the candidate count is 0: the
/// miss now carries `UnresolvedValue` and admits nothing, so every
/// enclosing composition sees the partial. Only the ADMISSION half was
/// fixed — the value is still a miss, and producing the member's real
/// type is the capability this canary is waiting on.
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
/// honest answer", and as a LOCAL answer it is. It was never a complete
/// RESULT, and it used to publish as one: `degradation: None`,
/// warm-admitted, so `execute_function_return_source` never folded the
/// cache-read rails and an enclosing composition warmed with an opaque
/// interior. That admission half is fixed and separately guarded; what
/// this canary still owns is the missing member projection itself.
#[test]
fn member_read_off_an_annotated_parameter_resolves_to_the_member_type() {
    let host = ts_host();
    assert_clean_warm(&host, TL, "tlPlainMember", string());
    // The same read at the nesting depths the evaluation composes as
    // FLOW expressions: an object-literal member value, an array element
    // and a nested function's return. Each resolves through the
    // parameter's annotation — clean, warm, the checker's own answer
    // (`{ q: string }`, `string[]`, `() => string`).
    let assert_clean = |name: &str| -> TypeExpr {
        match eval(&host, TL, name) {
            Outcome::Value {
                ty,
                degradation,
                candidates,
            } => {
                assert_eq!(
                    (degradation, candidates),
                    (None, 1),
                    "{name} must resolve clean and warm"
                );
                ty
            }
            other => panic!("{name} must produce a value, got {other:?}"),
        }
    };
    let object = assert_clean("tlMissCarrierInObjectMember");
    assert_eq!(
        projected_member(&object, "q"),
        &TypeExpr::Primitive(PrimitiveName::String)
    );
    let array = assert_clean("tlMissCarrierInArray");
    assert!(
        matches!(
            &array,
            TypeExpr::Array { element, readonly: false }
                if **element == TypeExpr::Primitive(PrimitiveName::String)
        ),
        "tlMissCarrierInArray: {array:?}"
    );
    let nested = assert_clean("tlMissCarrierInNestedFunction");
    assert_eq!(
        projected_function_return(&nested),
        &TypeExpr::Primitive(PrimitiveName::String)
    );
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
///   left: Value { ty: Unknown(UnknownValue { raw: "semanticMiss", provenance: CompatibilityProjection }), degradation: Some(UnresolvedValue), candidates: 0 }
///  right: Value { ty: Primitive(String), degradation: None, candidates: 1 }
/// ```
///
/// Owning layer: the same member arm as the unconstrained row — the
/// constraint is never reached, so this row does not yet discriminate
/// constraint resolution specifically. The miss is `ReturnOnly`, so
/// nothing warms on it.
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

// ──────────────────────────────────────────────────────────────────────
// PRIORITY 8 — the two admission invariants
//
// Both are about what may be published WARM, not about what value the
// substrate can compute. Each pins the graph node, the degradation, and
// the candidate count on every row.
// ──────────────────────────────────────────────────────────────────────

/// Assert one function produces a USABLE value that carries the
/// `UnresolvedValue` verdict and admits NOTHING, and that the value's
/// graph node is the one `probe` expects.
#[track_caller]
fn assert_unresolved_value(
    host: &Arc<VerterHost>,
    canonical: &str,
    name: &str,
    probe: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId),
) {
    match eval(host, canonical, name) {
        Outcome::Value {
            degradation,
            candidates,
            ..
        } => {
            assert_eq!(
                degradation,
                Some(FlowReturnDegradation::UnresolvedValue),
                "{name} must carry the unresolved-value verdict"
            );
            assert_eq!(candidates, 0, "{name} must admit nothing");
        }
        other => panic!("{name} must produce a degraded value, got {other:?}"),
    }
    with_dispatch(host, |dispatch| {
        let key = key_of(dispatch, canonical, name);
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
        else {
            panic!("{name} must produce a value");
        };
        probe(dispatch, result.return_type());
    });
}

/// A value that REACHES a semantic-miss carrier is never admitted warm as
/// a complete result — at the top level, and at every nesting depth the
/// evaluation composes.
///
/// A miss carrier is an honest LOCAL answer: the leaf really did resolve
/// to nothing. It is not a complete RESULT. Published warm with
/// `degradation: None` it hands an enclosing composition an opaque
/// interior with NO partial marker —
/// `execute_function_return_source` never folds the cache-read rails, so
/// a `get_component_meta` / shape / materialize result built on top warms
/// with the opacity inside it. That is precisely what the degradation
/// channel exists to prevent, and `CLAUDE.md`'s Stub Prevention section
/// names the shape ("an always-`Opaque(Miss)` resolve is a nop").
///
/// Four member-path rows once lived here (`tlPlainMember` and its three
/// nesting depths — an object member value, an array element, a nested
/// function's return): each declined a frame-rooted `x.q` read as a
/// miss-carrier value. The frame-rooted member-path projection now
/// resolves the reads the evaluation composes as FLOW expressions (the
/// plain read, the object member, the array element, the nested-function
/// return — clean, warm, the checker's own `string`), so those moved to
/// the canary
/// `member_read_off_an_annotated_parameter_resolves_to_the_member_type`.
/// What remains is the genuinely unresolvable read:
///
/// ```text
/// tlFreeUnresolvedRead           the FREE-leaf arm — no FrameShadowed
///                                carrier is involved at all
/// ```
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict
/// --ignoreConfig`): `tlFreeUnresolvedRead` is a program tsgo REJECTS
/// (`Cannot find name 'noSuchGlobalValue'.`), so there is no honest value
/// to publish for it at all.
///
/// Mutation recipe: returning `false` unconditionally from
/// `flow_return_value_is_unresolved` flips the row to a warm
/// `candidates: 1`.
#[test]
fn a_value_reaching_a_miss_carrier_is_never_admitted_warm() {
    let host = ts_host();

    // Top-level miss, reached through the FREE-leaf arm.
    assert_unresolved_value(&host, TL, "tlFreeUnresolvedRead", |dispatch, node| {
        assert_eq!(node_shape(dispatch, node), NodeShape::Opaque);
    });
}

/// The DISCRIMINATOR for the row above: a deferred CARRIER is not a miss,
/// and a fully-resolved composition is not either.
///
/// Without this, "everything degrades" would pass the test above just as
/// well as the rule does.
///
/// `tlMissingParamAnnotation(x: NoSuchTypeName)` is the sharp case: tsgo
/// REJECTS the program (`Cannot find name 'NoSuchTypeName'.`), yet the
/// parameter lowers to a `BareRef` carrier — an addressable, deferred
/// reference — not to `Opaque(Miss)`. The verdict is taken on the NODE,
/// so it stays clean and warm: unresolved-at-lowering-time is not the
/// same fact as not-known, and the walk must not conflate them. The
/// carrier's own re-resolution is where that program's error surfaces.
#[test]
fn a_deferred_carrier_and_a_resolved_composition_still_admit_warm() {
    let host = ts_host();

    assert_eq!(
        eval(&host, TL, "tlMissingParamAnnotation"),
        Outcome::Value {
            ty: type_ref("NoSuchTypeName"),
            degradation: None,
            candidates: 1,
        },
        "a deferred BareRef carrier is not a miss"
    );
    flow_node(&host, TL, "tlMissingParamAnnotation", |dispatch, node| {
        assert_eq!(
            node_shape(dispatch, node),
            NodeShape::BareRef("NoSuchTypeName".to_string())
        );
    });

    // A fully-resolved nested composition of exactly the shapes the walk
    // descends (object → object → primitive) stays clean and warm.
    let value = value_of(&host, TL, "tlObjReturn");
    assert_eq!(projected_member(&value, "m"), &string());
    assert_eq!(
        projected_member(projected_member(&value, "n"), "deep"),
        &boolean()
    );
    flow_node(&host, TL, "tlObjReturn", |_, _| {});
}

/// A CALL POSITION with no structural arm fails closed — decided on the
/// expression FORM, not on whether the shallow pass happened to mint a
/// call-return carrier.
///
/// The call-position gate promises that a call the content half could
/// not route through its call carrier fails closed. The gate that
/// delivered it (`embeds_call_return_carrier`) reads the leaf ANSWER, so
/// it only ever fired when the shared shallow pass produced an unreduced
/// `ReturnType<callee>` carrier. For every form below that pass answers a
/// bare `any` instead — no carrier, no gate, and the fabricated `any`
/// published clean and WARM with `candidates: 1`. The promise was
/// therefore broader than the mechanism, which is the defect.
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict
/// --ignoreConfig`) — the answer the fail-closed arm declines to
/// produce, different from `any`:
///
/// ```text
/// callOptional    maybeFn?.()                     number | undefined
/// ```
///
/// `await asyncSrc()` is NOT in this set: an awaited
/// call is a modelled form (`ValueDescent::Awaited`) whose operand rides
/// the call carrier, so `callAwait` publishes `Promise<number>` (see
/// `awaited_call_return_is_the_awaited_value_wrapped_again`). Neither is
/// `new` nor a tagged template: a construction rides the call carrier to
/// the construct signatures (see
/// `construct_expression_return_is_the_instance_type`), and a tagged
/// template to its tag's call signatures (see
/// `tagged_template_call_return_is_the_tag_return`).
///
/// Mutation recipe: `value_is_unmodeled_call` is the single authority
/// (both `value_descent`'s guarded arm and the content half's residual
/// type-carrier check delegate to it), so flipping one of its arms flips
/// exactly the matching rows — `ChainElement::CallExpression` to `false`
/// flips `callOptional` back to a warm `any`.
#[test]
fn an_unmodeled_call_position_fails_closed_whatever_the_shallow_pass_answered() {
    let host = ts_host();
    assert_fails_closed(&host, CALLS, "callOptional");
}

/// D7 — the SEQUENCE wrapping of a call does not change the call's
/// verdict, in either direction.
///
/// The sequence's value is its last operand, so a call there lowers
/// through the SAME structural call rails its bare spelling takes:
/// `(0, restFn(1, 2, 3))` surfaces `restFn`'s `"rest"` clean and warm,
/// exactly like the bare `callRest` twin, `(0, new CtorC(1))` surfaces
/// the constructed `CtorC` exactly like the bare `callNew`, and
/// `` (0, tag`a${1}b`) `` surfaces the tag's `boolean` exactly like the
/// bare `callTagged` — the sequence context never converts a resolved
/// call into a fail-closed marker.
///
/// The discriminator runs the other way with the same fixture: a call
/// form with NO structural arm (an optional call) keeps the fail-closed
/// verdict when a sequence wraps it —
/// the sequence context never converts an unmodeled call into a
/// published value either. The delegation answers the CALL's own
/// question; it invents no arm. An `await` last operand is a MODELLED
/// form, so `(0, await asyncSrc())` surfaces
/// `Promise<number>` clean and warm — the await's own verdict, kept by
/// the sequence exactly as the resolved bare call's is.
///
/// Oracle (TypeScript 7.0.2 `tsc`, `--noEmit --strict
/// --ignoreConfig`):
///
/// ```text
/// callSeqNew       CtorC                (published)
/// callSeqTagged    boolean              (published)
/// callSeqOptional  number | undefined   (declined — fails closed)
/// ```
///
/// Mutation recipe: dropping the sequence delegation from the content
/// half's sequence arm flips `callSeqRest`, `callSeqNew`, `callSeqTagged`
/// and `callSeqAwait` to the fail-closed marker; widening the delegation
/// past the paren-transparent `CallExpression` / `NewExpression` /
/// `TaggedTemplateExpression` forms (a chain last operand) flips the
/// negative row to a published value.
#[test]
fn a_sequence_wrapped_call_keeps_the_calls_own_verdict() {
    let host = ts_host();
    // A resolved call surfaces through the sequence, clean and warm.
    assert_clean_warm(&host, CALLS, "callSeqRest", string_lit("rest"));
    assert_clean_warm(&host, CALLS, "callSeqNew", type_ref("CtorC"));
    assert_clean_warm(&host, CALLS, "callSeqTagged", boolean());
    // An awaited call keeps its own modelled verdict through the
    // sequence: resolved operand, `Awaited` unwrap, async re-wrap.
    assert_clean_warm(
        &host,
        CALLS,
        "callSeqAwait",
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(vec![number()].into_boxed_slice()),
        },
    );
    // A form with no structural arm keeps failing closed.
    assert_fails_closed(&host, CALLS, "callSeqOptional");
}

/// The DISCRIMINATOR for the call-position rule: the forms that are NOT
/// call positions keep answering, and the modeled call arms are
/// untouched.
///
/// `a?.b` is an optional MEMBER read, not an optional call — the chain
/// guard must look at the chain element, not at the `?.` token.
/// `restFn(1, 2, 3)` is a plain call with a direct-call arm.
/// `geoIifeInside` is an IIFE, whose value is the nested function's
/// evaluated return. None of the three may be swept up.
#[test]
fn non_call_forms_and_modeled_call_arms_are_untouched_by_the_call_position_gate() {
    let host = ts_host();
    // A modeled direct call still resolves.
    assert_clean_warm(&host, CALLS, "callRest", string_lit("rest"));
    // An IIFE still resolves through the nested-function arm (its lone
    // fresh literal contributor widens, exactly as a plain body's does).
    assert_clean_warm(&host, GEO, "geoIifeInside", string());
    // A typed optional MEMBER read is not a call position: it publishes
    // the member's type over the NULLISH-STRIPPED base with `undefined`
    // unioned in — the checker's `string | undefined`, clean and warm
    // (the carrier's own canary,
    // `optional_member_read_return_is_the_stripped_member_or_undefined`,
    // pins the shape; this row pins only that the call-position gate
    // leaves it alone).
    assert_clean_warm(
        &host,
        CALLS,
        "callOptionalMemberRead",
        TypeExpr::union(vec![
            string(),
            TypeExpr::Primitive(PrimitiveName::Undefined),
        ]),
    );
}

/// A yield-position hold must never leak into the RETURN equation's own
/// join. `sccGenYield` and `sccGenPartner` form one SCC component through
/// the YIELD-position call (`yield sccGenPartner(c)`), not through a
/// return-position one: `sccGenYield`'s own `return` is the unconditional
/// literal `1`, and `sccGenPartner`'s hold-back to `sccGenYield` is a
/// RETURN-position edge on `sccGenPartner`, not on `sccGenYield`.
///
/// Before the fix, the hold `sccGenPartner`'s call registered while
/// evaluating the yield argument was never dropped from the frame's hold
/// list, so the SCC fixed point folded `sccGenPartner`'s resolved return
/// into `sccGenYield`'s own return-type join — publishing a return
/// parameter of `"s" | 1` instead of the correct `number`. The yield
/// parameter alone carries `sccGenPartner`'s contribution; the RETURN
/// parameter — this row's subject — must stay exactly `sccGenYield`'s own
/// widened return-position value.
///
/// No checker answer pins the YIELD parameter for this shape: tsgo declines
/// the whole mutually-recursive pair with TS7023 (`implicitly has return
/// type 'any' ... referenced directly or indirectly in one of its return
/// expressions`), so this row is an internal contract about hold
/// contamination rather than a checker comparison. The yield parameter
/// follows the same rule a return position does — a yielded call's
/// fresh-literal deposit widens as a lone arm — which the two shapes the
/// checker CAN type do pin: an unrecursive twin publishes its callee's
/// return verbatim (`ctlPartner(c): "s" | "t"`), and a single-literal
/// callee widens inside its own return inference.
#[test]
fn yield_position_hold_does_not_contaminate_the_return_position_join() {
    let host = ts_host();
    assert_eq!(
        value_of(&host, LEAF, "sccGenYield"),
        TypeExpr::Ref {
            name: Arc::from("Generator"),
            type_arguments: Arc::from(
                vec![string(), number(), TypeExpr::Primitive(PrimitiveName::Unknown)]
                    .into_boxed_slice()
            ),
        },
        "the return parameter must be sccGenYield's own return join, never sccGenPartner's yielded value",
    );
}

/// `negAsync`'s wrap has no generator lib-head dependency at all (plain
/// `async`, no `Generator`/`AsyncGenerator` lookup), so — unlike `negGen`
/// — the missing lib surface in this file never touches it: it must
/// resolve exactly like `callAsyncPlain` does.
#[test]
fn plain_async_wrap_is_unaffected_by_a_missing_generator_lib_surface() {
    let host = ts_host();
    assert_clean_warm(
        &host,
        NEG,
        "negAsync",
        TypeExpr::Ref {
            name: Arc::from("Promise"),
            type_arguments: Arc::from(vec![number()].into_boxed_slice()),
        },
    );
}

/// The async wrap over a SELF-REFERENTIAL promise: `negAsyncSrc`'s
/// declared return is the alias `NegSelfProm = Promise<NegSelfProm>`,
/// whose payload is the alias itself.
///
/// tsc 7.0.2 (`--noEmit --strict`) accepts the alias and types
/// `negAsyncSelfRef` `Promise<any>`, reporting TS1062 ("Type is referenced
/// directly or indirectly in the fulfillment callback of its own 'then'
/// method.") at its name — the checker's promised-type recursion rule,
/// whose own case is a promise whose promised type is the promise itself.
/// The payload relation resolves the alias to `Promise<NegSelfProm>`,
/// re-enters on the payload, finds the SAME alias already being unwrapped
/// and fails with that diagnostic: the wrap publishes the checker's
/// recovery, complete and never warm (the failure is a fact about the
/// path), instead of looping or answering a meaningless carrier.
#[test]
fn async_wrap_over_a_self_referential_promise_recovers_under_ts1062() {
    let host = ts_host();
    assert_eq!(
        eval(&host, NEG, "negAsyncSelfRef"),
        Outcome::Value {
            ty: TypeExpr::Ref {
                name: Arc::from("Promise"),
                type_arguments: Arc::from(
                    vec![TypeExpr::Primitive(PrimitiveName::Any)].into_boxed_slice()
                ),
            },
            degradation: None,
            candidates: 0,
        }
    );
}

/// Structural matcher for [`awaited_relation_oracle_matrix`]. Spans are part
/// of `TypeExpr` equality, so object rows cannot be spelled as one expected
/// value.
#[derive(Debug)]
enum Shape {
    /// A `TypeParameter` named `T`.
    T,
    /// A primitive with this name.
    Primitive(PrimitiveName),
    /// A `Ref` with this name and argument shapes.
    Ref(&'static str, &'static [Shape]),
    /// The compiler-native `Awaited` operation over one argument — what a
    /// position that DEMANDS the awaited meaning mints.
    Awaited(&'static Shape),
    /// An authored `Awaited<…>` kept in its syntax-preserving spelling. Where
    /// nothing demands its meaning, the checker-visible type is already right
    /// and resolving it only to change the representation would be eager
    /// normalization, so the row pins that it is NOT resolved.
    AuthoredAwaited(&'static Shape),
    /// A mutable array of this element.
    Array(&'static Shape),
    /// An object with exactly this one member.
    Member(&'static str, &'static Shape),
}

fn shape_matches(shape: &Shape, ty: &TypeExpr) -> bool {
    match (shape, ty) {
        (Shape::T, TypeExpr::TypeParameter(param)) => &*param.name == "T",
        (Shape::Primitive(name), TypeExpr::Primitive(got)) => name == got,
        (
            Shape::Ref(name, args),
            TypeExpr::Ref {
                name: got,
                type_arguments,
            },
        ) => {
            got.as_ref() == *name
                && args.len() == type_arguments.len()
                && args
                    .iter()
                    .zip(type_arguments.iter())
                    .all(|(arg, got)| shape_matches(arg, got))
        }
        (
            Shape::AuthoredAwaited(arg),
            TypeExpr::Ref {
                name,
                type_arguments,
            },
        ) => {
            name.as_ref() == "Awaited"
                && type_arguments.len() == 1
                && shape_matches(arg, &type_arguments[0])
        }
        (
            Shape::Awaited(arg),
            TypeExpr::IntrinsicApplication {
                op: CompilerIntrinsicTypeOp::Awaited,
                arguments,
            },
        ) => arguments.len() == 1 && shape_matches(arg, &arguments[0]),
        (
            Shape::Array(element),
            TypeExpr::Array {
                element: got,
                readonly: false,
            },
        ) => shape_matches(element, got),
        (Shape::Member(key, value), TypeExpr::Object(object)) => {
            object.properties.len() == 1 && shape_matches(value, projected_member(ty, key))
        }
        _ => false,
    }
}

/// The awaited-relation ORACLE MATRIX, pinned as rows. Every row publishes
/// the checker's type clean and warm.
///
/// Two relations meet here and must never merge: the async RETURN payload
/// (`return v` in an async function) and the AWAIT expression (`await v`).
/// The checker keeps a bare parameter under the first and defers
/// `Awaited<T>` under the second unless the constraint already settles it,
/// so x1 != r1 and x4 != r4 are the discriminating pairs.
///
/// g1 against g6 / g7 pins that the return payload resolves and strips only
/// a TOP-LEVEL authored `Awaited`: the payload position demands its meaning.
/// Under an array nothing demands it, so g6 / g7 keep the authored spelling,
/// and so does the synchronous control g8.
///
/// Each `tsc` column is the declaration emit of the fixture in `CALLS_SRC`,
/// measured on the repository's TypeScript 7.0.2 with
///
/// ```text
/// tsc --ignoreConfig --declaration --emitDeclarationOnly --target es2022 --strict
/// ```
///
/// (the x rows read the local's type off the returned `{ a }`).
#[test]
fn awaited_relation_oracle_matrix() {
    const PROMISE_T: Shape = Shape::Ref("Promise", &[Shape::T]);
    const AWAITED_T: Shape = Shape::Awaited(&Shape::T);
    const AUTHORED_AWAITED_T: Shape = Shape::AuthoredAwaited(&Shape::T);
    const PROMISE_A_AWAITED_T: Shape = Shape::Ref("Promise", &[Shape::Member("a", &AWAITED_T)]);
    const PROMISE_AUTHORED_ARRAY: Shape =
        Shape::Ref("Promise", &[Shape::Array(&AUTHORED_AWAITED_T)]);
    let rows: &[(&str, &str, &str, Shape)] = &[
        ("r1", "asyncGenericIdentity", "Promise<T>", PROMISE_T),
        ("r2", "matrixR2", "Promise<T>", PROMISE_T),
        ("r3", "matrixR3", "Promise<T>", PROMISE_T),
        ("r4", "matrixR4", "Promise<T>", PROMISE_T),
        (
            "x1",
            "matrixX1",
            "Promise<{ a: Awaited<T>; }>",
            PROMISE_A_AWAITED_T,
        ),
        (
            "x2",
            "matrixX2",
            "Promise<{ a: T; }>",
            Shape::Ref("Promise", &[Shape::Member("a", &Shape::T)]),
        ),
        (
            "x3",
            "matrixX3",
            "Promise<{ a: Awaited<T>; }>",
            PROMISE_A_AWAITED_T,
        ),
        (
            "x4",
            "matrixX4",
            "Promise<{ a: Awaited<T>; }>",
            PROMISE_A_AWAITED_T,
        ),
        (
            "y1",
            "asyncGenYieldParam",
            "AsyncGenerator<Awaited<T>, void, unknown>",
            Shape::Ref(
                "AsyncGenerator",
                &[
                    AWAITED_T,
                    Shape::Primitive(PrimitiveName::Void),
                    Shape::Primitive(PrimitiveName::Unknown),
                ],
            ),
        ),
        (
            "y2",
            "matrixY2",
            "AsyncGenerator<T, void, unknown>",
            Shape::Ref(
                "AsyncGenerator",
                &[
                    Shape::T,
                    Shape::Primitive(PrimitiveName::Void),
                    Shape::Primitive(PrimitiveName::Unknown),
                ],
            ),
        ),
        ("g1", "matrixG1", "Promise<T>", PROMISE_T),
        (
            "g6",
            "matrixG6",
            "Promise<Awaited<T>[]>",
            PROMISE_AUTHORED_ARRAY,
        ),
        (
            "g7",
            "matrixG7",
            "Promise<Awaited<T>[]>",
            PROMISE_AUTHORED_ARRAY,
        ),
        ("g8", "matrixG8", "Awaited<T>", AUTHORED_AWAITED_T),
    ];
    let host = ts_host();
    let mut failures = Vec::new();
    for (id, function, tsc, shape) in rows {
        let outcome = eval(&host, CALLS, function);
        let pinned = matches!(
            &outcome,
            Outcome::Value { ty, degradation: None, candidates: 1 } if shape_matches(shape, ty)
        );
        if !pinned {
            failures.push(format!(
                "{id} ({function}): tsc `{tsc}`, pinned {shape:?}, measured {outcome:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The negative twin of matrix row g1: a module-local `type Awaited<X>`
/// shadows the lib declaration, so the top-level carrier is a USERLAND
/// declaration. The async return payload must never strip it to `T` nor turn
/// it into the compiler intrinsic; as an ordinary non-thenable alias it is
/// its own payload and keeps its identity.
///
/// Oracle (tsc 7.0.2 declaration emit): `Promise<Awaited<T>>`, where
/// `Awaited` is the local `{ w: X }` alias.
#[test]
fn a_shadowed_awaited_at_the_top_of_an_async_return_is_not_stripped() {
    const SHADOW: &str = "/ws/cov/shadowed_awaited.ts";
    let host = host_with(&[(
        SHADOW,
        "type Awaited<X> = { w: X };\nexport async function shadowG1<T>(v: Awaited<T>) {\n  return v;\n}\n",
    )]);
    match eval(&host, SHADOW, "shadowG1") {
        Outcome::Value {
            ty,
            degradation: None,
            candidates: 1,
        } => assert!(
            shape_matches(
                &Shape::Ref("Promise", &[Shape::AuthoredAwaited(&Shape::T)]),
                &ty
            ),
            "the local alias keeps its identity: {ty:?}"
        ),
        other => panic!("shadowG1 must publish Promise<Awaited<T>> clean, got {other:?}"),
    }
}

/// Declaration carriers, constrained parameters and structural thenables for
/// [`awaited_thenable_protocol_oracle_matrix`].
const THENABLES: &str = "/ws/cov/thenables.ts";
const THENABLES_SRC: &str = r#"
interface AsyncGenerator<T, TReturn, TNext> {}
type Box<X> = { w: X };
interface IBox<X> { w: X }
type Plain = { p: string };
type P<X> = Promise<X>;
type ThenBox<X> = { then(onfulfilled: (value: X) => void): void };
type BadThen<X> = { then: number; w: X };
type BadThen2<X> = { then(): void; w: X };
type OptThen = { then?(onfulfilled: (value: number) => void): void };
export async function a1<T>(v: Box<T>) { return v; }
export async function a2<T>(v: IBox<T>) { return v; }
export async function a3(v: Plain) { return v; }
export async function a4<T>(v: P<T>) { return v; }
export async function a5<T>(v: ThenBox<T>) { return v; }
export async function a6(v: ThenBox<string>) { return v; }
export async function a7<T>(v: BadThen<T>) { return v; }
export async function a8<T>(v: BadThen2<T>) { return v; }
export async function w1<T>(v: Box<T>) { const a = await v; return { a }; }
export async function w5<T>(v: ThenBox<T>) { const a = await v; return { a }; }
export async function w6(v: ThenBox<string>) { const a = await v; return { a }; }
export async function w7<T>(v: BadThen<T>) { const a = await v; return { a }; }
export async function w8<T>(v: BadThen2<T>) { const a = await v; return { a }; }
export async function* y5(v: ThenBox<string>) { yield v; }
export async function c1<T extends {}>(v: T) { const a = await v; return { a }; }
export async function c2<T extends object>(v: T) { const a = await v; return { a }; }
export async function c3<T extends Plain>(v: T) { const a = await v; return { a }; }
export async function c4<T extends ThenBox<number>>(v: T) { const a = await v; return { a }; }
export async function c5<T extends unknown>(v: T) { const a = await v; return { a }; }
export async function c6<T extends { p: string }>(v: T) { const a = await v; return { a }; }
export async function c7<T extends number | Plain>(v: T) { const a = await v; return { a }; }
export async function o1(v: OptThen) { return v; }
export async function o2(v: OptThen) { const a = await v; return { a }; }
"#;

/// The checker's awaited-type protocol beyond the lib `Promise`, pinned as
/// rows: declaration carriers, constrained type parameters and structural
/// thenables, through both awaited relations (`a` / `o1` rows are the
/// async return payload, `w` / `c` / `o2` rows the await expression, `y5` the
/// async generator yield).
///
/// - A non-thenable declaration carrier is its own awaited type and KEEPS its
///   identity (`Promise<Box<T>>`, `Promise<Plain>`; a `then` that is not
///   callable is not a thenable, `a7` / `w7`).
/// - An alias to `Promise` unwraps (`a4`).
/// - A callable `then` whose `onfulfilled` is callable promises that
///   callback's first parameter, reduced by the SAME relation (`a5` is `T`,
///   `w5` is `Awaited<T>`).
/// - A callable `then` that promises nothing, and an optional callable `then`,
///   are errors the checker types `any` (`a8`, `w8`, `o1`, `o2`).
/// - An awaited type parameter stays `Awaited<T>` unless its constraint is
///   provably its own awaited type; `{}`, `object`, `unknown` and a thenable
///   constraint all defer (`c1`..`c7`).
///
/// Each `tsc` column is the declaration emit of the matching declaration,
/// measured on the repository's TypeScript 7.0.2 with
/// `tsc --ignoreConfig --declaration --emitDeclarationOnly --target es2022 --strict`.
#[test]
fn awaited_thenable_protocol_oracle_matrix() {
    const T: Shape = Shape::T;
    const PROMISE_T: Shape = Shape::Ref("Promise", &[Shape::T]);
    const PROMISE_A_T: Shape = Shape::Ref("Promise", &[Shape::Member("a", &Shape::T)]);
    const AWAITED_T: Shape = Shape::Awaited(&Shape::T);
    const PROMISE_A_AWAITED_T: Shape = Shape::Ref("Promise", &[Shape::Member("a", &AWAITED_T)]);
    const ANY: Shape = Shape::Primitive(PrimitiveName::Any);
    const STRING: Shape = Shape::Primitive(PrimitiveName::String);
    let clean: &[(&str, &str, Shape)] = &[
        (
            "a1",
            "Promise<Box<T>>",
            Shape::Ref("Promise", &[Shape::Ref("Box", &[T])]),
        ),
        (
            "a2",
            "Promise<IBox<T>>",
            Shape::Ref("Promise", &[Shape::Ref("IBox", &[T])]),
        ),
        (
            "a3",
            "Promise<Plain>",
            Shape::Ref("Promise", &[Shape::Ref("Plain", &[])]),
        ),
        ("a4", "Promise<T>", PROMISE_T),
        ("a5", "Promise<T>", PROMISE_T),
        ("a6", "Promise<string>", Shape::Ref("Promise", &[STRING])),
        (
            "a7",
            "Promise<BadThen<T>>",
            Shape::Ref("Promise", &[Shape::Ref("BadThen", &[T])]),
        ),
        ("a8", "Promise<any>", Shape::Ref("Promise", &[ANY])),
        (
            "w1",
            "Promise<{ a: Box<T>; }>",
            Shape::Ref("Promise", &[Shape::Member("a", &Shape::Ref("Box", &[T]))]),
        ),
        ("w5", "Promise<{ a: Awaited<T>; }>", PROMISE_A_AWAITED_T),
        (
            "w6",
            "Promise<{ a: string; }>",
            Shape::Ref("Promise", &[Shape::Member("a", &STRING)]),
        ),
        (
            "w7",
            "Promise<{ a: BadThen<T>; }>",
            Shape::Ref(
                "Promise",
                &[Shape::Member("a", &Shape::Ref("BadThen", &[T]))],
            ),
        ),
        (
            "w8",
            "Promise<{ a: any; }>",
            Shape::Ref("Promise", &[Shape::Member("a", &ANY)]),
        ),
        (
            "y5",
            "AsyncGenerator<string, void, unknown>",
            Shape::Ref(
                "AsyncGenerator",
                &[
                    STRING,
                    Shape::Primitive(PrimitiveName::Void),
                    Shape::Primitive(PrimitiveName::Unknown),
                ],
            ),
        ),
        ("c1", "Promise<{ a: Awaited<T>; }>", PROMISE_A_AWAITED_T),
        ("c2", "Promise<{ a: Awaited<T>; }>", PROMISE_A_AWAITED_T),
        ("c3", "Promise<{ a: T; }>", PROMISE_A_T),
        ("c4", "Promise<{ a: Awaited<T>; }>", PROMISE_A_AWAITED_T),
        ("c5", "Promise<{ a: Awaited<T>; }>", PROMISE_A_AWAITED_T),
        ("c6", "Promise<{ a: T; }>", PROMISE_A_T),
        ("c7", "Promise<{ a: T; }>", PROMISE_A_T),
        ("o1", "Promise<any>", Shape::Ref("Promise", &[ANY])),
        (
            "o2",
            "Promise<{ a: any; }>",
            Shape::Ref("Promise", &[Shape::Member("a", &ANY)]),
        ),
    ];
    let host = host_with(&[(THENABLES, THENABLES_SRC)]);
    let mut failures = Vec::new();
    for (function, tsc, shape) in clean {
        let outcome = eval(&host, THENABLES, function);
        let pinned = matches!(
            &outcome,
            Outcome::Value { ty, degradation: None, candidates: 1 } if shape_matches(shape, ty)
        );
        if !pinned {
            failures.push(format!(
                "{function}: tsc `{tsc}`, pinned {shape:?}, measured {outcome:?}"
            ));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// Authored lib `Awaited<X>` applications for
/// [`lib_awaited_conditional_oracle_matrix`].
const LIB_AWAITED: &str = "/ws/cov/lib_awaited.ts";
const LIB_AWAITED_SRC: &str = r#"
type ThenBox<X> = { then(onfulfilled: (value: X) => void): void };
type Plain = { p: string };
type C1<X> = (value: X) => void;
type C2<X> = C1<X>;
type C3<X> = C2<X>;
type C4<X> = C3<X>;
type DeepThen = { then(cb: C4<string>): void };
type RestThen = { then(...onfulfilled: ((value: number) => void)[]): void };
declare const l1: Awaited<{ then(): void }>; export function f1() { return l1; }
declare const l2: Awaited<{ then?(cb: (v: number) => void): void }>; export function f2() { return l2; }
declare const l3: Awaited<{ then: number }>; export function f3() { return l3; }
declare const l4: Awaited<ThenBox<string>>; export function f4() { return l4; }
declare const l5: Awaited<{ then(...cbs: ((v: number) => void)[]): void }>; export function f5() { return l5; }
declare const l6: Awaited<{ then(cb: number): void }>; export function f6() { return l6; }
declare const l7: Awaited<{ then(cb: ((v: number) => void) | undefined): void }>; export function f7() { return l7; }
declare const l8: Awaited<Plain>; export function f8() { return l8; }
declare const l9: Awaited<Promise<ThenBox<boolean>>>; export function f9() { return l9; }
declare const l10: Awaited<{ then(cb: () => void): void }>; export function f10() { return l10; }
declare const l11: Awaited<string | Promise<number>>; export function f11() { return l11; }
declare const l12: Awaited<{ then(cb: (...v: number[]) => void): void }>; export function f12() { return l12; }
declare const l13: Awaited<{ then(cb: (v: number) => void): void; then(cb: (v: string) => void, x: 1): void }>; export function f13() { return l13; }
declare const l14: Awaited<() => void>; export function f14() { return l14; }
declare const l15: Awaited<DeepThen>; export function f15() { return l15; }
export async function m1<T extends string>(v: Awaited<T>) { return v; }
export async function m2<T extends Promise<string>>(v: Awaited<T>) { return v; }
export async function m3(v: Awaited<{ then(): void }>) { return v; }
export async function m4<T>(v: Awaited<T>) { const a = await v; return { a }; }
export async function m5<T>(v: Awaited<Awaited<T>>) { return v; }
export async function m6(v: Awaited<{ then?(cb: (v: number) => void): void }>) { return v; }
export async function m7(v: Awaited<{ then(): void }>) { const a = await v; return { a }; }
export async function n1(v: RestThen) { return v; }
export async function d1(v: DeepThen) { return v; }
"#;

/// One oracle row: the fixture function, the tsc answer, and its pin.
type OracleRow<'a> = (&'a str, &'a str, &'a dyn Fn(&TypeExpr) -> bool);

/// Whether `ty` is an object carrying a `then` member (property or method).
fn has_then_member(ty: &TypeExpr) -> bool {
    let TypeExpr::Object(object) = ty else {
        return false;
    };
    object.properties.iter().any(|member| match member {
        verter_type_expr::ObjectMember::Property(p) => p.key.as_string() == Some("then"),
        verter_type_expr::ObjectMember::Method(m) => m.key.as_string() == Some("then"),
        _ => false,
    })
}

/// An authored `Awaited<X>` is the LIB conditional, not the compiler's
/// runtime awaited relation, and the two differ on malformed thenables.
///
/// The `f` rows read each authored application through the `Instantiate`
/// family (the declared const's flow return is the syntax-preserving carrier,
/// so the rows expand it explicitly):
///
/// - a callable `then` with no callable `onfulfilled` is `never` (`f1`, `f6`)
///   — where `await` of the same value is `any` (`w8` of
///   `awaited_thenable_protocol_oracle_matrix`);
/// - an OPTIONAL callable `then` does not satisfy the required member, so the
///   operand is its own result (`f2`), as is a non-callable `then` (`f3`);
/// - a leading rest `then` parameter and a rest callback parameter read their
///   element (`f5`, `f12`); a parameterless callback infers `unknown` (`f10`);
/// - an overloaded `then` infers from its LAST signature (`f13`, `string`),
///   where the runtime relation unions every signature;
/// - unions distribute (`f11`), promises and thenables nest (`f9`), a
///   non-thenable alias keeps its identity (`f8`), and a callback reached
///   through a four-level alias chain settles through the shared signature
///   rail (`f15`).
///
/// The `m` rows are the async positions over authored applications: a
/// deferred `Awaited<T>` strips at the return payload whatever the
/// constraint (`m1`, `m2`, twice for `m5`); a reduced one is the payload's
/// operand (`m3` `never`, `m6` the optional-`then` surface the runtime
/// relation calls `any`); an await keeps a deferred application (`m4`) and
/// awaits a reduced one (`m7`). `n1` / `d1` are the runtime relation over a
/// rest `then` and a deep callback alias chain.
///
/// Every `tsc` column is TypeScript 7.0.2: the `f` rows from assignability
/// diagnostics (`tsc --noEmit --strict`), the rest from declaration emit
/// (`--declaration --emitDeclarationOnly --target es2022 --strict`).
#[test]
fn lib_awaited_conditional_oracle_matrix() {
    let host = host_with(&[(LIB_AWAITED, LIB_AWAITED_SRC)]);
    let mut failures = Vec::new();

    let lib_rows: &[OracleRow<'_>] = &[
        ("f1", "never", &|ty| {
            *ty == TypeExpr::Primitive(PrimitiveName::Never)
        }),
        (
            "f2",
            "{ then?(cb: (v: number) => void): void; }",
            &has_then_member,
        ),
        ("f3", "{ then: number; }", &has_then_member),
        ("f4", "string", &|ty| *ty == string()),
        ("f5", "number", &|ty| *ty == number()),
        ("f6", "never", &|ty| {
            *ty == TypeExpr::Primitive(PrimitiveName::Never)
        }),
        ("f7", "number", &|ty| *ty == number()),
        ("f8", "Plain", &|ty| {
            shape_matches(&Shape::Ref("Plain", &[]), ty)
        }),
        ("f9", "boolean", &|ty| *ty == boolean()),
        ("f10", "unknown", &|ty| {
            *ty == TypeExpr::Primitive(PrimitiveName::Unknown)
        }),
        ("f11", "string | number", &|ty| match ty {
            TypeExpr::Union(members) => {
                members.len() == 2 && members.contains(&string()) && members.contains(&number())
            }
            _ => false,
        }),
        ("f12", "number", &|ty| *ty == number()),
        ("f13", "string", &|ty| *ty == string()),
        ("f14", "() => void", &|ty| {
            matches!(ty, TypeExpr::Function(_))
        }),
        ("f15", "string", &|ty| *ty == string()),
    ];
    with_dispatch(&host, |dispatch| {
        for (function, tsc, pinned) in lib_rows {
            let key = key_of(dispatch, LIB_AWAITED, function);
            let measured = match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key))) {
                QueryResult::Value(SemanticQueryOutput {
                    value: SemanticQueryValue::FlowReturn(result),
                    ..
                }) => dispatch
                    .declaration_carrier_body(result.return_type())
                    .and_then(|body| host.project_node_to_type_expr_for_test(body)),
                _ => None,
            };
            if !measured.as_ref().is_some_and(pinned) {
                failures.push(format!("{function}: tsc `{tsc}`, measured {measured:?}"));
            }
        }
    });

    const PROMISE_T: Shape = Shape::Ref("Promise", &[Shape::T]);
    let async_rows: &[OracleRow<'_>] = &[
        ("m1", "Promise<T>", &|ty| shape_matches(&PROMISE_T, ty)),
        ("m2", "Promise<T>", &|ty| shape_matches(&PROMISE_T, ty)),
        ("m3", "Promise<never>", &|ty| {
            shape_matches(
                &Shape::Ref("Promise", &[Shape::Primitive(PrimitiveName::Never)]),
                ty,
            )
        }),
        ("m4", "Promise<{ a: Awaited<T>; }>", &|ty| {
            shape_matches(
                &Shape::Ref(
                    "Promise",
                    &[Shape::Member("a", &Shape::AuthoredAwaited(&Shape::T))],
                ),
                ty,
            )
        }),
        ("m5", "Promise<T>", &|ty| shape_matches(&PROMISE_T, ty)),
        ("m6", "Promise<any>", &|ty| {
            shape_matches(
                &Shape::Ref("Promise", &[Shape::Primitive(PrimitiveName::Any)]),
                ty,
            )
        }),
        ("m7", "Promise<{ a: never; }>", &|ty| {
            shape_matches(
                &Shape::Ref(
                    "Promise",
                    &[Shape::Member("a", &Shape::Primitive(PrimitiveName::Never))],
                ),
                ty,
            )
        }),
        ("n1", "Promise<number>", &|ty| {
            shape_matches(
                &Shape::Ref("Promise", &[Shape::Primitive(PrimitiveName::Number)]),
                ty,
            )
        }),
        ("d1", "Promise<string>", &|ty| {
            shape_matches(
                &Shape::Ref("Promise", &[Shape::Primitive(PrimitiveName::String)]),
                ty,
            )
        }),
    ];
    for (function, tsc, pinned) in async_rows {
        let outcome = eval(&host, LIB_AWAITED, function);
        let ok = matches!(
            &outcome,
            Outcome::Value { ty, degradation: None, candidates: 1 } if pinned(ty)
        );
        if !ok {
            failures.push(format!("{function}: tsc `{tsc}`, measured {outcome:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// `any` / `never` / `unknown` operands and non-signature callback
/// representations for [`awaited_callback_edge_oracle_matrix`].
const AWAITED_EDGES: &str = "/ws/cov/awaited_edges.ts";
const AWAITED_EDGES_SRC: &str = r#"
declare const l1: Awaited<{ then(onfulfilled: any): void }>; export function f1() { return l1; }
declare const l2: Awaited<{ then(onfulfilled: ((v: number) => void) & ((v: string) => void)): void }>; export function f2() { return l2; }
declare const l3: Awaited<{ then(onfulfilled: unknown): void }>; export function f3() { return l3; }
declare const l4: Awaited<{ then(onfulfilled: never): void }>; export function f4() { return l4; }
declare const l5: Awaited<{ then: any }>; export function f5() { return l5; }
declare const l6: Awaited<{ then(onfulfilled: (v: any) => void): void }>; export function f6() { return l6; }
declare const l7: Awaited<{ then(onfulfilled: Function): void }>; export function f7() { return l7; }
declare const l8: Awaited<{ then(onfulfilled: ((v: number) => void) | ((v: string) => void)): void }>; export function f8() { return l8; }
declare const l9: Awaited<{ then(onfulfilled: object): void }>; export function f9() { return l9; }
declare const la: Awaited<{ then: unknown }>; export function fa() { return la; }
declare const lb: Awaited<{ then: never }>; export function fb() { return lb; }
declare const lc: Awaited<{ then(cb: ((v: number) => void) & { x: 1 }): void }>; export function fc() { return lc; }
declare const ld: Awaited<{ then(cb: { x: 1 } & ((v: number) => void)): void }>; export function fd() { return ld; }
declare const le: Awaited<{ then(cb: Date): void }>; export function fe() { return le; }
declare const lf: Awaited<{ then(onfulfilled: any, x: number): void }>; export function ff() { return lf; }
export async function r1(v: { then(onfulfilled: any): void }) { return v; }
export async function r2(v: { then(onfulfilled: ((v: number) => void) & ((v: string) => void)): void }) { return v; }
export async function r3(v: { then(onfulfilled: unknown): void }) { return v; }
export async function r5(v: { then: any }) { return v; }
export async function r7(v: { then(onfulfilled: Function): void }) { return v; }
export async function r8(v: { then(onfulfilled: ((v: number) => void) | ((v: string) => void)): void }) { return v; }
export async function s1(v: { then: unknown }) { return v; }
export async function s2(v: { then: never }) { return v; }
export async function s3(v: { then(cb: ((v: number) => void) & { x: 1 }): void }) { return v; }
export async function s4(v: { then(cb: Date): void }) { return v; }
"#;

/// The awaited relations over `any` / `never` / `unknown` and over callbacks
/// that are not a single signature, pinned against TypeScript 7.0.2.
///
/// The lib conditional (`f` rows, read through `Instantiate`):
///
/// - `onfulfilled: any` takes BOTH branches of
///   `F extends (value: infer V, ...) => any` with `V` inferred as `unknown`,
///   so the result is `Awaited<unknown>` = `unknown` (`f1`, `ff`) — never the
///   `never` a "non-callable" reading of `any` would give;
/// - a `then` typed `any` or `never` infers no `onfulfilled` and is `never`
///   (`f5`, `fb`), while `then: unknown` does not match and leaves the operand
///   (`fa`);
/// - an intersection callback reads its LAST call signature, whatever arm
///   order or non-callable arms it has (`f2`, `fc`, `fd`); a union callback
///   distributes (`f8`);
/// - an `unknown` / `never` / `object` / `Function` / `Date` callback has no
///   call signature and is `never` (`f3`, `f4`, `f9`, `f7`, `fe`); the lib
///   nominals are decided by declaration identity.
///
/// The runtime relation (`r` / `s` rows, the async return payload):
/// `onfulfilled` of `any` / `unknown` / `Function` / `Date` is the checker's
/// malformed-thenable `any` (`r1`, `r3`, `r7`, `s4`); an intersection callback
/// unions EVERY signature (`r2` is `string | number`); a `then` of `any` /
/// `unknown` / `never` is not a thenable (`r5`, `s1`, `s2`).
///
/// `r8` is the honest-refusal leg: the checker merges a union of callbacks
/// into one signature with intersected parameters (`number & string`, so
/// `Promise<never>`); this reader does not model union-signature merging and
/// publishes the typed gap.
///
/// `f` columns come from assignability diagnostics with an `any`/`never`
/// discriminating probe (`tsc --noEmit --strict`), the rest from declaration
/// emit (`--declaration --emitDeclarationOnly --target es2022 --strict`).
#[test]
fn awaited_callback_edge_oracle_matrix() {
    let host = host_with(&[(AWAITED_EDGES, AWAITED_EDGES_SRC)]);
    let never = TypeExpr::Primitive(PrimitiveName::Never);
    let unknown = TypeExpr::Primitive(PrimitiveName::Unknown);
    let any = TypeExpr::Primitive(PrimitiveName::Any);
    let is = |expected: TypeExpr| move |ty: &TypeExpr| *ty == expected;
    let string_or_number = |ty: &TypeExpr| match ty {
        TypeExpr::Union(members) => {
            members.len() == 2 && members.contains(&string()) && members.contains(&number())
        }
        _ => false,
    };
    let mut failures = Vec::new();

    let f1 = is(unknown.clone());
    let f2 = is(string());
    let f3 = is(never.clone());
    let f4 = is(never.clone());
    let f5 = is(never.clone());
    let f6 = is(any.clone());
    let f7 = is(never.clone());
    let f9 = is(never.clone());
    let fb = is(never.clone());
    let fc = is(number());
    let fd = is(number());
    let fe = is(never.clone());
    let ff = is(unknown.clone());
    let lib_rows: &[OracleRow<'_>] = &[
        ("f1", "unknown", &f1),
        ("f2", "string", &f2),
        ("f3", "never", &f3),
        ("f4", "never", &f4),
        ("f5", "never", &f5),
        ("f6", "any", &f6),
        ("f7", "never", &f7),
        ("f8", "string | number", &string_or_number),
        ("f9", "never", &f9),
        ("fa", "{ then: unknown; }", &has_then_member),
        ("fb", "never", &fb),
        ("fc", "number", &fc),
        ("fd", "number", &fd),
        ("fe", "never", &fe),
        ("ff", "unknown", &ff),
    ];
    with_dispatch(&host, |dispatch| {
        for (function, tsc, pinned) in lib_rows {
            let key = key_of(dispatch, AWAITED_EDGES, function);
            let measured = match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key))) {
                QueryResult::Value(SemanticQueryOutput {
                    value: SemanticQueryValue::FlowReturn(result),
                    ..
                }) => dispatch
                    .declaration_carrier_body(result.return_type())
                    .and_then(|body| host.project_node_to_type_expr_for_test(body)),
                _ => None,
            };
            if !measured.as_ref().is_some_and(pinned) {
                failures.push(format!("{function}: tsc `{tsc}`, measured {measured:?}"));
            }
        }
    });

    let promise_of = |inner: TypeExpr| {
        move |ty: &TypeExpr| {
            *ty == TypeExpr::Ref {
                name: Arc::from("Promise"),
                type_arguments: Arc::from(vec![inner.clone()].into_boxed_slice()),
            }
        }
    };
    let promise_of_then_surface = |ty: &TypeExpr| match ty {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            name.as_ref() == "Promise"
                && type_arguments.len() == 1
                && has_then_member(&type_arguments[0])
        }
        _ => false,
    };
    let promise_string_or_number = |ty: &TypeExpr| match ty {
        TypeExpr::Ref {
            name,
            type_arguments,
        } => {
            name.as_ref() == "Promise"
                && type_arguments.len() == 1
                && string_or_number(&type_arguments[0])
        }
        _ => false,
    };
    let r1 = promise_of(any.clone());
    let r3 = promise_of(any.clone());
    let r7 = promise_of(any.clone());
    let s3 = promise_of(number());
    let s4 = promise_of(any.clone());
    let runtime_rows: &[OracleRow<'_>] = &[
        ("r1", "Promise<any>", &r1),
        ("r2", "Promise<string | number>", &promise_string_or_number),
        ("r3", "Promise<any>", &r3),
        ("r5", "Promise<{ then: any; }>", &promise_of_then_surface),
        ("r7", "Promise<any>", &r7),
        (
            "s1",
            "Promise<{ then: unknown; }>",
            &promise_of_then_surface,
        ),
        ("s2", "Promise<{ then: never; }>", &promise_of_then_surface),
        ("s3", "Promise<number>", &s3),
        ("s4", "Promise<any>", &s4),
    ];
    for (function, tsc, pinned) in runtime_rows {
        let outcome = eval(&host, AWAITED_EDGES, function);
        let ok = matches!(
            &outcome,
            Outcome::Value { ty, degradation: None, candidates: 1 } if pinned(ty)
        );
        if !ok {
            failures.push(format!("{function}: tsc `{tsc}`, measured {outcome:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
    assert_degraded(
        &host,
        AWAITED_EDGES,
        "r8",
        FlowReturnDegradation::UnresolvedValue,
    );
}

/// A consumer whose callbacks name the lib runtime nominals `Function` and
/// `Date`, for [`augmented_global_nominal_callbacks_follow_the_merged_interface`].
const AUGMENT_CONSUMER: &str = "/ws/cov/augment_consumer.ts";
const AUGMENT_CONSUMER_SRC: &str = r#"
declare const la: Awaited<{ then(onfulfilled: Function): void }>; export function fa() { return la; }
declare const lb: Awaited<{ then(onfulfilled: Date): void }>; export function fb() { return lb; }
declare const lc: Awaited<{ then(onfulfilled: ((v: string) => void) & ((v: number) => void)): void }>; export function fc() { return lc; }
export async function r1(v: { then(onfulfilled: Function): void }) { return v; }
export async function r2(v: { then(onfulfilled: Date): void }) { return v; }
export async function r3(v: { then(onfulfilled: ((v: string) => void) & ((v: number) => void)): void }) { return v; }
"#;
/// A separate `declare global` augmenter merging call signatures into both.
const AUGMENT_GLOBAL: &str = "/ws/cov/augment_global.d.ts";
const AUGMENT_GLOBAL_SRC: &str = r#"
export {};
declare global {
  interface Function { (value: number): void; }
  interface Date { (value: string): void; }
}
"#;

/// The lib runtime nominals are OPEN interfaces: a pristine `Function` or
/// `Date` declares no call signature, but a project's `declare global` block
/// merges them in, and both awaited relations must see the merged interface.
///
/// Oracle (tsc 7.0.2, the augmentation in scope): `Awaited<{ then(onfulfilled:
/// Function): void }>` is `number` and `Date` gives `string` (assignability
/// diagnostics); the async return over the same `then` publishes
/// `Promise<number>` / `Promise<string>` (declaration emit). Without the
/// augmentation the lib results are `never` and the async returns `Promise<any>`
/// (`awaited_callback_edge_oracle_matrix` rows `f7`, `fe`, `r7`, `s4`).
///
/// The rows are measured twice on ONE host: pristine first, then after the
/// augmenter enters the program — the second read must not serve the pristine
/// answer, which is what the augmenter-set fact the shared augmentation folder
/// observes guarantees. A fresh cold read discovers an augmenter nothing
/// imports through program-completeness indexing.
///
/// `fc` / `r3` pin intersection ORDER: `((v: string) => void) & ((v: number)
/// => void)` is `number` for the lib conditional (last signature) and
/// `string | number` for the await relation — the reverse of `f2` / `r2`.
#[test]
fn augmented_global_nominal_callbacks_follow_the_merged_interface() {
    let host = host_with(&[(AUGMENT_CONSUMER, AUGMENT_CONSUMER_SRC)]);
    let lib = |function: &str| {
        with_dispatch(&host, |dispatch| {
            let key = key_of(dispatch, AUGMENT_CONSUMER, function);
            match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key))) {
                QueryResult::Value(SemanticQueryOutput {
                    value: SemanticQueryValue::FlowReturn(result),
                    ..
                }) => dispatch
                    .declaration_carrier_body(result.return_type())
                    .and_then(|body| host.project_node_to_type_expr_for_test(body)),
                _ => None,
            }
        })
    };
    let promise_of = |inner: TypeExpr| TypeExpr::Ref {
        name: Arc::from("Promise"),
        type_arguments: Arc::from(vec![inner].into_boxed_slice()),
    };
    let never = TypeExpr::Primitive(PrimitiveName::Never);
    let any = TypeExpr::Primitive(PrimitiveName::Any);
    // Clean (no degradation) and admitted. After the augmenter enters, the
    // slot legitimately holds the pristine candidate beside the merged one.
    let clean = |function: &str| match eval(&host, AUGMENT_CONSUMER, function) {
        Outcome::Value {
            ty,
            degradation: None,
            candidates,
        } if candidates >= 1 => Some(ty),
        _ => None,
    };

    // Pristine lib interfaces: no call signature.
    assert_eq!(lib("fa"), Some(never.clone()), "pristine Function");
    assert_eq!(lib("fb"), Some(never), "pristine Date");
    assert_eq!(
        clean("r1"),
        Some(promise_of(any.clone())),
        "pristine Function"
    );
    assert_eq!(clean("r2"), Some(promise_of(any)), "pristine Date");

    let _ = host.upsert(UpsertRequest {
        canonical_id: Some(AUGMENT_GLOBAL.to_string()),
        input_id: AUGMENT_GLOBAL.to_string(),
        source: Arc::from(AUGMENT_GLOBAL_SRC),
        file_language: lang(AUGMENT_GLOBAL),
        aliases: Vec::new(),
    });
    // Augmentation discovery is demand-driven: the augmenter enters the program
    // when its artifact publishes, which is what retires the index rows the
    // pristine read recorded.
    host.ensure_indexed_ready(AUGMENT_GLOBAL)
        .expect("the augmenter must index");

    // Augmented: the merged call signatures decide.
    assert_eq!(lib("fa"), Some(number()), "augmented Function");
    assert_eq!(lib("fb"), Some(string()), "augmented Date");
    assert_eq!(
        clean("r1"),
        Some(promise_of(number())),
        "augmented Function"
    );
    assert_eq!(clean("r2"), Some(promise_of(string())), "augmented Date");

    // Intersection order, reversed from `f2` / `r2`.
    assert_eq!(lib("fc"), Some(number()), "last intersection signature");
    let r3 = clean("r3").expect("r3 publishes clean");
    let TypeExpr::Ref { type_arguments, .. } = &r3 else {
        panic!("r3 must be Promise<…>, got {r3:?}");
    };
    assert!(
        matches!(
            &type_arguments[0],
            TypeExpr::Union(members)
                if members.len() == 2 && members.contains(&string()) && members.contains(&number())
        ),
        "the await relation unions every intersection signature: {r3:?}"
    );
}

/// A `declare global` augmenter that nothing imports, adding GENERIC call
/// signatures to `Promise<T>` and `Map<K, V>` and a plain one to `Function`,
/// for [`augmented_generic_nominal_callbacks_bind_their_type_arguments`].
const GENERIC_GLOBAL: &str = "/ws/cov/generic_global.d.ts";
const GENERIC_GLOBAL_SRC: &str = r#"export {};
declare global {
  interface Promise<T> { (value: T): void; }
  interface Map<K, V> { (value: V): void; }
  interface Function { (value: boolean): void; }
}
"#;
const GENERIC_CONSUMER: &str = "/ws/cov/generic_consumer.ts";
const GENERIC_CONSUMER_SRC: &str = r#"
declare const la: Awaited<{ then(onfulfilled: Promise<number>): void }>; export function fa() { return la; }
declare const lb: Awaited<{ then(onfulfilled: Map<string, number>): void }>; export function fb() { return lb; }
declare const lc: Awaited<{ then(onfulfilled: Promise<string>): void }>; export function fc() { return lc; }
declare const ld: Awaited<{ then(onfulfilled: Function): void }>; export function fd() { return ld; }
export async function r1(v: { then(onfulfilled: Promise<number>): void }) { return v; }
export async function r2(v: { then(onfulfilled: Map<string, number>): void }) { return v; }
"#;

/// A GENERIC lib nominal's augmentation is instantiated with the nominal's
/// own type arguments, and an augmenter nothing imports is discovered cold.
///
/// Oracle (tsc 7.0.2, the augmentation in scope): `Awaited<{ then(onfulfilled:
/// Promise<number>): void }>` is `number`, `Map<string, number>` gives
/// `number` (the SECOND parameter, so the binding is positional, not
/// first-argument), and `Promise<string>` gives `string` (the argument, not a
/// fixed answer); the async returns over the first two are `Promise<number>`.
///
/// The host only UPSERTS both files — the augmenter is never explicitly
/// indexed, so every row goes through the production program-completeness
/// scan that indexes known members before the augmentation lookup. `fd` pins
/// the non-generic cold path.
///
/// Discriminating: collecting the augmentation without its type arguments
/// lowers `T` / `V` unbound and every row degrades to a typed gap.
#[test]
fn augmented_generic_nominal_callbacks_bind_their_type_arguments() {
    let host = host_with(&[
        (GENERIC_GLOBAL, GENERIC_GLOBAL_SRC),
        (GENERIC_CONSUMER, GENERIC_CONSUMER_SRC),
    ]);
    let lib = |function: &str| {
        with_dispatch(&host, |dispatch| {
            let key = key_of(dispatch, GENERIC_CONSUMER, function);
            match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key))) {
                QueryResult::Value(SemanticQueryOutput {
                    value: SemanticQueryValue::FlowReturn(result),
                    ..
                }) => dispatch
                    .declaration_carrier_body(result.return_type())
                    .and_then(|body| host.project_node_to_type_expr_for_test(body)),
                _ => None,
            }
        })
    };
    assert_eq!(lib("fa"), Some(number()), "Promise<number>");
    assert_eq!(lib("fb"), Some(number()), "Map<string, number>");
    assert_eq!(lib("fc"), Some(string()), "Promise<string>");
    assert_eq!(lib("fd"), Some(boolean()), "Function");
    let promise_number = TypeExpr::Ref {
        name: Arc::from("Promise"),
        type_arguments: Arc::from(vec![number()].into_boxed_slice()),
    };
    assert_clean_warm(&host, GENERIC_CONSUMER, "r1", promise_number.clone());
    assert_clean_warm(&host, GENERIC_CONSUMER, "r2", promise_number);
}

/// Receiver-bearing and union-valued `then` members for
/// [`awaited_receiver_and_union_then_oracle_matrix`].
const RECEIVER_THENS: &str = "/ws/cov/receiver_thens.ts";
const RECEIVER_THENS_SRC: &str = r#"
type RecvOk = { x: 1; then(this: { x: 1 }, onfulfilled: (value: number) => void): void };
type RecvBad = { y: 2; then(this: { x: 1 }, onfulfilled: (value: number) => void): void };
type UnionThen = { then: ((cb: (v: number) => void) => void) | ((cb: (v: string) => void) => void) };
declare const l1: Awaited<RecvOk>; export function f1() { return l1; }
declare const l2: Awaited<RecvBad>; export function f2() { return l2; }
declare const l3: Awaited<UnionThen>; export function f3() { return l3; }
export async function r1(v: RecvOk) { return v; }
export async function r2(v: RecvBad) { return v; }
export async function r3(v: UnionThen) { return v; }
"#;

/// A `then` signature's declared `this` and a union-valued `then` member,
/// through both awaited relations.
///
/// The RUNTIME relation keeps only the `then` signatures whose declared
/// `this` the operand is assignable to, and a `then` all of whose signatures
/// are rejected that way promises nothing: the checker reports
/// "The 'this' context of type 'RecvBad' is not assignable to method's
/// 'this' of type '{ x: 1; }'" (TS1058) and types the await `any` (`r2`).
/// A compatible receiver adopts normally (`r1`), and the receiver is never
/// the callback: reading position 0 as the `this` parameter would promise
/// `{ x: 1 }` instead of `number`.
///
/// The LIB conditional does NOT model receiver eligibility at all: both `f1`
/// and `f2` are `number`. That divergence is the point of keeping the two
/// readers distinct.
///
/// A union-valued `then` decides in BOTH: the runtime relation reads the
/// union's synthesized signature, whose `onfulfilled` intersects the arms,
/// so every arm's callback contributes (`r3` is `string | number`); the lib
/// conditional infers `F` per union constituent and the distributed arm step
/// unions their results (`f3` is `string | number`).
///
/// Every `tsc` column is TypeScript 7.0.2: the `f` rows from declaration
/// emit of the reading function, the `r` rows from the async declaration
/// emit (`tsc --ignoreConfig --declaration --emitDeclarationOnly
/// --target es2022 --strict`).
#[test]
fn awaited_receiver_and_union_then_oracle_matrix() {
    let host = host_with(&[(RECEIVER_THENS, RECEIVER_THENS_SRC)]);
    let mut failures = Vec::new();

    let lib_rows: &[(&str, &str, TypeExpr)] = &[
        ("f1", "number", number()),
        ("f2", "number", number()),
        (
            "f3",
            "string | number",
            TypeExpr::Union(Arc::from(vec![string(), number()].into_boxed_slice())),
        ),
    ];
    with_dispatch(&host, |dispatch| {
        for (function, tsc, expected) in lib_rows {
            let key = key_of(dispatch, RECEIVER_THENS, function);
            let measured = match dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key))) {
                QueryResult::Value(SemanticQueryOutput {
                    value: SemanticQueryValue::FlowReturn(result),
                    ..
                }) => dispatch
                    .declaration_carrier_body(result.return_type())
                    .and_then(|body| host.project_node_to_type_expr_for_test(body)),
                _ => None,
            };
            let matched = match (&measured, expected) {
                (Some(TypeExpr::Union(members)), TypeExpr::Union(wanted)) => {
                    members.len() == wanted.len() && wanted.iter().all(|arm| members.contains(arm))
                }
                (Some(ty), wanted) => ty == wanted,
                (None, _) => false,
            };
            if !matched {
                failures.push(format!("{function}: tsc `{tsc}`, measured {measured:?}"));
            }
        }
    });

    let runtime_rows: &[OracleRow<'_>] = &[
        ("r1", "Promise<number>", &|ty: &TypeExpr| {
            *ty == TypeExpr::Ref {
                name: Arc::from("Promise"),
                type_arguments: Arc::from(vec![number()].into_boxed_slice()),
            }
        }),
        ("r2", "Promise<any>", &|ty: &TypeExpr| {
            *ty == TypeExpr::Ref {
                name: Arc::from("Promise"),
                type_arguments: Arc::from(
                    vec![TypeExpr::Primitive(PrimitiveName::Any)].into_boxed_slice(),
                ),
            }
        }),
        (
            "r3",
            "Promise<string | number>",
            &|ty: &TypeExpr| match ty {
                TypeExpr::Ref {
                    name,
                    type_arguments,
                } => {
                    name.as_ref() == "Promise"
                        && type_arguments.len() == 1
                        && matches!(
                            &type_arguments[0],
                            TypeExpr::Union(members)
                                if members.len() == 2
                                    && members.contains(&string())
                                    && members.contains(&number())
                        )
                }
                _ => false,
            },
        ),
    ];
    for (function, tsc, pinned) in runtime_rows {
        let outcome = eval(&host, RECEIVER_THENS, function);
        let ok = matches!(
            &outcome,
            Outcome::Value { ty, degradation: None, candidates: 1 } if pinned(ty)
        );
        if !ok {
            failures.push(format!("{function}: tsc `{tsc}`, measured {outcome:?}"));
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The file declaring a cross-file thenable's `then`, for
/// [`awaited_result_roots_on_the_file_declaring_then`].
const ROOT_THENABLE: &str = "/ws/cov/root_thenable.ts";
const ROOT_THENABLE_NUMBER: &str =
    "export type RootThenable = { then(onfulfilled: (value: number) => void): void };\n";
const ROOT_THENABLE_STRING: &str =
    "export type RootThenable = { then(onfulfilled: (value: string) => void): void };\n";
/// A file the awaited answer does not depend on at all.
const ROOT_UNRELATED: &str = "/ws/cov/root_unrelated.ts";
const ROOT_UNRELATED_SRC: &str = "export const unrelated = 1;\n";
const ROOT_CONSUMER: &str = "/ws/cov/root_consumer.ts";
const ROOT_CONSUMER_SRC: &str = r#"
import type { RootThenable } from "./root_thenable";
export async function rootAwait(v: RootThenable) { return v; }
"#;

/// An awaited result follows an edit to the file declaring the thenable's
/// `then`, and ignores an edit to a file it never read.
///
/// The operand of `rootAwait` is a carrier written in the CONSUMER, while
/// the promised value is read out of a `then` declared in another file. The
/// positive leg pins that the stale promised value is never served; the
/// negative leg pins the other half, that validity stays PRECISE — an edit
/// to a file the answer never read adds ZERO awaited-relation builds on the
/// next demand, so nothing here degenerates into blanket invalidation.
///
/// Oracle (tsc 7.0.2, declaration emit): `Promise<number>` before the edit
/// and `Promise<string>` after `then`'s callback parameter changes.
#[test]
fn awaited_result_follows_a_cross_file_then_edit_and_ignores_unrelated_ones() {
    let host = host_with(&[
        (ROOT_THENABLE, ROOT_THENABLE_NUMBER),
        (ROOT_UNRELATED, ROOT_UNRELATED_SRC),
        (ROOT_CONSUMER, ROOT_CONSUMER_SRC),
    ]);
    let promise_of = |inner: TypeExpr| TypeExpr::Ref {
        name: Arc::from("Promise"),
        type_arguments: Arc::from(vec![inner].into_boxed_slice()),
    };
    let upsert = |canonical: &str, source: &str| {
        let _ = host.upsert(UpsertRequest {
            canonical_id: Some(canonical.to_string()),
            input_id: canonical.to_string(),
            source: Arc::from(source),
            file_language: lang(canonical),
            aliases: Vec::new(),
        });
        host.ensure_indexed_ready(canonical)
            .expect("the edited file must index");
    };
    let payload_builds = || {
        host.project_type_store()
            .semantic_graph()
            .stats_snapshot()
            .async_return_payload_count
    };

    assert_clean_warm(&host, ROOT_CONSUMER, "rootAwait", promise_of(number()));

    // An unrelated edit: the answer never read that file, so the warm entry
    // stays valid and the repeat demand pays no awaited-relation build.
    upsert(ROOT_UNRELATED, "export const unrelated = 2;\n");
    let before_unrelated = payload_builds();
    assert_clean_warm(&host, ROOT_CONSUMER, "rootAwait", promise_of(number()));
    assert_eq!(
        payload_builds(),
        before_unrelated,
        "an edit to a file the awaited answer never read must not rebuild it"
    );

    // The declaring file's edit: the stale promised value must not be served.
    // After the edit the slot legitimately holds the pre-edit candidate
    // beside the new one, so the assertion is on the served value and its
    // cleanliness, not on the candidate count.
    upsert(ROOT_THENABLE, ROOT_THENABLE_STRING);
    let after = eval(&host, ROOT_CONSUMER, "rootAwait");
    assert!(
        matches!(
            &after,
            Outcome::Value { ty, degradation: None, candidates }
                if *ty == promise_of(string()) && *candidates >= 1
        ),
        "an edit to the file declaring `then` must retire the stale promised \
         value, got {after:?}"
    );
}

/// BOUNDED WORK on the ASYNC RETURN PAYLOAD relation: one build per
/// publication level, and identical warm demand adds ZERO.
///
/// `X18_async_return`'s shape — `async function makeProps() { return { label:
/// "x" } }` — publishes `Promise<{ label: string }>`, so the cold demand runs
/// the payload relation exactly once over the joined object return. The
/// counter is `async_return_payload_count`, deliberately separate from
/// `awaited_normalize_count`: the two relations are distinct families (a
/// naked type parameter is its own payload but stays `Awaited<T>` under
/// normalization), and only a per-family counter can show that the async
/// wrap did not silently route through the other one.
///
/// The load-bearing half is the second assertion: a warm family hit returns
/// before the builder runs, so zero is observable only through this counter.
#[test]
fn async_return_payload_demand_is_bounded_cold_and_zero_warm() {
    let host = ts_host();
    let payload_builds = || {
        host.project_type_store()
            .semantic_graph()
            .stats_snapshot()
            .async_return_payload_count
    };
    let normalize_builds = || {
        host.project_type_store()
            .semantic_graph()
            .stats_snapshot()
            .awaited_normalize_count
    };
    let expected = TypeExpr::Ref {
        name: Arc::from("Promise"),
        type_arguments: Arc::from(vec![number()].into_boxed_slice()),
    };

    let before_payload = payload_builds();
    let before_normalize = normalize_builds();
    assert_clean_warm(&host, CALLS, "callAsyncPlain", expected.clone());
    let cold_payload = payload_builds();
    assert_eq!(
        cold_payload - before_payload,
        1,
        "the cold async wrap pays exactly one async-return-payload build"
    );
    assert_eq!(
        normalize_builds(),
        before_normalize,
        "an async wrap with no await must not enter the normalization family"
    );

    // bounded-loop: three warm repeats, a fixed demand count
    for _ in 0..3 {
        assert_clean_warm(&host, CALLS, "callAsyncPlain", expected.clone());
    }
    assert_eq!(
        payload_builds(),
        cold_payload,
        "identical warm demand must add ZERO async-return-payload builds"
    );
}

// ──────────────────────────────────────────────────────────────────────
// Recursive thenables: the checker's TS1062 / TS2589 diagnostics and the
// error type it continues with
// ──────────────────────────────────────────────────────────────────────

/// Recursive thenables in every Awaited position, beside a non-recursive
/// thenable and a non-thenable cycle that must keep their ordinary answers.
/// Every expected answer below is measured on tsc 7.0.2 over this exact
/// module (`--noEmit --strict --target es2022`, each type read through the
/// tuple wrapper `null as any as [ReturnType<typeof f>]` quoted by TS2322).
const RECURSIVE_THENABLE: &str = "/ws/cov/recursive_thenable.ts";
const RECURSIVE_THENABLE_SRC: &str = r#"
interface Rec { then(onfulfilled: (v: Rec) => void): void }
interface PingThen { then(onfulfilled: (v: PongThen) => void): void }
interface PongThen { then(onfulfilled: (v: PingThen) => void): void }
interface SelfOrText { then(onfulfilled: (v: SelfOrText | string) => void): void }
interface NumberThen { then(onfulfilled: (v: number) => void): void }
interface Chain { next: Chain; v: number }
interface Linked { then(onfulfilled: (v: { next: Linked }) => void): void }
interface Holder { self: Holder; box: { then(onfulfilled: (v: Holder) => void): void } }
type AwaitedOf<T> = Awaited<T>;
export async function awaitRec(r: Rec) { const value = await r; return value; }
export async function returnRec(r: Rec) { return r; }
export async function awaitPromiseRec(p: Promise<Rec>) { const value = await p; return value; }
export async function returnPromiseRec(p: Promise<Rec>) { return p; }
export async function awaitRecOrNumber(a: Rec | number) { const value = await a; return value; }
export async function awaitPing(a: PingThen) { const value = await a; return value; }
export async function returnPing(a: PingThen) { return a; }
export async function awaitSelfOrText(a: SelfOrText) { const value = await a; return value; }
export async function returnSelfOrText(a: SelfOrText) { return a; }
export async function awaitNumberThen(a: NumberThen) { const value = await a; return value; }
export async function returnNumberThen(a: NumberThen) { return a; }
export async function awaitChain(a: Chain) { const value = await a; return value; }
export async function returnChain(a: Chain) { return a; }
export async function awaitLinked(a: Linked) { const value = await a; return value; }
export async function returnLinked(a: Linked) { return a; }
export async function awaitHolderBox(h: Holder) { const value = await h.box; return value; }
export async function returnHolderBox(h: Holder) { return h.box; }
export async function declaredRec(): Promise<Rec> { return null as any; }
export function declaredRecReturn() { const x: ReturnType<typeof declaredRec> = null as any; return x; }
export function libRec() { const x: Awaited<Rec> = null as any; return x; }
export function libRecThroughAlias() { const x: AwaitedOf<Rec> = null as any; return x; }
export function libPromiseRec() { const x: Awaited<Promise<Rec>> = null as any; return x; }
export function libPing() { const x: Awaited<PingThen> = null as any; return x; }
export function libSelfOrText() { const x: Awaited<SelfOrText> = null as any; return x; }
export function libNumberThen() { const x: Awaited<NumberThen> = null as any; return x; }
export function libChain() { const x: Awaited<Chain> = null as any; return x; }
export function libHolderBox() { const x: Awaited<Holder['box']> = null as any; return x; }
export function libLinked() { const x: Awaited<Linked> = null as any; return x; }
// The lib generator surfaces this standalone host has no `lib*.d.ts` for
// (measured against the real lib).
interface Generator<T, TReturn, TNext> {}
interface AsyncGenerator<T, TReturn, TNext> {}
export async function* yieldRec(r: Rec) { yield r; }
export async function* yieldRecTwice(r: Rec) { yield r; yield r; }
export async function* yieldRecAndPing(r: Rec, a: PingThen) { yield r; yield a; }
export async function* yieldSelfOrText(a: SelfOrText) { yield a; }
export async function* yieldRecOrNumber(a: Rec | number) { yield a; }
export async function* yieldAwaitedRec(r: Rec) { yield await r; }
export async function* returnRecFromGenerator(r: Rec) { return r; }
export async function* yieldNumberReturnRec(r: Rec) { yield 1; return r; }
export async function* yieldRecAndNumber(r: Rec, n: number) { yield r; yield n; }
export async function* yieldNumberThen(a: NumberThen) { yield a; }
export function* syncYieldRec(r: Rec) { yield r; }
interface GenericRec<T> { then(onfulfilled: (v: GenericRec<T>) => void): void }
interface GenericBox<T> { then(onfulfilled: (v: T) => void): void }
interface GenericTree<T> { children: GenericTree<T>[]; value: T }
interface GrowThen<T> { then(onfulfilled: (v: GrowThen<[T]>) => void): void }
export async function awaitGenericRec(a: GenericRec<string>) { const value = await a; return value; }
export async function returnGenericRec(a: GenericRec<string>) { return a; }
export function libGenericRec() { const x: Awaited<GenericRec<string>> = null as any; return x; }
export async function awaitGenericBox(a: GenericBox<number>) { const value = await a; return value; }
export function libGenericBox() { const x: Awaited<GenericBox<number>> = null as any; return x; }
export async function awaitGenericTree(a: GenericTree<string>) { const value = await a; return value; }
export function libGenericTree() { const x: Awaited<GenericTree<string>> = null as any; return x; }
export function libGrowThen() { const x: Awaited<GrowThen<string>> = null as any; return x; }
export async function awaitGrowThen(a: GrowThen<string>) { const value = await a; return value; }
export async function returnGrowThen(a: GrowThen<string>) { return a; }
export async function awaitRecInArray(r: Rec) { return [await r]; }
"#;

/// The checker diagnostic a recovery carrier names, if `node` is one.
fn checker_recovery_of(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Option<crate::semantic_query::CheckerDiagnostic> {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Opaque(QueryError::CheckerRecovery(diagnostic))) => {
            Some(*diagnostic)
        }
        _ => None,
    }
}

/// The TS1062 diagnostic `operation` raises on a recursive thenable.
fn recursive_fulfillment(
    operation: crate::semantic_query::CheckerDiagnosticOperation,
) -> crate::semantic_query::CheckerDiagnostic {
    crate::semantic_query::CheckerDiagnostic {
        code: crate::semantic_query::CheckerDiagnosticCode::RecursiveFulfillmentCallback,
        operation,
    }
}

/// `Promise<payload>`, and the payload node, of one async function's
/// COMPLETE flow return: never degraded, and ReturnOnly exactly when
/// `warm` is false.
#[track_caller]
fn async_payload<R>(
    host: &Arc<VerterHost>,
    name: &str,
    warm: bool,
    pick: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId) -> R,
) -> R {
    with_dispatch(host, |dispatch| {
        let key = key_of(dispatch, RECURSIVE_THENABLE, name);
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
        else {
            panic!("{name} must produce a value");
        };
        assert_eq!(result.degradation(), None, "{name} is a complete answer");
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            usize::from(warm),
            "{name}: warm-admitted exactly when no recursive-thenable failure is on its path"
        );
        let payload = match dispatch.graph().node_data(result.return_type()).as_deref() {
            Some(SemanticNodeData::InstantiationRef { base, args })
                if base.decl_name.as_ref() == "Promise" && args.len() == 1 =>
            {
                args[0]
            }
            other => panic!("{name} must publish `Promise<…>`, got {other:?}"),
        };
        pick(dispatch, payload)
    })
}

/// `Promise<inner>` with no spans.
fn promise_of(inner: TypeExpr) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from("Promise"),
        type_arguments: Arc::from(vec![inner].into_boxed_slice()),
    }
}

fn any() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::Any)
}

/// The node an authored lib `Awaited<…>` annotation reduces to when a
/// consumer demands it (publication keeps the application; the corpus
/// lane's structural-fact demand reduces it), and that node raised.
#[track_caller]
fn reduced_annotation(host: &Arc<VerterHost>, name: &str) -> (SemanticNodeData, TypeExpr) {
    reduced_annotation_in(host, RECURSIVE_THENABLE, name)
}

/// [`reduced_annotation`] over a function of `canonical`.
#[track_caller]
fn reduced_annotation_in(
    host: &Arc<VerterHost>,
    canonical: &str,
    name: &str,
) -> (SemanticNodeData, TypeExpr) {
    with_dispatch(host, |dispatch| {
        let key = key_of(dispatch, canonical, name);
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
        else {
            panic!("{name} must produce a value");
        };
        let node = dispatch
            .normalize_node_keeping_declaration_refs_for_tests(
                result.return_type(),
                crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Expanded,
                ),
            )
            .into_complete_node()
            .unwrap_or_else(|| panic!("{name}: the demand must complete"));
        let data = dispatch
            .graph()
            .node_data(node)
            .unwrap_or_else(|| panic!("{name}: the reduced node exists"))
            .as_ref()
            .clone();
        let raised = host
            .project_node_to_type_expr_for_test(node)
            .unwrap_or_else(|| panic!("{name}: the reduced node raises"));
        (data, raised)
    })
}

/// `await` of a recursive thenable is the checker's error type under TS1062,
/// raised by the await operand.
///
/// tsc 7.0.2, each function's type and its one diagnostic ("Type is
/// referenced directly or indirectly in the fulfillment callback of its own
/// 'then' method."), reported at the `await` expression:
/// `awaitRec` is `Promise<any>`; `awaitPromiseRec` (the payload of
/// `Promise<Rec>` is the recursive thenable) is `Promise<any>`; `awaitPing`
/// (`PingThen` promises `PongThen`, which promises `PingThen`) is
/// `Promise<any>`. The `await` answers the recovery carrier naming that
/// diagnostic, which reads as `any`; the answer is complete, but which
/// types the relation is already unwrapping is a fact about the path, so it
/// never warms.
#[test]
fn awaiting_a_recursive_thenable_recovers_with_any_under_ts1062() {
    use crate::semantic_query::CheckerDiagnosticOperation;
    let host = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    for name in ["awaitRec", "awaitPromiseRec", "awaitPing"] {
        assert_eq!(
            eval(&host, RECURSIVE_THENABLE, name),
            Outcome::Value {
                ty: promise_of(any()),
                degradation: None,
                candidates: 0,
            },
            "{name}"
        );
        let recovery = async_payload(&host, name, false, checker_recovery_of);
        assert_eq!(
            recovery,
            Some(recursive_fulfillment(
                CheckerDiagnosticOperation::AwaitOperand
            )),
            "{name}"
        );
    }
}

/// An async function returning a recursive thenable publishes `Promise<any>`
/// under TS1062, raised by the async return.
///
/// tsc 7.0.2: `returnRec`, `returnPromiseRec` and `returnPing` are each
/// `Promise<any>`, with the one TS1062 reported at the function's name (the
/// async return's awaited type).
#[test]
fn returning_a_recursive_thenable_recovers_with_promise_any_under_ts1062() {
    use crate::semantic_query::CheckerDiagnosticOperation;
    let host = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    for name in ["returnRec", "returnPromiseRec", "returnPing"] {
        assert_eq!(
            eval(&host, RECURSIVE_THENABLE, name),
            Outcome::Value {
                ty: promise_of(any()),
                degradation: None,
                candidates: 0,
            },
            "{name}"
        );
        let recovery = async_payload(&host, name, false, checker_recovery_of);
        assert_eq!(
            recovery,
            Some(recursive_fulfillment(
                CheckerDiagnosticOperation::AsyncReturnPayload
            )),
            "{name}"
        );
    }
}

/// A recursive arm of an awaited union is DROPPED, not recovered: the
/// checker maps its awaited type over the union and leaves out the arm it
/// failed on, so the answer is the other arms and carries no `any`.
///
/// tsc 7.0.2: `awaitSelfOrText` is `Promise<string>` (TS1062 at the
/// `await`), `returnSelfOrText` is `Promise<string>` (TS1062 at the name,
/// measured alone: sharing a file, the checker caches the union's awaited
/// type from the first position and reports only there), and
/// `awaitRecOrNumber` is `Promise<number>` (TS1062 at the `await`).
#[test]
fn a_recursive_arm_is_dropped_from_an_awaited_union() {
    let host = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    for (name, expected) in [
        ("awaitSelfOrText", string()),
        ("returnSelfOrText", string()),
        ("awaitRecOrNumber", number()),
    ] {
        assert_eq!(
            eval(&host, RECURSIVE_THENABLE, name),
            Outcome::Value {
                ty: promise_of(expected),
                degradation: None,
                candidates: 0,
            },
            "{name}"
        );
    }
}

/// The authored lib `Awaited<X>` over a recursive thenable is the checker's
/// error type under TS2589, raised by the lib conditional: `Awaited<V>`
/// recurses into the application it is still evaluating.
///
/// tsc 7.0.2, each read through the tuple wrapper, each reporting
/// "Type instantiation is excessively deep and possibly infinite." at the
/// `Awaited` reference: `Awaited<Rec>`, `AwaitedOf<Rec>` (a generic alias
/// over `Awaited<T>`, measured alone — the checker caches the shared
/// instantiation and reports it once per file), `Awaited<Promise<Rec>>`,
/// `Awaited<PingThen>` and `Awaited<SelfOrText>` are all `[any]` — the
/// error type absorbs the union arm the runtime relation would drop.
#[test]
fn the_authored_awaited_of_a_recursive_thenable_is_the_ts2589_recovery() {
    use crate::semantic_query::{
        CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation,
    };
    let host = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    let ts2589 = CheckerDiagnostic {
        code: CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
        operation: CheckerDiagnosticOperation::LibAwaited,
    };
    for name in [
        "libRec",
        "libRecThroughAlias",
        "libPromiseRec",
        "libPing",
        "libSelfOrText",
    ] {
        let (data, raised) = reduced_annotation(&host, name);
        assert_eq!(
            data,
            SemanticNodeData::Opaque(QueryError::CheckerRecovery(ts2589)),
            "{name}"
        );
        assert_eq!(raised, any(), "{name}: the recovery raises as `any`");
    }
}

/// A declared `Promise<Rec>` return keeps its declared type: the checker
/// reports TS1062 against the annotation, but no type operation recovers
/// there.
///
/// tsc 7.0.2: `ReturnType<typeof declaredRec>` is `Promise<Rec>`, with
/// TS1062 at `declaredRec`'s name.
#[test]
fn a_declared_promise_of_a_recursive_thenable_keeps_its_declared_type() {
    let host = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    let (data, _) = reduced_annotation(&host, "declaredRecReturn");
    with_dispatch(&host, |dispatch| match &data {
        SemanticNodeData::InstantiationRef { base, args }
            if base.decl_name.as_ref() == "Promise" && args.len() == 1 =>
        {
            assert_eq!(
                node_shape(dispatch, args[0]),
                NodeShape::DeclRef("Rec".to_string()),
                "the declared payload stays `Rec`"
            );
        }
        other => panic!("`ReturnType<typeof declaredRec>` is `Promise<Rec>`, got {other:?}"),
    });
}

/// The controls: a thenable that promises a plain value keeps reducing, and
/// a type that refers to itself WITHOUT being a recursive thenable is never
/// the recovery — whether it is not a thenable at all (`Chain`), is what a
/// thenable inside its own body promises (`Holder['box']`, whose callback
/// names `Holder` through the body's self-reference), or is a thenable that
/// holds itself inside the value it promises (`Linked`). All of them stay
/// warm.
///
/// tsc 7.0.2, no diagnostics: `awaitNumberThen` / `returnNumberThen` are
/// `Promise<number>` and `Awaited<NumberThen>` is `number`; `awaitChain` /
/// `returnChain` are `Promise<Chain>` and `Awaited<Chain>` is `Chain`;
/// `awaitHolderBox` / `returnHolderBox` are `Promise<Holder>` and
/// `Awaited<Holder['box']>` is `Holder`; `awaitLinked` / `returnLinked`
/// are `Promise<{ next: Linked; }>` and `Awaited<Linked>` is
/// `{ next: Linked; }`.
#[test]
fn non_recursive_thenables_and_non_thenable_cycles_keep_their_answers() {
    let host = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    for name in ["awaitNumberThen", "returnNumberThen"] {
        assert_clean_warm(&host, RECURSIVE_THENABLE, name, promise_of(number()));
    }
    for name in ["awaitChain", "returnChain"] {
        assert_clean_warm(
            &host,
            RECURSIVE_THENABLE,
            name,
            promise_of(type_ref("Chain")),
        );
    }
    for name in ["awaitHolderBox", "returnHolderBox"] {
        assert_clean_warm(
            &host,
            RECURSIVE_THENABLE,
            name,
            promise_of(type_ref("Holder")),
        );
    }
    for name in ["awaitLinked", "returnLinked"] {
        let next = async_payload(&host, name, true, |dispatch, payload| {
            match dispatch.graph().node_data(payload).as_deref() {
                Some(SemanticNodeData::Object(surface)) => surface
                    .positive_members()
                    .iter()
                    .find(|member| member.string_name() == Some("next"))
                    .map(|member| member.value),
                other => panic!("{name} promises `{{ next: Linked }}`, got {other:?}"),
            }
        });
        let next = next.unwrap_or_else(|| panic!("{name}: the promised value carries `next`"));
        let raised = host.project_node_to_type_expr_for_test(next);
        assert!(
            matches!(&raised, Some(TypeExpr::RecursiveRef { name: referent, .. })
                | Some(TypeExpr::Ref { name: referent, .. }) if referent.as_ref() == "Linked"),
            "{name}: `next` is `Linked`, got {raised:?}"
        );
    }
    let (number_then, raised) = reduced_annotation(&host, "libNumberThen");
    assert_eq!(
        number_then,
        SemanticNodeData::Primitive(PrimitiveKind::Number)
    );
    assert_eq!(raised, number());
    for (name, declaration) in [("libChain", "Chain"), ("libHolderBox", "Holder")] {
        let (data, raised) = reduced_annotation(&host, name);
        assert!(
            matches!(&data, SemanticNodeData::DeclRef { identity } if identity.decl_name.as_ref() == declaration),
            "{name} is `{declaration}`, got {data:?}"
        );
        assert_eq!(raised, type_ref(declaration), "{name}");
    }
    let (linked, _) = reduced_annotation(&host, "libLinked");
    assert!(
        matches!(&linked, SemanticNodeData::Object(surface)
            if surface.positive_members().iter().any(|member| member.string_name() == Some("next"))),
        "`Awaited<Linked>` is `{{ next: Linked; }}`, got {linked:?}"
    );
}

/// A recursive-thenable answer is the same whichever position is demanded
/// first and however often: the relation's in-path failures never warm, so
/// no position can serve another a failure computed on a different path.
/// (tsc 7.0.2 itself is order-dependent here only in which position REPORTS
/// the diagnostic, never in the types measured above.)
#[test]
fn recursive_thenable_answers_do_not_depend_on_demand_order() {
    let names = [
        "awaitSelfOrText",
        "returnSelfOrText",
        "libSelfOrText",
        "awaitRecOrNumber",
        "returnRec",
        "awaitRec",
    ];
    let forward = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    let reverse = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    let first: Vec<Outcome> = names
        .iter()
        .map(|name| eval(&forward, RECURSIVE_THENABLE, name))
        .collect();
    let mut second: Vec<Outcome> = names
        .iter()
        .rev()
        .map(|name| eval(&reverse, RECURSIVE_THENABLE, name))
        .collect();
    second.reverse();
    assert_eq!(first, second, "reversed demand order");
    let again: Vec<Outcome> = names
        .iter()
        .map(|name| eval(&forward, RECURSIVE_THENABLE, name))
        .collect();
    assert_eq!(first, again, "repeated demand");
}

/// `AsyncGenerator<yield, return, unknown>` with no spans.
fn async_generator_of(yielded: TypeExpr, returned: TypeExpr) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from("AsyncGenerator"),
        type_arguments: Arc::from(
            vec![
                yielded,
                returned,
                TypeExpr::Primitive(PrimitiveName::Unknown),
            ]
            .into_boxed_slice(),
        ),
    }
}

fn void() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::Void)
}

fn never() -> TypeExpr {
    TypeExpr::Primitive(PrimitiveName::Never)
}

/// An async generator's YIELD of a recursive thenable contributes no yield
/// type, and its RETURN is the checker's error type.
///
/// tsc 7.0.2, each with TS1062 ("Type is referenced directly or indirectly
/// in the fulfillment callback of its own 'then' method.") at every failing
/// `yield` operand, and at the name for a failing return:
/// `yieldRec` (`yield r`), `yieldRecTwice` (`yield r; yield r`) and
/// `yieldRecAndPing` (`yield r; yield a`, both recursive) are
/// `AsyncGenerator<never, void, unknown>`; `yieldSelfOrText` is
/// `AsyncGenerator<string, void, unknown>` and `yieldRecOrNumber` (`yield a`
/// over `Rec | number`) `AsyncGenerator<number, void, unknown>`, the failing
/// union arm dropped; `yieldAwaitedRec` (`yield await r`) is
/// `AsyncGenerator<any, void, unknown>`, the yield of the await's recovery;
/// `returnRecFromGenerator` (`return r`) is `AsyncGenerator<never, any,
/// unknown>` and `yieldNumberReturnRec` (`yield 1; return r`)
/// `AsyncGenerator<number, any, unknown>`. The controls: `yieldNumberThen`
/// is `AsyncGenerator<number, void, unknown>`, and a SYNC generator awaits
/// nothing — `syncYieldRec` is `Generator<Rec, void, unknown>`.
#[test]
fn async_generator_yields_of_a_recursive_thenable_follow_the_checker() {
    let host = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    for (name, yielded, returned, warm) in [
        ("yieldRec", never(), void(), false),
        ("yieldRecTwice", never(), void(), false),
        ("yieldRecAndPing", never(), void(), false),
        ("yieldSelfOrText", string(), void(), false),
        ("yieldRecOrNumber", number(), void(), false),
        ("yieldAwaitedRec", any(), void(), false),
        ("returnRecFromGenerator", never(), any(), false),
        ("yieldNumberReturnRec", number(), any(), false),
        ("yieldNumberThen", number(), void(), true),
    ] {
        assert_eq!(
            eval(&host, RECURSIVE_THENABLE, name),
            Outcome::Value {
                ty: async_generator_of(yielded, returned),
                degradation: None,
                candidates: usize::from(warm),
            },
            "{name}"
        );
    }
    assert_clean_warm(
        &host,
        RECURSIVE_THENABLE,
        "syncYieldRec",
        TypeExpr::Ref {
            name: Arc::from("Generator"),
            type_arguments: Arc::from(
                vec![
                    type_ref("Rec"),
                    void(),
                    TypeExpr::Primitive(PrimitiveName::Unknown),
                ]
                .into_boxed_slice(),
            ),
        },
    );
}

/// Two DISTINCT yields of which one is a recursive thenable have no oracle:
/// tsc 7.0.2 itself crashes on `async function* g(r: Rec, n: number) {
/// yield r; yield n; }` (a nil-pointer panic in `getUnionTypeEx` under
/// `getReturnTypeFromBody`, whichever order the two yields come in, and
/// for `yield r; yield "s"` as well). This is an INTERNAL CONTRACT, not a
/// measured answer: the yield join drops the failing yield exactly as it
/// does in every program the checker does answer (a lone failing yield
/// contributes `never`, a failing arm of a yielded union is dropped), so
/// `yieldRecAndNumber` is `AsyncGenerator<number, void, unknown>`.
#[test]
fn a_failing_yield_beside_another_yield_drops_out_of_the_join() {
    let host = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    assert_eq!(
        eval(&host, RECURSIVE_THENABLE, "yieldRecAndNumber"),
        Outcome::Value {
            ty: async_generator_of(number(), void()),
            degradation: None,
            candidates: 0,
        }
    );
}

/// A GENERIC thenable whose fulfillment callback takes its own
/// instantiation: the self-reference inside the generic body records the
/// instantiation it stands for, so the relations see `GenericRec<string>`
/// promise `GenericRec<string>` itself.
///
/// tsc 7.0.2: `awaitGenericRec` is `Promise<any>` under TS1062 at the
/// `await`, `returnGenericRec` `Promise<any>` under TS1062 at its name, and
/// `Awaited<GenericRec<string>>` is `any` under TS2589. The controls: a
/// generic thenable promising its argument (`GenericBox<number>`) is
/// `number` both ways, and a generic interface referencing itself without
/// a `then` (`GenericTree<string>`) is itself — `Promise<GenericTree<string>>`
/// awaited, `GenericTree<string>` through `Awaited`.
#[test]
fn a_generic_self_referencing_thenable_follows_its_recorded_instantiation() {
    use crate::semantic_query::{
        CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation,
    };
    let host = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    for (name, operation) in [
        ("awaitGenericRec", CheckerDiagnosticOperation::AwaitOperand),
        (
            "returnGenericRec",
            CheckerDiagnosticOperation::AsyncReturnPayload,
        ),
    ] {
        assert_eq!(
            eval(&host, RECURSIVE_THENABLE, name),
            Outcome::Value {
                ty: promise_of(any()),
                degradation: None,
                candidates: 0,
            },
            "{name}"
        );
        let recovery = async_payload(&host, name, false, checker_recovery_of);
        assert_eq!(recovery, Some(recursive_fulfillment(operation)), "{name}");
    }
    let (data, raised) = reduced_annotation(&host, "libGenericRec");
    assert_eq!(
        data,
        SemanticNodeData::Opaque(QueryError::CheckerRecovery(CheckerDiagnostic {
            code: CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
            operation: CheckerDiagnosticOperation::LibAwaited,
        }))
    );
    assert_eq!(raised, any());
    assert_clean_warm(
        &host,
        RECURSIVE_THENABLE,
        "awaitGenericBox",
        promise_of(number()),
    );
    let (data, _) = reduced_annotation(&host, "libGenericBox");
    assert_eq!(data, SemanticNodeData::Primitive(PrimitiveKind::Number));
    let tree = TypeExpr::Ref {
        name: Arc::from("GenericTree"),
        type_arguments: Arc::from(vec![string()].into_boxed_slice()),
    };
    assert_clean_warm(
        &host,
        RECURSIVE_THENABLE,
        "awaitGenericTree",
        promise_of(tree),
    );
    let (data, _) = reduced_annotation(&host, "libGenericTree");
    assert!(
        matches!(&data, SemanticNodeData::InstantiationRef { base, .. } if base.decl_name.as_ref() == "GenericTree"),
        "`Awaited<GenericTree<string>>` is `GenericTree<string>`, got {data:?}"
    );
}

/// A thenable that GROWS without ever repeating a type.
///
/// The lib conditional: tsc 7.0.2 answers `Awaited<GrowThen<string>>` with
/// `any` under TS2589 in about a second — its tail run reaches the
/// checker's 1000-step limit — and so does the relation here, through the
/// same count.
///
/// The runtime relation has NO such rule in the checker: a 5000-deep chain
/// of distinct thenables awaits to its value, and neither `await` of a
/// `GrowThen<string>` nor returning one from an async function terminated
/// within 25 minutes (0.9 GB and climbing) on tsc 7.0.2 — no oracle answer
/// exists for them. The relation here charges every tail step to the
/// connected-work budget and stops where it runs out: the `await` has no
/// value and the async return publishes the typed gap — never the thenable
/// the relation stopped at, never warm, and never the checker's diagnostic.
/// (The runtime half runs under a reduced budget so the test stays fast;
/// the production budget's cost is recorded in the ledger.)
#[test]
fn a_growing_thenable_hits_the_checker_limit_in_the_lib_conditional_only() {
    use crate::semantic_query::{
        CheckerDiagnostic, CheckerDiagnosticCode, CheckerDiagnosticOperation,
    };
    let host = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    let (data, raised) = reduced_annotation(&host, "libGrowThen");
    assert_eq!(
        data,
        SemanticNodeData::Opaque(QueryError::CheckerRecovery(CheckerDiagnostic {
            code: CheckerDiagnosticCode::ExcessivelyDeepInstantiation,
            operation: CheckerDiagnosticOperation::LibAwaited,
        }))
    );
    assert_eq!(raised, any());
    let under_budget = |name: &str| {
        with_dispatch(&host, |dispatch| {
            dispatch.set_connected_limits_for_tests(
                8_000,
                super::connected_demand::MAX_CONNECTED_QUERY_DEPTH,
            );
            let key = key_of(dispatch, RECURSIVE_THENABLE, name);
            eval_key_on(&host, dispatch, key)
        })
    };
    assert_eq!(under_budget("awaitGrowThen"), Outcome::Miss);
    match under_budget("returnGrowThen") {
        Outcome::Value {
            ty,
            degradation,
            candidates,
        } => {
            assert_eq!(
                (degradation, candidates),
                (Some(FlowReturnDegradation::UnresolvedValue), 0),
                "returnGrowThen: the typed gap, never warm, got {ty:?}"
            );
            assert!(
                !format!("{ty:?}").contains("GrowThen"),
                "returnGrowThen: never the thenable the relation stopped at, got {ty:?}"
            );
        }
        other => panic!("returnGrowThen: the typed gap, got {other:?}"),
    }
}

/// A chain of DISTINCT thenables `C0 → C1 → … → number`: the runtime
/// relation runs its tail steps in one loop, so a chain's length costs no
/// query depth, and a long chain awaits to its value as the checker's does.
/// tsc 7.0.2 has no depth rule here (5000 steps measured for `await`; at 8,
/// 40 and 200 steps `awaitChain` and `returnChain` are `Promise<number>`,
/// `yieldChain` `AsyncGenerator<number, void, unknown>` and
/// `returnFromGenerator` `AsyncGenerator<never, number, unknown>`).
///
/// The connected-work budget stays this substrate's typed incompleteness
/// for work it cannot finish: a demand whose budget runs out part-way down
/// the chain has no `await` value, and every wrap publishes the typed gap —
/// never the thenable the relation stopped at, which a budget-cut read used
/// to hand the wrap as its payload (`Promise<C14>`).
#[test]
fn a_long_thenable_chain_awaits_to_its_value_within_the_work_budget() {
    let chain = |steps: usize| {
        let mut source = String::new();
        for index in 0..steps {
            let next = if index + 1 == steps {
                "number".to_owned()
            } else {
                format!("C{}", index + 1)
            };
            source.push_str(&format!(
                "interface C{index} {{ then(onfulfilled: (v: {next}) => void): void }}\n"
            ));
        }
        source
            .push_str("export async function awaitChain(c: C0) { const v = await c; return v; }\n");
        source.push_str("export async function returnChain(c: C0) { return c; }\n");
        // The lib generator surface this standalone host has no
        // `lib*.d.ts` for.
        source.push_str("interface AsyncGenerator<T, TReturn, TNext> {}\n");
        source.push_str("export async function* yieldChain(c: C0) { yield c; }\n");
        source.push_str("export async function* returnFromGenerator(c: C0) { return c; }\n");
        source
    };
    const SHORT: &str = "/ws/cov/thenable_chain_short.ts";
    const LONG: &str = "/ws/cov/thenable_chain_long.ts";
    let short = chain(8);
    let long = chain(200);
    let host = host_with(&[(SHORT, short.as_str()), (LONG, long.as_str())]);
    for file in [SHORT, LONG] {
        for name in ["awaitChain", "returnChain"] {
            assert_clean_warm(&host, file, name, promise_of(number()));
        }
        assert_clean_warm(
            &host,
            file,
            "yieldChain",
            async_generator_of(number(), void()),
        );
        assert_clean_warm(
            &host,
            file,
            "returnFromGenerator",
            async_generator_of(never(), number()),
        );
    }
    // The budget's measure: a cold chain charges 16 units per element plus
    // 6, every tail step among them, so under a budget of 806 units a
    // 50-element chain answers and a 51-element one does not.
    for (steps, answers) in [(50, true), (51, false)] {
        let file = format!("/ws/cov/thenable_chain_{steps}.ts");
        let source = chain(steps);
        let host = host_with(&[(file.as_str(), source.as_str())]);
        let outcome = with_dispatch(&host, |dispatch| {
            dispatch.set_connected_limits_for_tests(
                16 * 50 + 6,
                super::connected_demand::MAX_CONNECTED_QUERY_DEPTH,
            );
            let key = key_of(dispatch, &file, "awaitChain");
            eval_key_on(&host, dispatch, key)
        });
        if answers {
            assert!(
                matches!(&outcome, Outcome::Value { ty, degradation: None, .. } if *ty == promise_of(number())),
                "{steps} elements answer within 806 units, got {outcome:?}"
            );
        } else {
            assert_eq!(outcome, Outcome::Miss, "{steps} elements exceed 806 units");
        }
    }
    // The same 200-step chain under a work budget it cannot finish within:
    // each tail step is charged, so the relation stops as a typed partial.
    let budgeted = host_with(&[(LONG, long.as_str())]);
    let under_budget = |name: &str| {
        with_dispatch(&budgeted, |dispatch| {
            dispatch.set_connected_limits_for_tests(
                600,
                super::connected_demand::MAX_CONNECTED_QUERY_DEPTH,
            );
            let key = key_of(dispatch, LONG, name);
            eval_key_on(&budgeted, dispatch, key)
        })
    };
    for name in [
        "awaitChain",
        "returnChain",
        "yieldChain",
        "returnFromGenerator",
    ] {
        match under_budget(name) {
            Outcome::Value {
                ty,
                degradation,
                candidates,
            } => {
                assert!(
                    degradation.is_some() && candidates == 0,
                    "{name}: a budget-cut relation read is never a clean payload, got {ty:?}"
                );
                assert!(
                    !format!("{ty:?}").contains("C1"),
                    "{name}: never the thenable the relation stopped at, got {ty:?}"
                );
            }
            Outcome::Miss => {}
            other => panic!("{name}: a typed incompleteness, got {other:?}"),
        }
    }
}

/// A chain of distinct thenables `C0 → C1 → … → number`, one step per
/// character of `steps`, each step's `then` in the given shape:
///
/// - `t`: `then(onfulfilled: (v: Next) => void)` — a single callback into a
///   non-union value, the checker's TAIL step;
/// - `n`: `then(onfulfilled?: ((v: Next) => void) | null)` — a callback
///   union, one NESTED step;
/// - `u`: `then(onfulfilled: (v: Next | string) => void)` — a union value,
///   one nested step;
/// - `b`: `then(onfulfilled?: ((v: Next | string) => void) | null)` — both,
///   two nested steps.
///
/// `probe` reads `Awaited<C0>`.
fn thenable_chain(steps: &str) -> String {
    let count = steps.len();
    let mut source = String::new();
    for (index, kind) in steps.chars().enumerate() {
        let next = if index + 1 < count {
            format!("C{}", index + 1)
        } else {
            "number".to_owned()
        };
        let callback = match kind {
            't' => format!("onfulfilled: (v: {next}) => void"),
            'n' => format!("onfulfilled?: ((v: {next}) => void) | null"),
            'u' => format!("onfulfilled: (v: {next} | string) => void"),
            _ => format!("onfulfilled?: ((v: {next} | string) => void) | null"),
        };
        source.push_str(&format!(
            "interface C{index} {{ then({callback}): void }}\n"
        ));
    }
    source.push_str("export function probe() { const x: Awaited<C0> = null as any; return x; }\n");
    source
}

/// The lib conditional's reduction of `probe` in `source`.
fn lib_awaited_probe(source: &str) -> SemanticNodeData {
    const CHAIN: &str = "/ws/cov/thenable_chain.ts";
    let host = host_with(&[(CHAIN, source)]);
    reduced_annotation_in(&host, CHAIN, "probe").0
}

fn is_ts2589(data: &SemanticNodeData) -> bool {
    matches!(
        data,
        SemanticNodeData::Opaque(QueryError::CheckerRecovery(diagnostic))
            if diagnostic.code
                == crate::semantic_query::CheckerDiagnosticCode::ExcessivelyDeepInstantiation
    )
}

/// The lib conditional's TAIL runs stop where the checker's do.
///
/// tsc 7.0.2 (`Awaited<C0>` through the tuple wrapper, `--strict`) over
/// chains of distinct thenables: 999 tail steps are `number` and 1000 are
/// `any` under TS2589 at the `Awaited` reference. A run entered through a
/// callback union starts with one step counted: one nested step then 998
/// tail steps are `number`, then 999 are `any` under TS2589.
#[test]
fn lib_awaited_tail_runs_stop_at_the_checker_limit() {
    let tails = |count: usize| "t".repeat(count);
    assert_eq!(
        lib_awaited_probe(&thenable_chain(&tails(999))),
        SemanticNodeData::Primitive(PrimitiveKind::Number)
    );
    assert!(is_ts2589(&lib_awaited_probe(&thenable_chain(&tails(1000)))));
    assert_eq!(
        lib_awaited_probe(&thenable_chain(&format!("n{}", tails(998)))),
        SemanticNodeData::Primitive(PrimitiveKind::Number)
    );
    assert!(is_ts2589(&lib_awaited_probe(&thenable_chain(&format!(
        "n{}",
        tails(999)
    )))));
}

/// The lib conditional's NESTED steps stop where the checker's instantiation
/// depth does.
///
/// tsc 7.0.2: 97 nested steps through callback unions are `number` (the
/// checker reports TS2589 there while keeping the value) and 98 are `any`
/// under TS2589; 97 steps into union values are `string | number` and 98
/// `any`; a step that is both counts twice — 48 such steps then a tail
/// step are `string | number`, 49 are `any`. `Awaited` over `Promise`
/// nested 96 or 97 deep is `number`, 98 deep `any` under TS2589 (every
/// `Promise`'s `onfulfilled` is optional and nullable).
#[test]
fn lib_awaited_nested_steps_stop_at_the_checker_limit() {
    let number = || SemanticNodeData::Primitive(PrimitiveKind::Number);
    let string_or_number = |data: &SemanticNodeData| matches!(data, SemanticNodeData::Union(members) if members.len() == 2);
    assert_eq!(
        lib_awaited_probe(&thenable_chain(&"n".repeat(97))),
        number()
    );
    assert!(is_ts2589(&lib_awaited_probe(&thenable_chain(
        &"n".repeat(98)
    ))));
    assert!(string_or_number(&lib_awaited_probe(&thenable_chain(
        &"u".repeat(97)
    ))));
    assert!(is_ts2589(&lib_awaited_probe(&thenable_chain(
        &"u".repeat(98)
    ))));
    assert!(string_or_number(&lib_awaited_probe(&thenable_chain(
        &format!("{}t", "b".repeat(48))
    ))));
    assert!(is_ts2589(&lib_awaited_probe(&thenable_chain(&format!(
        "{}t",
        "b".repeat(49)
    )))));
    // Lowering a type argument nested this deep recurses per level, so the
    // probes run on the serve thread's stack size, as production does.
    std::thread::Builder::new()
        .stack_size(8 * 1024 * 1024)
        .spawn(|| {
            let promise_chain = |depth: usize| {
                let ty = (0..depth).fold("number".to_owned(), |ty, _| format!("Promise<{ty}>"));
                format!(
                    "export function probe() {{ const x: Awaited<{ty}> = null as any; return x; }}\n"
                )
            };
            assert_eq!(
                lib_awaited_probe(&promise_chain(96)),
                SemanticNodeData::Primitive(PrimitiveKind::Number)
            );
            assert_eq!(
                lib_awaited_probe(&promise_chain(97)),
                SemanticNodeData::Primitive(PrimitiveKind::Number)
            );
            assert!(is_ts2589(&lib_awaited_probe(&promise_chain(98))));
        })
        .expect("spawn the probe thread")
        .join()
        .expect("the Promise-chain probes");
}

// ──────────────────────────────────────────────────────────────────────
// Class expressions: the constructor type, the instance surface, and the
// mixin form over a constructor-constrained type parameter
// ──────────────────────────────────────────────────────────────────────

const CLASSES: &str = "/ws/cov/classes.ts";
const CLASSES_SRC: &str = r#"
interface Base { label: string }
declare const BaseCtor: new (...args: any[]) => Base;
export class Named { n = 1; constructor(a: string, b?: number) {} static s = "s"; }

export function plainClass() {
  return class { extra = 1; readonly lit = 1; static st = "s"; m(): number { return 1; } get g(): boolean { return true; } };
}
export function ctorClass() { return class { constructor(public a: string, b?: number) {} }; }
export function extendsNamed() { return class extends Named { e = 2; }; }
export function Mixin<S extends new (...args: any[]) => Base>(Base: S) {
  return class extends Base { extra = 1; };
}
export declare function DM<S extends new (...args: any[]) => Base>(b: S): (new (...args: any[]) => { extra: number }) & S;
declare function G<S>(b: S): S;
declare function O<T>(a: T): T;
declare function O(a: number, b: number): number;
declare function assertString(x: unknown): asserts x is string;
export function staticBlockAssertion(x: string | number) {
  const C = class { static { assertString(x); } };
  return { C, x };
}

export function plainInstance() { const x: InstanceType<ReturnType<typeof plainClass>> = null as any; return x; }
export function plainExtra() { const x: InstanceType<ReturnType<typeof plainClass>>['extra'] = null as any; return x; }
export function plainLit() { const x: InstanceType<ReturnType<typeof plainClass>>['lit'] = null as any; return x; }
export function plainMethod() { const x: InstanceType<ReturnType<typeof plainClass>>['m'] = null as any; return x; }
export function plainGetter() { const x: InstanceType<ReturnType<typeof plainClass>>['g'] = null as any; return x; }
export function plainStatic() { const x: ReturnType<typeof plainClass>['st'] = null as any; return x; }
export function protoRead() { const C = class { extra = 1; }; return C.prototype; }
export function plainCtorParams() { const x: ConstructorParameters<ReturnType<typeof plainClass>> = null as any; return x; }
export function ctorParams() { const x: ConstructorParameters<ReturnType<typeof ctorClass>> = null as any; return x; }
export function ctorProperty() { const x: InstanceType<ReturnType<typeof ctorClass>>['a'] = null as any; return x; }
export function derivedInstance() { const x: InstanceType<ReturnType<typeof extendsNamed>> = null as any; return x; }
export function derivedCtorParams() { const x: ConstructorParameters<ReturnType<typeof extendsNamed>> = null as any; return x; }
export function derivedInherited() { const x: InstanceType<ReturnType<typeof extendsNamed>>['n'] = null as any; return x; }
export function derivedOwn() { const x: InstanceType<ReturnType<typeof extendsNamed>>['e'] = null as any; return x; }
export function derivedStatic() { const x: ReturnType<typeof extendsNamed>['s'] = null as any; return x; }
export function mixinInstance() { const x: InstanceType<ReturnType<typeof Mixin<typeof BaseCtor>>> = null as any; return x; }
export function mixinCtorParams() { const x: ConstructorParameters<ReturnType<typeof Mixin<typeof BaseCtor>>> = null as any; return x; }
export function mixinInherited() { const x: InstanceType<ReturnType<typeof Mixin<typeof BaseCtor>>>['label'] = null as any; return x; }
export function mixinOwn() { const x: InstanceType<ReturnType<typeof Mixin<typeof BaseCtor>>>['extra'] = null as any; return x; }
export function declaredMixinInstance() { const x: InstanceType<ReturnType<typeof DM<typeof BaseCtor>>> = null as any; return x; }
export function instantiatedDeclaration() { const x: ReturnType<typeof G<string>> = null as any; return x; }
export function instantiatedOverloads() { const x: ReturnType<typeof O<string>> = null as any; return x; }
"#;

/// How a CLASSES probe's answer is admitted.
///
/// An answer read through a class expression's value is `ReturnOnly`: the
/// class's constructor and field initializers are callables no indexed
/// function position serves, so the capture family keeps its typed gap
/// over them — the answer is complete and undegraded, and it never warms.
#[derive(Clone, Copy, Debug)]
enum ClassProbeAdmission {
    Warm,
    ReturnOnly,
}

/// Evaluate one CLASSES probe CLEAN (undegraded) under `admission`, and
/// hand its answer to `check` normalized the way the signature corpus
/// compares a probe: deferred utility applications reduced, named
/// declarations — and class expressions — kept by name.
#[track_caller]
fn with_class_probe<R>(
    name: &str,
    admission: ClassProbeAdmission,
    check: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId) -> R,
) -> R {
    let host = host_with(&[(CLASSES, CLASSES_SRC)]);
    with_dispatch(&host, |dispatch| {
        let key = key_of(dispatch, CLASSES, name);
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone())))
        else {
            panic!("{name} must produce a value");
        };
        assert_eq!(result.degradation(), None, "{name} must evaluate clean");
        assert_eq!(
            dispatch
                .graph()
                .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
            match admission {
                ClassProbeAdmission::Warm => 1,
                ClassProbeAdmission::ReturnOnly => 0,
            },
            "{name} must be admitted {admission:?}"
        );
        let normalized = dispatch
            .normalize_node_keeping_declaration_refs_for_tests(
                result.return_type(),
                crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Expanded,
                ),
            )
            .into_complete_node()
            .unwrap_or_else(|| panic!("{name}: the probe's structural-fact demand completes"));
        check(dispatch, normalized)
    })
}

/// One CLASSES probe structurally equals the checker print `expected`,
/// through the shared checker-syntax projection.
#[track_caller]
fn assert_class_probe(name: &str, admission: ClassProbeAdmission, expected: &str) {
    use crate::u6_flow_shape_corpus_tests::u6_flow_expect_tests::{checker_syntax, render_node};
    with_class_probe(name, admission, |dispatch, node| {
        let parsed = checker_syntax::parse(expected)
            .unwrap_or_else(|error| panic!("`{expected}` must parse: {error}"));
        assert!(
            checker_syntax::matches_node(dispatch, node, &parsed, 0),
            "{name}: expected `{expected}`, measured `{}`",
            render_node(dispatch, node, 0)
        );
    });
}

/// One CLASSES probe is a parameter tuple: each element's label, its
/// optionality, and whether its value is the `string` primitive.
#[track_caller]
fn class_probe_tuple(name: &str) -> Vec<(Option<String>, bool, bool)> {
    with_class_probe(
        name,
        ClassProbeAdmission::ReturnOnly,
        |dispatch, node| match dispatch.graph().node_data(node).as_deref() {
            Some(SemanticNodeData::Tuple { elements, .. }) => elements
                .iter()
                .map(|element| {
                    (
                        element.label.as_deref().map(str::to_owned),
                        element.optional,
                        matches!(
                            dispatch.graph().node_data(element.value).as_deref(),
                            Some(SemanticNodeData::Primitive(PrimitiveKind::String))
                        ),
                    )
                })
                .collect(),
            other => panic!("{name}: expected a parameter tuple, got {other:?}"),
        },
    )
}

/// A class expression's value is its constructor type: an instance named
/// the way the checker names it, the class's own members on that
/// instance, its statics on the constructor, and a constructor that takes
/// nothing when the class declares none.
///
/// Oracle (tsc 7.0.2), over `plainClass`'s `class { extra = 1; readonly
/// lit = 1; static st = "s"; m(): number { … }; get g(): boolean { … } }`:
/// the instance prints `(Anonymous class)` (no type-parameter clause
/// encloses it); `extra` is `number`, `lit` is `1`, `m` is `() => number`,
/// `g` is `boolean`; the static `st` is `string`; `ConstructorParameters`
/// is `[]`. `protoRead`'s `C.prototype` over `const C = class { extra = 1;
/// }` is `C`: the constructor's `prototype` is the instance, and a class
/// expression that initializes a variable is named after it.
#[test]
fn class_expression_value_is_its_constructor_over_its_own_members() {
    assert_class_probe(
        "plainInstance",
        ClassProbeAdmission::ReturnOnly,
        "(Anonymous class)",
    );
    assert_class_probe("plainExtra", ClassProbeAdmission::ReturnOnly, "number");
    assert_class_probe("plainLit", ClassProbeAdmission::ReturnOnly, "1");
    assert_class_probe(
        "plainMethod",
        ClassProbeAdmission::ReturnOnly,
        "() => number",
    );
    assert_class_probe("plainGetter", ClassProbeAdmission::ReturnOnly, "boolean");
    assert_class_probe("plainStatic", ClassProbeAdmission::ReturnOnly, "string");
    assert_class_probe("protoRead", ClassProbeAdmission::ReturnOnly, "C");
    assert_eq!(class_probe_tuple("plainCtorParams"), Vec::new());
}

/// A declared constructor's parameters are the class's construct
/// signature, and a parameter property is an instance member.
///
/// Oracle (tsc 7.0.2), over `class { constructor(public a: string, b?:
/// number) {} }`: `ConstructorParameters` is `[a: string, b?: number |
/// undefined]` and the instance's `a` is `string`.
#[test]
fn class_expression_declared_constructor_types_the_construct_signature() {
    assert_eq!(
        class_probe_tuple("ctorParams"),
        vec![
            (Some("a".to_owned()), false, true),
            (Some("b".to_owned()), true, false),
        ]
    );
    assert_class_probe("ctorProperty", ClassProbeAdmission::ReturnOnly, "string");
}

/// A class expression extending a named class inherits the base
/// constructor's parameters (it declares none of its own), the base
/// instance members, and the base statics.
///
/// Oracle (tsc 7.0.2), over `class extends Named { e = 2; }` with `class
/// Named { n = 1; constructor(a: string, b?: number) {} static s = "s"; }`:
/// the instance prints `(Anonymous class)`; `ConstructorParameters` is
/// `[a: string, b?: number | undefined]`; `n` and `e` are `number`; the
/// static `s` is `string`.
#[test]
fn class_expression_extending_a_class_inherits_its_constructor_and_members() {
    assert_class_probe(
        "derivedInstance",
        ClassProbeAdmission::ReturnOnly,
        "(Anonymous class)",
    );
    assert_eq!(
        class_probe_tuple("derivedCtorParams"),
        vec![
            (Some("a".to_owned()), false, true),
            (Some("b".to_owned()), true, false),
        ]
    );
    assert_class_probe(
        "derivedInherited",
        ClassProbeAdmission::ReturnOnly,
        "number",
    );
    assert_class_probe("derivedOwn", ClassProbeAdmission::ReturnOnly, "number");
    assert_class_probe("derivedStatic", ClassProbeAdmission::ReturnOnly, "string");
}

/// The mixin form: a class expression extending a parameter typed by a
/// constructor-constrained type parameter is its constructor type
/// intersected with that type parameter, so instantiating the factory
/// composes the two constructors under the checker's mixin rule — the
/// instance is the class over the base instance, named after the factory
/// whose clause encloses the class.
///
/// Oracle (tsc 7.0.2), over `function Mixin<S extends new (...args: any[])
/// => Base>(Base: S) { return class extends Base { extra = 1; }; }` and
/// `declare const BaseCtor: new (...args: any[]) => Base`:
/// `InstanceType<ReturnType<typeof Mixin<typeof BaseCtor>>>` prints
/// `Mixin.(Anonymous class) & Base`, its `label` is `string` and its
/// `extra` is `number`, and `ConstructorParameters` is `any[]`.
#[test]
fn mixin_class_expression_composes_with_its_instantiated_base() {
    assert_class_probe(
        "mixinInstance",
        ClassProbeAdmission::ReturnOnly,
        "Mixin.(Anonymous class) & Base",
    );
    assert_class_probe("mixinCtorParams", ClassProbeAdmission::ReturnOnly, "any[]");
    assert_class_probe("mixinInherited", ClassProbeAdmission::ReturnOnly, "string");
    assert_class_probe("mixinOwn", ClassProbeAdmission::ReturnOnly, "number");
}

/// A DECLARED mixin factory's result composes the same way: the
/// constructor it declares is a mixin constructor, so its instance mixes
/// into the instantiated base's.
///
/// Oracle (tsc 7.0.2), over `declare function DM<S extends new (...args:
/// any[]) => Base>(b: S): (new (...args: any[]) => { extra: number }) &
/// S`: `InstanceType<ReturnType<typeof DM<typeof BaseCtor>>>` is `{ extra:
/// number; } & Base`.
#[test]
fn declared_mixin_factory_instance_is_its_constructor_result_over_the_base() {
    assert_class_probe(
        "declaredMixinInstance",
        ClassProbeAdmission::Warm,
        "{ extra: number; } & Base",
    );
}

/// An instantiation expression over a function DECLARATION instantiates
/// every signature the declaration carries whose type-parameter list
/// accepts the arguments, and drops the others.
///
/// Oracle (tsc 7.0.2): `ReturnType<typeof G<string>>` over `declare
/// function G<S>(b: S): S` is `string`; `ReturnType<typeof O<string>>`
/// over the overloads `O<T>(a: T): T` and `O(a: number, b: number):
/// number` is `string` (the non-generic overload takes no type argument).
#[test]
fn instantiation_expression_instantiates_a_declarations_signatures() {
    assert_class_probe(
        "instantiatedDeclaration",
        ClassProbeAdmission::Warm,
        "string",
    );
    assert_class_probe("instantiatedOverloads", ClassProbeAdmission::Warm, "string");
}

/// A class expression's class-evaluation-time positions RUN in the
/// enclosing frame: an `asserts` call in a static block narrows what
/// follows the class expression, so the composed class value never lets
/// the frame publish the un-narrowed read clean.
///
/// Oracle (tsc 7.0.2): `function staticBlockAssertion(x: string | number) {
/// const C = class { static { assertString(x); } }; return { C, x }; }`
/// returns `{ C: { new (): {}; }; x: string; }`. The narrowing through a
/// static block is not modelled, so the statement carries the typed
/// guard-narrowing gap.
#[test]
fn class_expression_static_block_assertion_is_not_dropped() {
    let host = host_with(&[(CLASSES, CLASSES_SRC)]);
    assert_degraded(
        &host,
        CLASSES,
        "staticBlockAssertion",
        FlowReturnDegradation::FlowGap(FlowGap::GuardNarrowing),
    );
}

/// An array literal's element is a flow value, so `[await r]` over a
/// recursive thenable carries the await's recovery as its element.
///
/// tsc 7.0.2: `awaitRecInArray` (`return [await r]`) is `Promise<any[]>`,
/// with TS1062 at the `await`. The element is the `AwaitOperand` recovery
/// carrier; like every answer built over a TS1062 failure it never warms.
#[test]
fn an_awaited_recursive_thenable_in_an_array_literal_is_the_recovery_element() {
    use crate::semantic_query::CheckerDiagnosticOperation;
    let host = host_with(&[(RECURSIVE_THENABLE, RECURSIVE_THENABLE_SRC)]);
    assert_eq!(
        eval(&host, RECURSIVE_THENABLE, "awaitRecInArray"),
        Outcome::Value {
            ty: promise_of(TypeExpr::Array {
                element: Arc::new(any()),
                readonly: false,
            }),
            degradation: None,
            candidates: 0,
        }
    );
    let element = async_payload(
        &host,
        "awaitRecInArray",
        false,
        |dispatch, payload| match dispatch.graph().node_data(payload).as_deref() {
            Some(SemanticNodeData::Array { element, .. }) => {
                checker_recovery_of(dispatch, *element)
            }
            other => panic!("`[await r]` is an array, got {other:?}"),
        },
    );
    assert_eq!(
        element,
        Some(recursive_fulfillment(
            CheckerDiagnosticOperation::AwaitOperand
        ))
    );
}

// ──────────────────────────────────────────────────────────────────────
// A declaration's self-reference read through a member path
// ──────────────────────────────────────────────────────────────────────

/// Declarations that reference themselves inside their own bodies. Every
/// expected answer below is measured on tsc 7.0.2 over this module
/// (`--noEmit --strict --target es2022`, each type read through the tuple
/// wrapper quoted by TS2322).
const SELF_REFERENCE: &str = "/ws/cov/self_reference.ts";
const SELF_REFERENCE_SRC: &str = r#"
interface Chain { next: Chain; v: number }
interface Holder { self: Holder; items: Holder[] }
interface Box<T> { inner: Box<T>; value: T }
export function readNext(a: Chain) { return a.next; }
export function readNextNext(a: Chain) { return a.next.next.v; }
export function readSelf(h: Holder) { return h.self; }
export function readSelfItems(h: Holder) { return h.self.items; }
export function readBoxInnerValue(b: Box<string>) { return b.inner.inner.value; }
export function indexedNext() { const x: Chain['next'] = null as any; return x; }
export function indexedNextV() { const x: Chain['next']['v'] = null as any; return x; }
"#;

/// A member read whose member is its declaration's own self-reference
/// (`next: Chain` inside `Chain`) reads that declaration, and a longer
/// path walks on through it.
///
/// tsc 7.0.2: `readNext` (`a.next`) is `Chain`, `readNextNext`
/// (`a.next.next.v`) `number`, `readSelf` (`h.self`) `Holder`,
/// `readSelfItems` (`h.self.items`) `Holder[]`, and the generic
/// `readBoxInnerValue` (`b.inner.inner.value` over `Box<string>`)
/// `string`. A declaration's body lowers the self-reference as the
/// `RecursiveRef` sentinel, and the path walk used to end at it in a miss.
#[test]
fn a_member_read_through_a_self_reference_walks_on_through_the_declaration() {
    let host = host_with(&[(SELF_REFERENCE, SELF_REFERENCE_SRC)]);
    assert_clean_warm(&host, SELF_REFERENCE, "readNext", type_ref("Chain"));
    assert_clean_warm(&host, SELF_REFERENCE, "readNextNext", number());
    assert_clean_warm(&host, SELF_REFERENCE, "readSelf", type_ref("Holder"));
    assert_clean_warm(&host, SELF_REFERENCE, "readBoxInnerValue", string());
    match &eval(&host, SELF_REFERENCE, "readSelfItems") {
        Outcome::Value {
            ty:
                TypeExpr::Array {
                    element,
                    readonly: false,
                },
            degradation: None,
            candidates: 1,
        } => assert!(
            matches!(element.as_ref(), TypeExpr::RecursiveRef { name, .. }
                | TypeExpr::Ref { name, .. } if name.as_ref() == "Holder"),
            "`h.self.items` is `Holder[]`, got {element:?}"
        ),
        other => panic!("`h.self.items` is a clean, warm `Holder[]`, got {other:?}"),
    }
    // A projected path that crosses the self-reference MID-walk: the flow
    // reads above project one member at a time, a type path does not. The
    // base is `Chain`'s own expanded surface, whose `next` is the sentinel.
    with_dispatch(&host, |dispatch| {
        let key = key_of(dispatch, SELF_REFERENCE, "indexedNext");
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)))
        else {
            panic!("indexedNext must produce a value");
        };
        let surface = dispatch
            .normalize_node_keeping_declaration_refs_for_tests(
                result.return_type(),
                crate::semantic_query::ProjectionReductionContext::published(
                    crate::semantic_query::ProjectionMode::Expanded,
                ),
            )
            .into_complete_node()
            .expect("`Chain['next']` reduces");
        let member = |name: &str| {
            crate::semantic_query::PathSegment::Member(
                crate::semantic_query::PropertyKey::identifier(name),
            )
        };
        let projected = match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
            base: surface,
            path: Arc::from(vec![member("next"), member("next"), member("v")].into_boxed_slice()),
            context: crate::semantic_query::ProjectionReductionContext::published(
                crate::semantic_query::ProjectionMode::Navigate,
            ),
        }) {
            QueryResult::Value(SemanticQueryOutput { value, .. }) => value,
            other => panic!("`Chain` projects `next.next.v`, got {other:?}"),
        };
        assert_eq!(
            dispatch.graph().node_data(projected).as_deref(),
            Some(&SemanticNodeData::Primitive(PrimitiveKind::Number)),
            "`Chain['next']['next']['v']` is `number`"
        );
    });
}

/// An indexed access whose member is its declaration's self-reference is
/// that declaration.
///
/// tsc 7.0.2: `Chain['next']` is `Chain` and `Chain['next']['v']` is
/// `number`. The flow lane publishes the authored annotation; a consumer
/// that reduces it (the corpus lane's structural-fact demand) used to read
/// a miss at the self-reference. Under that expanded demand
/// `Chain['next']` now reduces to `Chain`'s own surface, `next` the
/// self-reference and `v` a `number`.
#[test]
fn an_indexed_access_of_a_self_reference_is_the_declaration() {
    let host = host_with(&[(SELF_REFERENCE, SELF_REFERENCE_SRC)]);
    let (data, raised) = reduced_annotation_in(&host, SELF_REFERENCE, "indexedNextV");
    assert_eq!(data, SemanticNodeData::Primitive(PrimitiveKind::Number));
    assert_eq!(raised, number());
    let (data, raised) = reduced_annotation_in(&host, SELF_REFERENCE, "indexedNext");
    assert!(
        matches!(&data, SemanticNodeData::Object(_)),
        "`Chain['next']` reduces to `Chain`'s surface, got {data:?}"
    );
    assert_eq!(projected_member(&raised, "v"), &number());
    assert!(
        matches!(projected_member(&raised, "next"), TypeExpr::RecursiveRef { name, .. }
            | TypeExpr::Ref { name, .. } if name.as_ref() == "Chain"),
        "`next` is the self-reference, got {raised:?}"
    );
}

// ──────────────────────────────────────────────────────────────────────
// The checker's evolving array
// ──────────────────────────────────────────────────────────────────────

/// Unannotated declarations initialised to an empty array literal. Every
/// expected answer below is measured on tsc 7.0.2 over this module
/// (`--noEmit --strict --target es2022`, each type read through the tuple
/// wrapper quoted by TS2322).
const EVOLVING: &str = "/ws/cov/evolving_array.ts";
const EVOLVING_SRC: &str = r#"
export function readWhole() { const a = []; return a; }
export function readParenthesized() { const a = ([]); return a; }
export function readInElement() { const a = []; return [a]; }
export function readThroughLocal() { const a = []; const b = a; return b; }
export function readVar() { var a = []; return a; }
export function pushed() { const a = []; a.push(1); return a; }
export function reassigned() { let a = []; a = [1]; return a; }
export function captured() { const a = []; const f = () => a; return f(); }
export function annotated() { const a: number[] = []; return a; }
export function literal() { return []; }
"#;

fn array_of(element: TypeExpr) -> TypeExpr {
    TypeExpr::Array {
        element: Arc::new(element),
        readonly: false,
    }
}

/// An empty array literal initialising an unannotated declaration is the
/// checker's EVOLVING array under `noImplicitAny`: its type follows the
/// `push` / `unshift` / element writes / reassignments that reach each
/// read. A binding the frame only reads whole reads `any[]`; one some
/// position may evolve is not followed here, so it is the typed gap rather
/// than the `never[]` of a bare `[]`.
///
/// tsc 7.0.2 (`--strict`, each with TS7034 / TS7005 at the declaration and
/// the read): `readWhole`, `readThroughLocal` and `readVar` are `any[]`,
/// `readInElement` (`[a]`) is `any[][]`; `pushed` (`a.push(1)`) and
/// `reassigned` (`a = [1]`) are `number[]` and `captured` (read inside an
/// arrow) `any[]` — the three this frame does not follow. The controls:
/// `annotated` is `number[]`, a bare `return []` is `never[]`, and so is
/// `readParenthesized` — `const a = ([])` is not the evolving form (no
/// TS7034; the checker does not look through the parentheses).
#[test]
fn an_empty_array_initializer_is_the_checkers_evolving_array() {
    let host = host_with(&[(EVOLVING, EVOLVING_SRC)]);
    let any_array = array_of(any());
    for name in ["readWhole", "readThroughLocal", "readVar"] {
        assert_clean_warm(&host, EVOLVING, name, any_array.clone());
    }
    assert_clean_warm(&host, EVOLVING, "readInElement", array_of(any_array));
    for name in ["pushed", "reassigned", "captured"] {
        assert_degraded(
            &host,
            EVOLVING,
            name,
            FlowReturnDegradation::UnmodeledPosition,
        );
    }
    assert_clean_warm(&host, EVOLVING, "annotated", array_of(number()));
    for name in ["literal", "readParenthesized"] {
        assert_clean_warm(
            &host,
            EVOLVING,
            name,
            array_of(TypeExpr::Primitive(PrimitiveName::Never)),
        );
    }
}
