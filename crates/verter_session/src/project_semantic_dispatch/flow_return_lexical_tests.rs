//! @ai-generated - Lexical-authority regression tests for the demand-sliced
//! `FlowReturn` evaluator.
//!
//! Every case here is oracle-anchored against `tsgo 7.0.0-dev.20260526.1 --strict
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

// ── QUALIFIED (dotted) type references ────────────────────────────────
// `QE.M` / `QNS.Inner` / `QA.B.C` resolve their LEFTMOST segment as a
// binding; the rest are member selections inside whatever that binding
// denotes. A frame that declares the HEAD owns the whole reference, so
// the owner-scope answer is the wrong one.
export enum QE {
  M = "outer",
}

export namespace QNS {
  export type Inner = "outerNs";
}

export namespace QA {
  export namespace B {
    export type C = "outerABC";
  }
}

export function qNoShadow(x: unknown) {
  return x as QE.M;
}

export function qEnum(x: unknown) {
  enum QE {
    M = "inner",
  }
  return x as QE.M;
}

export function qNs(x: unknown) {
  namespace QNS {
    export type Inner = "innerNs";
  }
  return x as QNS.Inner;
}

export function qDeep(x: unknown) {
  namespace QA {
    export namespace B {
      export type C = "innerABC";
    }
  }
  return x as QA.B.C;
}

// ── The NAME-MEANING matrix ───────────────────────────────────────────
// A BARE `N` reference demands a Type meaning; the HEAD of a qualified
// `N.B` demands a Namespace meaning (`SymbolFlags.Namespace =
// ValueModule | NamespaceModule | Enum` — a class is NOT in it). The two
// are different questions about the same local declaration, and every
// row below is one oracle-anchored cell of the matrix.
export type BareOuter = "bareOuter";
export declare const defC: "outer";
export declare const localVal: "outerLocalVal";

// Qualified head, frame declares NO namespace ⇒ the module namespace is
// the right answer, cleanly and warm.
export function qClassHead(x: unknown) {
  class QNS {}
  return x as QNS.Inner;
}

export function qClassStaticHead(x: unknown) {
  class QNS {
    static Inner = 1;
  }
  return x as QNS.Inner;
}

export function qTypeAliasHead(x: unknown) {
  type QNS = 1;
  return x as QNS.Inner;
}

export function qInterfaceHead(x: unknown) {
  interface QNS {
    k: 1;
  }
  return x as QNS.Inner;
}

export function qFnHead(x: unknown) {
  function QNS() {}
  return x as QNS.Inner;
}

export function qConstHead(x: unknown) {
  const QNS = 1;
  return x as QNS.Inner;
}

export function qLetHead(x: unknown) {
  let QNS = 1;
  QNS = 2;
  return x as QNS.Inner;
}

export function qVarHead(x: unknown) {
  var QNS = 1;
  return x as QNS.Inner;
}

export function qParamHead(QNS: number, x: unknown) {
  return x as QNS.Inner;
}

// Qualified head, frame DOES declare a namespace ⇒ the module answer is
// the wrong one.
export function qConstEnumHead(x: unknown) {
  const enum QNS {
    Inner = "innerConstEnum",
  }
  return x as QNS.Inner;
}

export function qClassNsMergeHead(x: unknown) {
  class QNS {}
  namespace QNS {
    export type Inner = "innerClassNs";
  }
  return x as QNS.Inner;
}

export function qFnNsMergeHead(x: unknown) {
  function QNS() {}
  namespace QNS {
    export type Inner = "innerFnNs";
  }
  return x as QNS.Inner;
}

// Bare reference, frame declares NO type ⇒ the module alias is right.
export function bNamespaceLocal(x: unknown) {
  namespace BareOuter {
    export const inner = 1;
  }
  return x as BareOuter;
}

// Bare reference, frame DOES declare a type ⇒ the module alias is wrong.
export function bTypeAliasLocal(x: unknown) {
  type BareOuter = "innerAlias";
  return x as BareOuter;
}

export function bInterfaceLocal(x: unknown) {
  interface BareOuter {
    k: 1;
  }
  return x as BareOuter;
}

export function bClassNsMergeLocal(x: unknown) {
  class BareOuter {}
  namespace BareOuter {
    export type Inner = "innerClassNs";
  }
  return x as BareOuter;
}

// The CAPTURE half of the same matrix: a nested function value reads the
// enclosing frame's declarations through the capture scope, which must
// carry the two meanings separately.
export function capQualClass() {
  class QNS {}
  return (x: unknown) => x as QNS.Inner;
}

export function capNamespaceBare() {
  namespace BareOuter {
    export const inner = 1;
  }
  return (x: unknown) => x as BareOuter;
}

export function capNamespaceQual() {
  namespace QNS {
    export type Inner = "innerCapNs";
  }
  return (x: unknown) => x as QNS.Inner;
}

export function capTypeAlias() {
  type BareOuter = "innerCapAlias";
  return (x: unknown) => x as BareOuter;
}

// ── Declarator-annotation entrance ────────────────────────────────────
// `SliceStatement::Binding.declared` is a BODY position, so this frame's
// body-local type declarations ARE in scope in it. The initializer's own
// gate cannot stand in: a non-union declared type binds the DECLARED
// node and never evaluates the initializer.
export type AnnotOuter = "annotOuter";

export function annotDeclCtrl(x: unknown) {
  const v: AnnotOuter = x as any;
  return v;
}

export function annotDeclBare(x: unknown) {
  class AnnotOuter {
    inner = 1;
  }
  const v: AnnotOuter = x as any;
  return v;
}

export function annotDeclQualified(x: unknown) {
  enum QNS {
    Inner = "innerAnnotEnum",
  }
  const v: QNS.Inner = x as any;
  return v;
}

export function annotDeclTypeofValue(x: unknown) {
  const localVal = 1;
  const v: typeof localVal = x as any;
  return v;
}

// ── ROOT signature: body-locals are NOT in scope ──────────────────────
// `checker.ts::resolveName` discards a Type-meaning hit in a function's
// own `locals` when `lastLocation !== location.body`. These four must
// stay CLEAN and WARM — gating them trades a correct answer for a
// spurious fail-closed.
export type RootInfo = "rootInfo";

export function rootParamAnnot(p: RootInfo) {
  class RootInfo {
    inner = 1;
  }
  return p;
}

export function rootTypeParamConstraint<T extends RootInfo>(p: T) {
  class RootInfo {
    inner = 1;
  }
  return 1;
}

export function rootParamDefault(p = defC) {
  const defC = "inner" as const;
  return p;
}

// ── NESTED signature: the ENCLOSING frame's body-locals ARE in scope ──
export type NestOuter = "nestOuter";

export function nestedParamAnnot() {
  class NestOuter {
    inner = 1;
  }
  return (p: NestOuter) => p;
}

export function nestedRestAnnot() {
  class NestOuter {
    inner = 1;
  }
  return (...p: NestOuter[]) => p;
}

export function nestedTypeParamConstraint() {
  class NestOuter {
    inner = 1;
  }
  return function <T extends NestOuter>(p: T) {
    return p;
  };
}

export function nestedTypeParamDefault() {
  class NestOuter {
    inner = 1;
  }
  return function <T = NestOuter>(p: T) {
    return p;
  };
}

export function nestedParamDefaultInit() {
  const defC = "inner" as const;
  return (p = defC) => p;
}

// ── A nested signature's OWN type parameters are BINDERS ──────────────
// They shadow a captured same-named type-space declaration, so gating
// them against the enclosing frame is a spurious fail-closed.
export type T = "outerT";

export function tpShadowClass() {
  class T {
    inner = 1;
  }
  return <T,>(x: unknown) => x as T;
}

export function tpShadowEnum() {
  enum T {
    A,
  }
  return <T,>(x: unknown) => x as T;
}

export function tpShadowClassFnExpr() {
  class T {
    inner = 1;
  }
  return function <T>(x: unknown) {
    return x as T;
  };
}

export function tpShadowParamAnnot() {
  class T {
    inner = 1;
  }
  return function <T>(p: T) {
    return p;
  };
}

export function genuineCapture() {
  class GC {
    inner = 1;
  }
  return (x: unknown) => x as GC;
}

// ── Type-parameter binder COMPOSITION ─────────────────────────────────
// Every binder name below has a MODULE-scope twin — the leak bait. A
// `DeclRef` to the twin and a surviving `TypeParam` binder BOTH raise to
// `Ref { name }`, so every row asserts the GRAPH NODE.
export type OuterTP = "moduleOuterTP";
export type SameClause = "moduleSameClause";
export type DeepTP = "moduleDeepTP";
export type ModuleAlias = "moduleAlias";

export function bindNestedParam<OuterTP>() {
  return (p: OuterTP) => p;
}

export function bindNestedRest<OuterTP>() {
  return (...p: OuterTP[]) => p;
}

export function bindNestedConstraint<OuterTP>() {
  return function <U extends OuterTP>(p: U) {
    return p;
  };
}

export function bindNestedDefault<OuterTP>() {
  return function <U = OuterTP>(p?: U) {
    return p;
  };
}

export function bindNestedSameClause() {
  return function <SameClause, U extends SameClause>(a: SameClause, p: U) {
    return p;
  };
}

export function bindRootSameClause<SameClause, U extends SameClause>(p: U) {
  return p;
}

export function bindForwardSibling<U extends SameClause, SameClause>(
  u: U,
  v: SameClause,
) {
  return u;
}

export function bindDepthTwo<DeepTP>() {
  return () => {
    return (p: DeepTP) => p;
  };
}

export function bindNestedBody<OuterTP>() {
  return (x: unknown) => x as OuterTP;
}

export function bindNoTwin<FreshTP>() {
  return (p: FreshTP) => p;
}

export function bindInferCheck<OuterTP>() {
  return (x: unknown) => x as (OuterTP extends Array<infer U> ? U : never);
}

export function ctrlNestedConstraintModule() {
  return function <U extends ModuleAlias>(p: U) {
    return p;
  };
}

export function ctrlNestedDefaultModule() {
  return function <U = ModuleAlias>(p?: U) {
    return p;
  };
}

export function ctrlRootConstraintModule<U extends ModuleAlias>(p: U) {
  return p;
}

// ── NEAREST WINS across frames, in BOTH directions ────────────────────
// TS2300 forbids a binder and a same-named local only INSIDE one frame,
// so across frames the two genuinely coexist and the nearer one wins.
export type NestT = "moduleNestT";

export function localClassShadowsOuterBinder<NestT>() {
  return () => {
    class NestT {
      inner = 1;
    }
    return (x: unknown) => x as NestT;
  };
}

export type BindT = "moduleBindT";
export type BodyT = "moduleBodyT";

export function binderShadowsOuterLocalClass() {
  class BindT {
    inner = 1;
  }
  return function <BindT>() {
    return (p: BindT) => p;
  };
}

export function binderShadowsOuterLocalClassInBody() {
  class BodyT {
    inner = 1;
  }
  return function <BodyT>() {
    return (x: unknown) => x as BodyT;
  };
}

export type SameT = "moduleSameT";

export function localClassSameFrameShadowsOuterBinder<SameT>() {
  return () => {
    class SameT {
      inner = 1;
    }
    return null as unknown as SameT;
  };
}

// A CLASS binder and a member body's local are different scopes — no
// TS2300 — and the member's local WINS, in the body and in a nested
// function value inside it.
export type CT = "moduleCT";

export class ClassLocalShadow<CT> {
  m(x: unknown) {
    class CT {
      inner = 1;
    }
    return x as CT;
  }
  n(x: unknown) {
    class CT {
      inner = 1;
    }
    return (y: unknown) => y as CT;
  }
}

// ── The CLASS type-parameter clause ───────────────────────────────────
export class ClassTP<OuterTP> {
  m(x: OuterTP) {
    return x;
  }
  n(x: unknown) {
    return (p: OuterTP) => p;
  }
}

// ── The PARAMETER-LIST inventory ──────────────────────────────────────
export declare const pv: "modulePV";
export declare const loc: "moduleLoc";

export function paramTypeofRoot(pv: number, b: typeof pv) {
  return b;
}

export function paramTypeofNested() {
  return (pv: number, b: typeof pv) => b;
}

export function paramDefaultRoot(pv = 1, b = pv) {
  return b;
}

export function paramDefaultNested() {
  return (pv = 1, b = pv) => b;
}

export function paramTypeofCtrl(a: number, b: typeof pv) {
  return b;
}

export function paramTypeofNoTwin(nt: number, b: typeof nt) {
  return b;
}

export function paramBodyLocalInvisible(b: typeof loc) {
  const loc = 1;
  return b;
}

// ── A BLOCK-scoped local in the binder's OWN frame ────────────────────
// TS2300 constrains only a BODY-level collision. A BLOCK-level `class` /
// `enum` of the binder's name is LEGAL and WINS for everything the block
// encloses, so this frame's own clause cannot short-circuit ahead of
// this frame's own lexical authority.
export type BT = "moduleBT";
export type BE = "moduleBE";
export type BN = "moduleBN";
export type BQ = "moduleBQ";
export type BD = "moduleBD";
export type BC = "moduleBC";
export type CP = "moduleCP";
export type CN = "moduleCN";

export function blockClassVsOwnBinder<BT>() {
  {
    class BT {
      inner = 1;
    }
    return null as unknown as BT;
  }
}

export function blockClassNestedSig<BT>() {
  {
    class BT {
      inner = 1;
    }
    return (p: BT) => p;
  }
}

export function blockEnumVsOwnBinder<BE>() {
  {
    enum BE {
      A,
    }
    return null as unknown as BE;
  }
}

export function blockClassDeeper<BN>() {
  {
    class BN {
      inner = 1;
    }
    {
      return null as unknown as BN;
    }
  }
}

export function blockNamespaceQual<BQ>() {
  {
    enum BQ {
      Inner,
    }
    return null as unknown as BQ.Inner;
  }
}

export function blockClassOneFrameDown<BD>() {
  return () => {
    {
      class BD {
        inner = 1;
      }
      return null as unknown as BD;
    }
  };
}

export function bodyClassCollides<BC>() {
  class BC {
    inner = 1;
  }
  return null as unknown as BC;
}

export function ctrlRootParam<CP>() {
  return null as unknown as CP;
}

export function ctrlNoLocal<CN>() {
  {
    class NotCN {
      inner = 1;
    }
    return null as unknown as CN;
  }
}

// ── A GENERIC callee at a direct-call site ────────────────────────────
export function gcDeclT<GD>(x: GD): GD {
  return x;
}

export function gcFlowT<GF>(x: GF) {
  return x;
}

export function gcBareT<GB>(): GB {
  return null as unknown as GB;
}

export function gcPlain() {
  return "plain";
}

export const gcAliasT: <GA>(x: GA) => GA = function <GX>(x: GX) {
  return x;
};

export function gcDeclExplicit() {
  return gcDeclT<string>("a");
}

export function gcDeclInferred() {
  return gcDeclT("a");
}

export function gcFlowExplicit() {
  return gcFlowT<string>("a");
}

export function gcFlowInferred() {
  return gcFlowT("a");
}

export function gcBareExplicit() {
  return gcBareT<string>();
}

export function gcBareInferred() {
  return gcBareT();
}

export function gcViaAnnotated() {
  return gcAliasT<string>("a");
}

export function gcNonGeneric() {
  return gcPlain();
}

// The callee's clause is spelled with the SAME name as the enclosing
// class's, which is what makes the leak substitutable.
export function gcSameName<GH>(x: GH) {
  return x;
}

export class GcHolder<GH> {
  viaCall() {
    return gcSameName<string>("a");
  }
  ownT(): GH {
    return null as unknown as GH;
  }
}

namespace GcNs {
  export function nsT<GN>(x: GN): GN {
    return x;
  }
  export function nsPlain() {
    return "ns";
  }
  export function nsCall() {
    return nsT<string>("a");
  }
  export function nsPlainCall() {
    return nsPlain();
  }
}

// ── Every OTHER route from a callee's return to a caller ──────────────
// A generic callee reached through a LOCAL function-typed binding, an
// IIFE, and a generic function-typed PARAMETER. None of these is a
// direct-call obligation edge, and every one of them reads a signature
// node that carries the callee's own clause.
export function rvLocalLambdaCall() {
  const idL = <RL>(x: RL): RL => x;
  return idL("a");
}

export function rvIife() {
  return (<RI>(x: RI): RI => x)("a");
}

export function rvParamCall(fn: <RP>(x: RP) => RP) {
  return fn("a");
}

// The MEMBER-ALIASING bait: the class clause is spelled `RL`, exactly
// the local lambda's parameter name, so a leaked callee binder is
// substitutable by `new RvHolder<number>()`.
export class RvHolder<RL> {
  viaLocal() {
    const idL = <RL>(x: RL): RL => x;
    return idL("a");
  }
  ownRL(): RL {
    return null as unknown as RL;
  }
}

// ── A GENERIC mutual-recursion component ──────────────────────────────
// Both members are provisional when the component closes, so each one's
// contribution to the other arrives through the SCC fixed point rather
// than through the direct-call arm's own instantiation.
export function cgOne<CO>(x: CO, f: boolean) {
  if (f) return x;
  return cgTwo(x, f);
}

export function cgTwo<CT>(y: CT, f: boolean) {
  if (f) return y;
  return cgOne(y, f);
}

// The bait twin for the component: `CT` is also a class clause.
export class CgHolder<CT> {
  ownCT(): CT {
    return null as unknown as CT;
  }
}

// ── A clause name that SHADOWS a file-scope declaration ───────────────
// `ShItem` / `SH` name both a file-scope interface and a callee clause
// parameter. The callee's DECLARED return lowers in file OWNER scope,
// where the name RESOLVES to the interface — so the leak is a resolved
// `DeclRef`, not an unbound head.
export interface ShItem {
  id: string;
}

export function shFirst<ShItem>(xs: ShItem[]): ShItem {
  return xs[0];
}

export function shUseFirst(xs: number[]) {
  return shFirst(xs);
}

// CONTROL — the SAME interface name, reached through a NON-generic
// callee. Nothing declares `ShItem` as a clause parameter here, so the
// published `DeclRef` is the interface and must survive untouched.
export function shPlainItem(): ShItem {
  return { id: "a" };
}

export function shUsePlainItem() {
  return shPlainItem();
}

export interface SH {
  sh: string;
}

export function shDecl<SH>(x: SH): SH {
  return x;
}

export function callsShDecl() {
  return shDecl("a");
}

// ── An OVERLOAD group ─────────────────────────────────────────────────
// The visible overloads return `OA` / `string`; only the HIDDEN trailing
// implementation returns `any`.
export function ovX<OA>(x: OA): OA;
export function ovX(x: string, y: number): string;
export function ovX(x: any, y?: number): any {
  return x;
}

export function ovXCall() {
  return ovX("a");
}

export function ovSingle<OS>(x: OS): OS {
  return x;
}

export function ovSingleCall() {
  return ovSingle("a");
}

// ── A clause parameter carrying a DEFAULT ─────────────────────────────
export function rvDefaulted<GDF = number>(): GDF {
  return null as unknown as GDF;
}

export function rvDefaultedCall() {
  return rvDefaulted();
}

// The DEFAULT (`string`) is deliberately DIFFERENT from what inference
// would produce (`number`), so a row that took the default anyway is
// distinguishable from one that inferred — the shipped fixture had them
// coincide and passed for the wrong reason.
export function rvDefaultedFlow<GDN = string>(x: GDN) {
  return x;
}

export function rvDefaultedFlowCall() {
  return rvDefaultedFlow(1);
}

// The BODY-DERIVED route's default row: argument-free, so nothing can be
// inferred and the declared default is the exact answer.
export function rvDefaultedFlowFree<GDW = number>() {
  return null as unknown as GDW;
}

export function rvDefaultedFlowFreeCall() {
  return rvDefaultedFlowFree();
}

// ── A DEFAULT the call site does NOT get to use ───────────────────────
// TypeScript applies a type-parameter default ONLY when inference
// produces no candidate. Every row here supplies an argument at an
// ordinal whose parameter type names the parameter, so the checker
// infers and the default never applies — the recorded interim is
// `unknown`, never the default.
export function zzMismA<ZA = string>(x: ZA) {
  return x;
}

export function zzMismACall() {
  return zzMismA(1);
}

export function zzMismB<ZB = string>(x: ZB): ZB {
  return x;
}

export function zzMismBCall() {
  return zzMismB(true);
}

export function zpExplicit<ZC = number>(): ZC {
  return null as unknown as ZC;
}

export function zpExplicitCall() {
  return zpExplicit<string>();
}

// The COMPLEMENT: an argument-bearing call whose clause parameter can
// still get no candidate, so the checker DOES take the default.
export function zpUnusedTP<ZE = number>(x: string): ZE {
  return null as unknown as ZE;
}

export function zpUnusedTPCall() {
  return zpUnusedTP("a");
}

export function zpNotSupplied<ZF = number>(a: string, b?: ZF): ZF {
  return null as unknown as ZF;
}

export function zpNotSuppliedCall() {
  return zpNotSupplied("a");
}

// ── A same-named FOREIGN declaration reached through a callee ─────────
// `QQ` names BOTH a file-scope interface and `aye`'s clause parameter.
// `bee`'s DECLARED return is the interface — a different symbol `aye`'s
// clause never shadows — so the caller's clause instantiation must not
// claim it.
export interface QQ {
  q: string;
}

export function bee(): QQ {
  return null as unknown as QQ;
}

export function aye<QQ>(x: QQ, f: boolean) {
  if (f) return bee();
  return x;
}

export function callAye() {
  return aye(1, true);
}

// The same shape with a GENERIC callee: `beeG` declares its own clause
// (`RR`), so callee genericity is no longer held constant at
// non-generic — the control the shipped suite lacked.
export function beeG<RR>(x: RR): QQ {
  return null as unknown as QQ;
}

export function ayeG<QQ>(x: QQ, f: boolean) {
  if (f) return beeG(x);
  return x;
}

export function callAyeG() {
  return ayeG(1, true);
}

// A foreign same-named declaration NESTED inside a structural callee
// return.
export interface ZItem {
  z: string;
}

export function zInner(): { v: ZItem } {
  return null as unknown as { v: ZItem };
}

export function zOuter<ZItem>() {
  return zInner();
}

export function zOuterCall() {
  return zOuter();
}

// The IIFE route's version: the nested function value's signature is
// COMPOSED from its own body, so the foreign interface reached through
// it is a different symbol from the IIFE's own clause parameter.
export declare const qqValue: QQ;

export function iifeForeign() {
  return (<QQ,>(f: boolean) => (f ? qqValue : null))(true);
}

// ── ONE callee, TWO routes: the binding and the IIFE ──────────────────
// `nbUse` and `nbIife` have the SAME callee body; only the route to it
// differs (a local `const` binding vs. an immediate invocation). Both
// reach a signature COMPOSED from that body, so both must answer alike.
// `ncUse` is the control: the identical body with the clause RENAMED,
// which removes the name collision without changing anything else.
export interface NB {
  nb: string;
}

export declare const nbValue: NB;

export function nbUse(k: boolean) {
  const g = <NB,>(j: boolean) => (j ? nbValue : null);
  return g(true);
}

export function nbIife(k: boolean) {
  return (<NB,>(j: boolean) => (j ? nbValue : null))(true);
}

export function ncUse(k: boolean) {
  const g = <ZZ,>(j: boolean) => (j ? nbValue : null);
  return g(true);
}

// ── Call forms reached through a COMPOSITE expression ─────────────────
// A call in a TERNARY arm is a call: the branch has a structural arm, so
// the callee's clause is instantiated and an overload group reached
// through one degrades exactly as it does at a bare call.
//
// A call in a LOGICAL / NULLISH / SEQUENCE operand, under a non-null
// assertion, or as a MEMBER BASE does NOT: those forms are `Leaf` to the
// shared descent, and the shallow leaf pass answers each of them `any`
// BEFORE the call-carrier gate can refuse anything. They publish
// `Primitive(Any)` cleanly and warm — under the checker's answer, never
// over it, and never the callee's raw return carrier — and the rows
// below assert exactly that, so a change that starts routing any of them
// through the call sink has to say so here.
export declare function tnAmb(x: string): "TA";
export declare function tnAmb(x: number): "TB";

export function tnAmbBare(k: boolean) {
  return tnAmb("a");
}

export function tnAmbIf(k: boolean) {
  if (k) return tnAmb("a");
  return tnAmb(1);
}

export function tnAmbTernary(k: boolean) {
  return k ? tnAmb("a") : tnAmb(1);
}

export function tnAmbLogical(k: boolean) {
  return k || tnAmb("a");
}

export function tnAmbNullish(k: null) {
  return k ?? tnAmb("a");
}

export function tnAmbSequence(k: boolean) {
  return (k, tnAmb("a"));
}

export function tnAmbNonNull(k: boolean) {
  return tnAmb("a")!;
}

export function tnAmbArray(k: boolean) {
  return [tnAmb("a")];
}

export function tnAmbAs(k: boolean) {
  return tnAmb("a") as "TA";
}

export function tnGeneric<TNG>(x?: TNG): TNG {
  return null as unknown as TNG;
}

export function tnGenericBare() {
  return tnGeneric<string>("s");
}

export function tnGenericTernary(k: boolean) {
  return k ? tnGeneric<string>("s") : tnGeneric<number>(1);
}

export function tnGenericLogical(k: boolean) {
  return k || tnGeneric<string>("s");
}

export function tnGenericArray() {
  return [tnGeneric<string>("s")];
}

export function tnLitTernary(k: boolean) {
  return k ? 1 : 2;
}

export function tnLitIf(k: boolean) {
  if (k) return 1;
  return 2;
}

export function tnStrTernary(k: boolean) {
  return k ? "a" : "b";
}

export function tnGenericMember() {
  return tnGeneric<{ q: string }>({ q: "a" }).q;
}

// ── METHOD-position overload groups ───────────────────────────────────
// An overload SET in method position retains every contributor on the
// surface, but a member projection is first-wins — so the call rail saw
// ONE signature and published the FIRST overload's return warm.
export declare class OvClass {
  m(x: string): "MA";
  m(x: number): "MB";
}
export declare const ovClassValue: OvClass;

export function ovClassMethodCall() {
  return ovClassValue.m(1);
}

export declare const ovObjValue: { m(x: string): "PA"; m(x: number): "PB" };

export function ovObjMethodCall() {
  return ovObjValue.m(1);
}

export interface OvIface {
  m(x: string): "IA";
  m(x: number): "IB";
}
export declare const ovIfaceValue: OvIface;

export function ovIfaceMethodCall() {
  return ovIfaceValue.m(1);
}

export declare const ovIntersectValue: OvClass & { z: 1 };

export function ovIntersectMethodCall() {
  return ovIntersectValue.m(1);
}

// CONTROL — a LONE method is not an overload group and must keep its
// exact answer.
export declare const ovSoloValue: { only(x: number): "SOLO" };

export function ovSoloMethodCall() {
  return ovSoloValue.only(1);
}

// CONTROL — a plain PROPERTY is projected first-wins as before.
export declare const ovPropValue: { p: "PROP" };

export function ovPropRead() {
  return ovPropValue.p;
}

// ── The OWNER-SCOPE routes' clause shadowing ──────────────────────────
// `SymItem` names both a file-scope interface and a clause parameter of
// a DECLARED function TYPE. Both routes below take their value from a
// callee type lowered in FILE OWNER SCOPE, where the clause is
// invisible — so the resolved head IS the clause parameter, and the
// claim must reach it.
export interface SymItem {
  s: string;
}

export const symFn: <SymItem>(x: SymItem) => SymItem = (x) => x;

export function symCall() {
  return symFn("a");
}

export function bindDeclCall(fn: <SymItem>(x: SymItem) => SymItem) {
  return fn("a");
}

// ── An AMBIENT overload group ─────────────────────────────────────────
// No implementation, so "the trailing signature has a body" never fires
// — yet picking the right overload still needs argument-driven overload
// resolution, and the index's single entry is the LAST declaration while
// the language picks the FIRST match.
export declare function amb3(x: string): "A";
export declare function amb3(x: number): "B";
export declare function amb3(x: boolean): "C";

export function amb3Call() {
  return amb3("a");
}

// ── The `ReturnType<typeof …>` MEMBER route ───────────────────────────
// A signature UTILITY, whose clause policy is `unknown` for every free
// parameter — the whole-return route's policy, which the member route
// has to share or the two disagree about one callee.
export function mpFlow<MG = number>(x: MG) {
  return { m: x };
}

export function mpWholeUse(x: { w: ReturnType<typeof mpFlow> }) {
  return x;
}

export function mpMemberUse(x: ReturnType<typeof mpFlow>["m"]) {
  return x;
}

// ── A conditional expression's branch STRUCTURE ───────────────────────
// The content half descends into both branches; the demand PLANNER must
// descend through the very same forms, or an object literal in a branch
// lowers with every member value `Elided` and the whole return fails
// closed.
export function ctObj(k: boolean) {
  return k ? { a: 1 } : 2;
}

export function ctObjBoth(k: boolean) {
  return k ? { a: 1 } : { a: 2 };
}

export function ctObjDisjoint(k: boolean) {
  return k ? { a: 1 } : { b: 2 };
}

export function ctObjLocalRead(k: boolean) {
  const q = 1;
  return k ? { a: q } : 2;
}

export function ctObjMethod(k: boolean) {
  return k
    ? {
        m() {
          return 1;
        },
      }
    : 2;
}

export function ctObjNested(k: boolean) {
  return k ? { a: { b: 1 } } : 2;
}

export function ctObjInObj(k: boolean) {
  return { a: k ? { b: 1 } : 2 };
}

export function ctNestedTernary(k: boolean) {
  return k ? (k ? { a: 1 } : 2) : 3;
}

export function ctObjEmpty(k: boolean) {
  return k ? {} : 2;
}

export function ctArray(k: boolean) {
  return k ? [1] : [2];
}

export function ctIdent(k: boolean) {
  return k ? k : 2;
}

export function ctNull(k: boolean) {
  return k ? null : 1;
}

export function ctArrow(k: boolean) {
  return k ? () => 1 : () => 2;
}

// ── Self-recursion through a ternary ──────────────────────────────────
// The `if` spelling holds coinductively and converges; the ternary
// spelling must reach the SAME fixed point, not convert the hold into a
// whole-evaluation failure.
export function ctRec(n: number) {
  return n > 0 ? ctRec(n - 1) : 0;
}

export function ctRecIf(n: number) {
  if (n > 0) return ctRecIf(n - 1);
  return 0;
}

export function ctRecObjMember(n: number) {
  return { a: n > 0 ? ctRecObjMember(n - 1) : 0 };
}

// ── A BODIED method overload group ────────────────────────────────────
// The trailing implementation signature is HIDDEN by TypeScript. The
// shared PathWalker member hop must apply the same overload-visibility
// rule `build_typeof` applies to a top-level function group, or the
// published group leaks `(x: any): any` and every signature utility
// reads it.
export class OvImpl {
  m(x: string): "MA";
  m(x: number): "MB";
  m(x: any): any {
    return x;
  }
}

export function ovImplGroup(x: OvImpl["m"]) {
  return x;
}

export function ovImplReturn(x: ReturnType<OvImpl["m"]>) {
  return x;
}

export function ovImplParams(x: Parameters<OvImpl["m"]>) {
  return x;
}

// CONTROL — the same group with NO implementation keeps every declared
// signature visible.
export declare class OvAmbient {
  m(x: string): "AA";
  m(x: number): "AB";
}

export function ovAmbientGroup(x: OvAmbient["m"]) {
  return x;
}

// CONTROL — a LONE BODIED method is visible (the lone-signature carve-out
// `build_typeof` applies).
export class OvLoneImpl {
  m(x: string): "LA" {
    return "LA";
  }
}

export function ovLoneImplGroup(x: OvLoneImpl["m"]) {
  return x;
}

// CONTROL — ONE declared overload plus an implementation: exactly one
// signature is visible, so the group carrier holds one contributor.
export class OvSingleImpl {
  m(x: string): "OA";
  m(x: any): any {
    return x;
  }
}

export function ovSingleImplGroup(x: OvSingleImpl["m"]) {
  return x;
}

// The signature UTILITIES over the same carrier, forced to MATERIALIZE by
// a projection ON the utility's result — the end-to-end route
// `select_signature_function` serves.
export function ovImplReturnProj(x: ReturnType<OvImpl["m"]>["length"]) {
  return x;
}

export function ovImplParamsProj(x: Parameters<OvImpl["m"]>[0]) {
  return x;
}

export function ovAmbientReturnProj(x: ReturnType<OvAmbient["m"]>["length"]) {
  return x;
}

// ── PARKED pre-existing defects (see the `#[ignore]`d rows below) ──────
// Each fixture characterizes a defect this substrate does NOT own. They
// are authored here so the parked test has a real body to fail with.
export declare class HbBase {
  hb(x: string): "BASE";
}
export declare class HbDerived extends HbBase {
  hb(x: string): "BASE";
}
export declare const hbDerivedValue: HbDerived;

export function hbClassCall() {
  return hbDerivedValue.hb("a");
}

export interface EbBase {
  eb(x: string): "EB";
}
export interface EbDerived extends EbBase {
  eb(x: string): "EB";
}
export declare const ebDerivedValue: EbDerived;

export function ebIfaceCall() {
  return ebDerivedValue.eb("a");
}

export declare class GaClass {
  get ga(): "GA";
  set ga(v: "GA");
}
export declare const gaValue: GaClass;

export function gaRead() {
  return gaValue.ga;
}

export function undefTernary(k: boolean) {
  return k ? undefined : 1;
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
    r5_key_part(dispatch, name, FunctionPartIdentity::DeclarationBody)
}

fn r5_key_part(
    dispatch: &ProjectSemanticDispatch<'_>,
    name: &str,
    part: FunctionPartIdentity,
) -> FlowReturnKey {
    FlowReturnKey {
        function: dispatch.flow_function_slot_for(
            Arc::from(R5_CANONICAL),
            verter_type_expr::TopLevelOwnerId::ordinary_file(),
            Arc::from(name),
            part,
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
            .project_node_to_type_expr_for_test(result.return_type())
            .expect("a flow return value projects");
        let candidates = dispatch
            .graph()
            .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key)));
        Some(R5Outcome {
            ty,
            degradation: result.degradation(),
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

/// Assert one function FAILS CLOSED — it admits NOTHING, and it publishes
/// no fabricated value.
///
/// Two shapes satisfy that, and the helper discriminates rather than
/// accepting either loosely:
///
/// - a POSITIONAL non-modelling (a call form with no structural arm, a
///   frame-local binding the flow content does not model) is a DEGRADED
///   SUCCESS whose value REACHES the typed unresolved MARKER — at the
///   root, or inside the structure the evaluation composed around it (a
///   nested function value's signature keeps its shape and carries the
///   marker at the unmodelled slot). The reach is asserted, so a leak to
///   the module-scope twin — the exact defect these rows exist to catch —
///   still fails, because a `DeclRef` reaches nothing;
/// - a whole-frame NO-VALUE failure (an unmodelled control surface, a
///   missing body, a torn view) is `Error(Miss)` with no value at all.
///
/// The composite-position TWIN of each positional class — the same
/// unmodelled position INSIDE a structure with a modelled sibling — lives
/// in `flow_return_positional_tests`, where the enclosing structure is
/// asserted to SURVIVE with the marker in place.
#[track_caller]
fn assert_fails_closed(host: &Arc<VerterHost>, name: &str) {
    with_dispatch(host, |dispatch| {
        let key = r5_key(dispatch, name);
        assert_flow_fails_closed(
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

/// The shared fail-closed discriminator, over one dispatch outcome.
#[track_caller]
pub(crate) fn assert_flow_fails_closed(
    dispatch: &ProjectSemanticDispatch<'_>,
    name: &str,
    outcome: QueryResult<SemanticQueryOutput<SemanticQueryValue>>,
) {
    match outcome {
        QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) => {
            assert!(
                dispatch
                    .graph()
                    .node_reaches_unresolved(result.return_type()),
                "{name}: a positional non-modelling REACHES the typed unresolved MARKER — \
                 at the root, or inside the structure the evaluation composed around it. \
                 Never a fabricated value and never the module-scope twin (which is a \
                 `DeclRef` and reaches nothing) — got {:?}",
                dispatch.graph().node_data(result.return_type())
            );
            assert!(
                result.degradation().is_some(),
                "{name}: a marker value is a DEGRADED success, never clean"
            );
        }
        QueryResult::Error(QueryError::Miss) => {}
        other => panic!("{name} must fail closed; got {other:?}"),
    }
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
/// nested frame with every enclosing parameter by name) — tsgo 7.0.0-dev.20260526.1:
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
/// tsgo 7.0.0-dev.20260526.1: `r5BlockLetShadowsParam(p: string): number`.
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
/// tsgo 7.0.0-dev.20260526.1 (`--strict`): each of these is `number` (the loop / if /
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
/// fail-close. tsgo 7.0.0-dev.20260526.1: `r5UsingInLoop(f: boolean): number`.
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
/// where tsgo 7.0.0-dev.20260526.1 says `1 | 2`.
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
/// initializer's literal, never the widened initializer. tsgo 7.0.0-dev.20260526.1:
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
/// the initializer's own (fresh or widened) type. tsgo 7.0.0-dev.20260526.1: `string`,
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
/// like any other: a single fresh literal widens. tsgo 7.0.0-dev.20260526.1:
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
/// `undefined` arm), and only a lone contributor widens. tsgo 7.0.0-dev.20260526.1:
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
/// does. tsgo 7.0.0-dev.20260526.1: `{ b: number }` and `{ b: 1 }`.
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
/// `[1]` from `[1 as const]`. tsgo 7.0.0-dev.20260526.1:
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
/// widens. tsgo 7.0.0-dev.20260526.1: `r5ConditionalReturn(c): 1 | 2`,
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
/// assertion — even to the literal type itself — PINS. tsgo 7.0.0-dev.20260526.1:
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
/// first. tsgo 7.0.0-dev.20260526.1: `1` for BOTH orders.
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
/// tsgo 7.0.0-dev.20260526.1: `r5CapturedWideningConst(): { a: number; b: () => number }`.
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

/// A QUALIFIED type reference is owned by its HEAD segment.
///
/// `x as QE.M` references exactly one binding — `QE`. The trailing
/// segments are member selections INSIDE whatever `QE` denotes, never
/// separate scope lookups. So when the frame declares `QE`, the whole
/// reference belongs to the frame and the owner-scope answer is wrong.
///
/// tsgo `7.0.0-dev.20260526.1` `--strict --declaration
/// --emitDeclarationOnly`, on exactly
/// these bodies:
///
/// ```text
/// qNoShadow(x: unknown): QE.M     // the module enum
/// qEnum(x: unknown): QE           // + TS4060 "private name 'QE'" — the LOCAL enum
/// qNs(x: unknown): "innerNs"      // the LOCAL namespace
/// qDeep(x: unknown): "innerABC"   // the LOCAL nested namespace
/// ```
///
/// The `qEnum` row is proven local by assignability, not just by the
/// printed name: `const w: import("./q").QE = qEnum(0)` is `TS2322 Type
/// 'QE' is not assignable to type 'import("…").QE'`.
///
/// Mutation recipe: pushing the DOTTED name into `ReferencedNames`
/// instead of its head (the pre-fix `recursive_traversal` `Ref` /
/// `RecursiveRef` arms) makes `"QE.M"` compare against binding names,
/// which only ever hold `"QE"` — so no frame binding matches, the gate
/// never fires, and all three shadowed rows publish the owner scope's
/// answer CLEAN and WARM. The `qNoShadow` row is the value control: it
/// pins that taking the head leaves an UNSHADOWED qualified reference's
/// resolved answer (the module enum member's literal) untouched, so the
/// gate cannot be widened into a blanket fail-closed on every qualified
/// reference.
#[test]
fn flow_return_qualified_type_reference_is_owned_by_its_head_segment() {
    let host = make_r5_host();

    // Control: no local `QE` at all, so the module enum member governs
    // and the answer stays clean + warm. This is the over-fire guard —
    // a head split that fires on an unshadowed name breaks this row.
    assert_clean_warm(&host, "qNoShadow", string_lit("outer"));

    // The frame declares the HEAD in type space (`enum` / `namespace`,
    // both unconditionally unmodelable), so every one of these must fail
    // closed rather than publish the module-scope answer.
    for name in ["qEnum", "qNs", "qDeep"] {
        assert_fails_closed(&host, name);
    }
}

/// A `TypeExpr::Ref` to `name` with no type arguments — how a shallow
/// published answer names a module-scope type.
fn type_ref(name: &str) -> TypeExpr {
    TypeExpr::Ref {
        name: Arc::from(name),
        type_arguments: Arc::from(Vec::new().into_boxed_slice()),
    }
}

/// One evaluated function's nested function value's return type.
#[track_caller]
fn nested_return(host: &Arc<VerterHost>, name: &str) -> TypeExpr {
    let outcome = r5_eval(host, name).unwrap_or_else(|| panic!("{name} must produce a value"));
    assert_eq!(outcome.degradation, None, "{name} must evaluate clean");
    assert_eq!(
        outcome.candidates, 1,
        "{name} must warm-admit exactly one candidate"
    );
    function_return(&outcome.ty).clone()
}

// ──────────────────────────────────────────────────────────────────────
// The NAME-MEANING matrix
// ──────────────────────────────────────────────────────────────────────

/// A type-space reference's MEANING selects which local declarations
/// shadow it, and the meaning is a property of the OCCURRENCE.
///
/// TypeScript resolves a BARE `N` with a Type meaning and the HEAD of a
/// qualified `N.B` with a Namespace meaning
/// (`SymbolFlags.Namespace = ValueModule | NamespaceModule | Enum` — a
/// class is NOT in it). The two questions have genuinely different
/// answers for the same declaration, so one verdict per name cannot be
/// right for both:
///
/// | local declaration      | bare shadows | qualified head shadows |
/// |------------------------|--------------|------------------------|
/// | `class` (± statics)    | yes          | NO                     |
/// | `enum` / `const enum`  | yes          | yes                    |
/// | `namespace`            | NO           | yes                    |
/// | `type` / `interface`   | yes          | NO                     |
/// | value-only kinds       | NO           | NO                     |
/// | `class` + `namespace`  | yes          | yes                    |
/// | `function` + `namespace` | NO         | yes                    |
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, checker not emitter — `const c: "outerNs" =
/// qClassHead(0)` and friends): every row below assigns cleanly exactly
/// when the module declaration is the answer, and reports TS2322 exactly
/// when the local one is.
///
/// Mutation recipes, each flipping a DIFFERENT row:
///   * a meaning-BLIND predicate (any type-space kind answers both
///     questions) flips `qClassHead` / `qTypeAliasHead` /
///     `qInterfaceHead` to `Error(Miss)` AND `bNamespaceLocal` to a warm
///     wrong answer;
///   * dropping the per-occurrence `qualified` bit from
///     `verter_type_expr::referenced_names` flips the same rows
///     identically (the head is then always asked as a bare Type);
///   * dropping `SkeletonBindingKind::{TypeAlias, Interface}` flips
///     `bTypeAliasLocal` / `bInterfaceLocal` to a warm wrong answer.
#[test]
fn flow_return_type_space_meaning_is_selected_per_reference_occurrence() {
    let host = make_r5_host();
    let outer_ns = || type_ref("QNS.Inner");
    let bare_outer = || type_ref("BareOuter");

    // QUALIFIED head, local declares no NAMESPACE ⇒ the module namespace
    // is the right answer and must publish clean and warm.
    for name in [
        "qClassHead",
        "qClassStaticHead",
        "qTypeAliasHead",
        "qInterfaceHead",
        "qFnHead",
        "qConstHead",
        "qLetHead",
        "qVarHead",
        "qParamHead",
    ] {
        assert_clean_warm(&host, name, outer_ns());
    }

    // QUALIFIED head, local DOES declare a namespace ⇒ fail closed.
    for name in [
        "qEnum",
        "qNs",
        "qDeep",
        "qConstEnumHead",
        "qClassNsMergeHead",
        "qFnNsMergeHead",
    ] {
        assert_fails_closed(&host, name);
    }

    // BARE reference, local declares no TYPE ⇒ the module alias stands.
    assert_clean_warm(&host, "bNamespaceLocal", bare_outer());

    // BARE reference, local DOES declare a type ⇒ fail closed. `type` and
    // `interface` are the two kinds the skeleton did not index at all.
    for name in ["bTypeAliasLocal", "bInterfaceLocal", "bClassNsMergeLocal"] {
        assert_fails_closed(&host, name);
    }

    // The CAPTURE half carries the two meanings as SEPARATE inventories:
    // a captured `class` owns the bare question but not the qualified
    // one, a captured `namespace` the reverse.
    assert_eq!(nested_return(&host, "capQualClass"), outer_ns());
    assert_eq!(nested_return(&host, "capNamespaceBare"), bare_outer());
    assert_fails_closed(&host, "capNamespaceQual");
    assert_fails_closed(&host, "capTypeAlias");
}

// ──────────────────────────────────────────────────────────────────────
// The producer ENTRANCE class
// ──────────────────────────────────────────────────────────────────────

/// A declarator's authored annotation is a BODY position: this frame's
/// body-local type declarations ARE in scope in it.
///
/// The annotation is minted by the shared shallow pass, which resolves
/// every name in FILE-OWNER scope, and the initializer's own gate cannot
/// stand in for it: with a NON-UNION declared type the evaluator binds
/// the DECLARED node and never evaluates the initializer at all.
///
/// Oracle (tsgo checker): `annotDeclCtrl` is `"annotOuter"` (the module
/// alias); `annotDeclBare` is the local class instance type,
/// `annotDeclQualified` the local enum member, `annotDeclTypeofValue`
/// the local `1` — none of which any owner-scope resolution can supply.
///
/// Mutation recipe: leaving `SliceStatement::Binding.declared` ungated
/// flips all three leak rows to a warm wrong answer (`deg=None`,
/// `cands=1`) carrying the module symbol, while `annotDeclCtrl` is
/// unaffected either way; gating it against the VALUE inventory instead
/// flips `annotDeclCtrl` to `Error(Miss)`.
#[test]
fn flow_return_declarator_annotation_resolves_through_the_frame() {
    let host = make_r5_host();
    assert_clean_warm(&host, "annotDeclCtrl", type_ref("AnnotOuter"));
    for name in [
        "annotDeclBare",
        "annotDeclQualified",
        "annotDeclTypeofValue",
    ] {
        assert_fails_closed(&host, name);
    }
}

/// A function's OWN signature does NOT see its body-local declarations;
/// a NESTED function value's signature DOES see the enclosing frame's.
///
/// `checker.ts::resolveName` discards a Type-meaning hit in a
/// function-like `location`'s `locals` whenever `lastLocation !==
/// location.body` (the value side mirrors it in
/// `useOuterVariableScopeInParameter`). So the root rows below must stay
/// CLEAN and WARM on the OUTER declaration, and the nested rows — whose
/// signatures sit INSIDE the enclosing body — must fail closed on it.
///
/// Oracle (tsgo checker, on exactly these bodies):
///
/// ```text
/// rootParamAnnot(p: RootInfo): "rootInfo"          // the MODULE alias
/// rootTypeParamConstraint<T extends RootInfo>: number
/// rootParamDefault(p?: "outer"): "outer"           // the MODULE const
/// nestedParamAnnot(): (p: { inner: number }) => …   // the LOCAL class
/// nestedRestAnnot(): (...p: { inner: number }[]) => …
/// nestedTypeParamConstraint(): <T extends { inner: number }>(p: T) => T
/// nestedTypeParamDefault(): <T = { inner: number }>(p: T) => T
/// nestedParamDefaultInit(): (p?: "inner") => "inner" // the LOCAL const
/// ```
///
/// Mutation recipes: gating the two ROOT call sites (passing
/// `SignatureScope::Nested` in `build_flow_slice_content`) flips all
/// three root controls to `Error(Miss)`; leaving ANY single nested
/// entrance ungated flips exactly its row to a warm wrong answer
/// (`deg=None`, `cands=1`) naming the module symbol.
#[test]
fn flow_return_signature_entrances_follow_the_root_versus_nested_scope_rule() {
    let host = make_r5_host();

    // ROOT signature — the OUTER declaration is the correct answer.
    assert_clean_warm(&host, "rootParamAnnot", type_ref("RootInfo"));
    assert_clean_warm(&host, "rootTypeParamConstraint", number());
    assert_clean_warm(&host, "rootParamDefault", string_lit("outer"));

    // NESTED signature — the enclosing frame's body-locals shadow it, and
    // no owner-scope answer can be right.
    for name in [
        "nestedParamAnnot",
        "nestedRestAnnot",
        "nestedTypeParamConstraint",
        "nestedTypeParamDefault",
        "nestedParamDefaultInit",
    ] {
        assert_fails_closed(&host, name);
    }
}

/// A nested function's OWN type parameters are BINDERS of its signature
/// and body, not references into the enclosing frame.
///
/// The evaluator's nested binder environment interns exactly those names,
/// so an answer naming one resolves to the binder — a captured same-named
/// `class` / `enum` does not shadow it and reporting it frame-bound is a
/// spurious fail-closed. A capture with NO same-named binder still fails
/// closed.
///
/// Oracle (tsgo): `tpShadowClass` / `tpShadowEnum` /
/// `tpShadowClassFnExpr` / `tpShadowParamAnnot` are all
/// `<T>(…) => T`; `genuineCapture` is `(x: unknown) => { inner: number }`
/// — the local class, which owner scope cannot supply.
///
/// Mutation recipe: dropping the binder subtraction in
/// `Lowerer::name_is_frame_bound` flips all four `tpShadow*` rows to
/// `Error(Miss)`; masking in NAMESPACE meaning as well as TYPE meaning
/// leaves them green but is unsound (a type parameter denotes no
/// namespace); dropping the binder frames from
/// `verter_type_expr::referenced_names` leaves these rows green (their
/// answers are bare `Ref`s, not function types) but flips
/// `referenced_names_masks_function_and_constructor_binders_depth_safely`.
#[test]
fn flow_return_nested_signature_type_parameter_shadows_captured_type_space_name() {
    let host = make_r5_host();
    for name in [
        "tpShadowClass",
        "tpShadowEnum",
        "tpShadowClassFnExpr",
        "tpShadowParamAnnot",
    ] {
        let returned = nested_return(&host, name);
        // The projected surface CANNOT discriminate here: a `DeclRef` to
        // the module-scope `type T = "outerT"` bait raises to the SAME
        // `Ref { name: "T" }` a surviving binder does. Only the binder
        // node is the correct answer.
        assert!(
            matches!(&returned, TypeExpr::TypeParameter(tp) if tp.name == "T"),
            "{name}: the nested binder `T` must survive, got {returned:?}"
        );
    }
    assert_fails_closed(&host, "genuineCapture");
}

// ──────────────────────────────────────────────────────────────────────
// Type-parameter binder COMPOSITION — asserted on the GRAPH NODE
// ──────────────────────────────────────────────────────────────────────

/// The discriminating GRAPH-NODE shape of one answer.
///
/// The PROJECTED surface cannot tell these apart: a surviving
/// `TypeParam` binder, a `DeclRef` to a module-scope declaration of the
/// same name, and a deferred `BareRef` ALL raise to
/// `TypeExpr::Ref { name }`. Every binder row asserts here instead —
/// asserting on the projection is exactly how a leak stays invisible.
#[derive(Debug, PartialEq, Eq)]
enum NodeShape {
    TypeParam(String),
    DeclRef(String),
    BareRef(String),
    Primitive(PrimitiveKind),
    Opaque,
    Other(String),
}

fn type_param(name: &str) -> NodeShape {
    NodeShape::TypeParam(name.to_string())
}

fn decl_ref(name: &str) -> NodeShape {
    NodeShape::DeclRef(name.to_string())
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

/// The `object` of an `IndexedAccess` answer.
#[track_caller]
fn indexed_access_object(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> SemanticNodeId {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::IndexedAccess { object, .. }) => *object,
        other => panic!("expected an IndexedAccess answer, got {other:?}"),
    }
}

/// Drive the shared PathWalker over `base` with one named segment, in
/// the PUBLISHED / EXPANDED context every consumer of a projected member
/// reaches this rail through.
#[track_caller]
fn project_member_path(
    dispatch: &ProjectSemanticDispatch<'_>,
    base: SemanticNodeId,
    key: &str,
) -> SemanticNodeId {
    let path: Arc<[crate::semantic_query::PathSegment]> = Arc::from(
        vec![crate::semantic_query::PathSegment::Member(
            crate::semantic_query::PropertyKey::identifier(key),
        )]
        .into_boxed_slice(),
    );
    match dispatch.execute_type_node(SemanticQueryKey::ProjectPath {
        base,
        path,
        context: crate::semantic_query::ProjectionReductionContext::published(
            crate::semantic_query::ProjectionMode::Expanded,
        ),
    }) {
        QueryResult::Value(out) => out.value,
        other => panic!("the path projection must resolve, got {other:?}"),
    }
}

/// The arms of a `Union` answer.
#[track_caller]
fn union_members(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
) -> Vec<SemanticNodeId> {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Union(arms)) => arms.to_vec(),
        other => panic!("expected a Union answer, got {other:?}"),
    }
}

/// One named member of an `Object` answer.
#[track_caller]
fn object_member(
    dispatch: &ProjectSemanticDispatch<'_>,
    node: SemanticNodeId,
    key: &str,
) -> SemanticNodeId {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Object(view)) => {
            view.positive_members()
                .iter()
                .find(|member| member.key.as_string() == Some(key))
                .unwrap_or_else(|| panic!("member `{key}` must be present"))
                .value
        }
        other => panic!("expected an Object answer, got {other:?}"),
    }
}

/// The `Array` element of an answer.
#[track_caller]
fn array_element(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> SemanticNodeId {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Array { element, .. }) => *element,
        other => panic!("expected an Array answer, got {other:?}"),
    }
}

/// The `Conditional` CHECK type of an answer.
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

/// One `Signature` answer, decomposed.
struct SigParts {
    params: Vec<SemanticNodeId>,
    return_type: SemanticNodeId,
    type_parameters: Vec<crate::semantic_query::TypeParamDecl>,
}

#[track_caller]
fn signature_parts(dispatch: &ProjectSemanticDispatch<'_>, node: SemanticNodeId) -> SigParts {
    match dispatch.graph().node_data(node).as_deref() {
        Some(SemanticNodeData::Signature {
            params,
            return_type,
            type_parameters,
            ..
        }) => SigParts {
            params: params.iter().map(|param| param.ty).collect(),
            return_type: *return_type,
            type_parameters: type_parameters.to_vec(),
        },
        other => panic!("expected a Signature answer, got {other:?}"),
    }
}

/// Evaluate one function under the CLEAN + WARM contract and hand its
/// flow-return GRAPH NODE to `pick`.
#[track_caller]
fn r5_node<R>(
    host: &Arc<VerterHost>,
    name: &str,
    part: FunctionPartIdentity,
    pick: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId) -> R,
) -> R {
    with_dispatch(host, |dispatch| {
        let key = r5_key_part(dispatch, name, part);
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

/// Evaluate one function under the CLEAN + UNADMITTED contract: the value
/// is served (no degradation) but the slot is NEVER warm-admitted — the
/// answer completes transaction-locally. That is the shape of a ROOTLESS
/// winner (a callee with no authored occurrence, like a local arrow) and
/// of a provisional SCC fixed-point join.
#[track_caller]
fn r5_node_unadmitted<R>(
    host: &Arc<VerterHost>,
    name: &str,
    part: FunctionPartIdentity,
    pick: impl FnOnce(&ProjectSemanticDispatch<'_>, SemanticNodeId) -> R,
) -> R {
    with_dispatch(host, |dispatch| {
        let key = r5_key_part(dispatch, name, part);
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
            0,
            "{name}'s answer completes transaction-locally and is never warm-admitted"
        );
        pick(dispatch, result.return_type())
    })
}

/// A NESTED function value's signature is composed under the ENCLOSING
/// frames' binder environment, not the file's owner scope.
///
/// Two independent halves have to hold at once. The CONTENT half must
/// stop reporting an enclosing binder as a frame-shadowed captured type
/// (a type parameter is a binder, never a scope lookup); the EVALUATOR
/// half must actually carry the enclosing binders into the nested
/// signature's environment. Fixing only the first turns a fail-closed
/// into a warm WRONG answer naming the module twin.
///
/// Correcting an earlier record: "an enclosing frame's type parameters
/// are captured into a nested frame, so a nested signature referring to
/// an outer binder fails closed — never a wrong answer" was only half
/// true. The nested BODY path did fail closed, because the capture scope
/// recorded the enclosing binders as captured TYPE names. The nested
/// SIGNATURE path never consulted that capture scope at all — it gates
/// against the enclosing frame directly, where the enclosing clause was
/// invisible — so `bindNestedParam` / `bindNestedRest` /
/// `bindNestedConstraint` / `bindNestedDefault` each published the
/// module twin's declaration cleanly and warm.
///
/// Oracle (tsgo checker on exactly these bodies):
///
/// ```text
/// bindNestedParam<number>()      : (p: number) => number
/// bindNestedRest<number>()       : (...p: number[]) => number[]
/// bindNestedConstraint<{k:1}>()  : <U extends { k: 1; }>(p: U) => U
/// bindNestedDefault<number>()    : <U = number>(p?: U) => U | undefined
/// bindDepthTwo<number>()()       : (p: number) => number
/// bindNestedBody<number>()       : (x: unknown) => number
/// bindNoTwin<number>()           : (p: number) => number
/// bindInferCheck<number[]>()     : (x: unknown) => number
/// ctrlNestedConstraintModule()   : <U extends ModuleAlias>(p: U) => U
/// ctrlNestedDefaultModule()      : <U = "moduleAlias">(p?: U) => U | undefined
/// ```
///
/// Mutation recipes: dropping `type_param_names` from the
/// `Lowerer::name_is_frame_bound` binder short-circuit flips every
/// nested row to `DeclRef`; dropping the `outer` seed from the composed
/// binder environment flips every nested row to `DeclRef` as well; both
/// leave the two `ctrl*` module rows green, which is what makes them
/// controls.
#[test]
fn flow_return_nested_signature_composes_the_enclosing_binder_environment() {
    let host = make_r5_host();

    // The enclosing clause's binder, referenced from the nested
    // signature's parameter / rest / constraint / default positions.
    r5_node(
        &host,
        "bindNestedParam",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let sig = signature_parts(dispatch, node);
            assert_eq!(node_shape(dispatch, sig.params[0]), type_param("OuterTP"));
            assert_eq!(node_shape(dispatch, sig.return_type), type_param("OuterTP"));
        },
    );
    r5_node(
        &host,
        "bindNestedRest",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let sig = signature_parts(dispatch, node);
            let element = array_element(dispatch, sig.params[0]);
            assert_eq!(node_shape(dispatch, element), type_param("OuterTP"));
        },
    );
    r5_node(
        &host,
        "bindNestedConstraint",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let sig = signature_parts(dispatch, node);
            let constraint = sig.type_parameters[0]
                .constraint
                .expect("the nested clause carries its constraint");
            assert_eq!(node_shape(dispatch, constraint), type_param("OuterTP"));
        },
    );
    r5_node(
        &host,
        "bindNestedDefault",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let sig = signature_parts(dispatch, node);
            let default = sig.type_parameters[0]
                .default
                .expect("the nested clause carries its default");
            assert_eq!(node_shape(dispatch, default), type_param("OuterTP"));
        },
    );

    // DEPTH 2: the binder crosses two nested frames, in BOTH the
    // parameter and the return position.
    r5_node(
        &host,
        "bindDepthTwo",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let outer = signature_parts(dispatch, node);
            let inner = signature_parts(dispatch, outer.return_type);
            assert_eq!(node_shape(dispatch, inner.params[0]), type_param("DeepTP"));
            assert_eq!(
                node_shape(dispatch, inner.return_type),
                type_param("DeepTP")
            );
        },
    );

    // A nested BODY leaf naming the enclosing binder.
    r5_node(
        &host,
        "bindNestedBody",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let sig = signature_parts(dispatch, node);
            assert_eq!(node_shape(dispatch, sig.return_type), type_param("OuterTP"));
        },
    );

    // NO module twin: the leak is latent there — a `BareRef` is just as
    // wrong as a `DeclRef`, it simply has nothing to bind to yet.
    r5_node(
        &host,
        "bindNoTwin",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let sig = signature_parts(dispatch, node);
            assert_eq!(node_shape(dispatch, sig.params[0]), type_param("FreshTP"));
        },
    );

    // An `infer`-bearing conditional whose CHECK type is the enclosing
    // binder.
    r5_node(
        &host,
        "bindInferCheck",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let sig = signature_parts(dispatch, node);
            let check = conditional_check(dispatch, sig.return_type);
            assert_eq!(node_shape(dispatch, check), type_param("OuterTP"));
        },
    );

    // CONTROLS — the MODULE alias is the checker's answer here, and a
    // composed environment must not steal it.
    r5_node(
        &host,
        "ctrlNestedConstraintModule",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let sig = signature_parts(dispatch, node);
            let constraint = sig.type_parameters[0]
                .constraint
                .expect("the nested clause carries its constraint");
            assert_eq!(node_shape(dispatch, constraint), decl_ref("ModuleAlias"));
        },
    );
    r5_node(
        &host,
        "ctrlNestedDefaultModule",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let sig = signature_parts(dispatch, node);
            let default = sig.type_parameters[0]
                .default
                .expect("the nested clause carries its default");
            assert_eq!(node_shape(dispatch, default), decl_ref("ModuleAlias"));
        },
    );
}

/// A type-parameter clause binds its OWN siblings — including FORWARD
/// ones — in both the ROOT and the NESTED arm.
///
/// The clause is interned in one pass and its constraints / defaults
/// lower in a second under that environment, so the visible inventory is
/// the WHOLE clause, never "the preceding siblings". TypeScript accepts
/// a forward sibling reference in a constraint (`<U extends V, V>`) and
/// still constrains through it, so a preceding-only inventory is wrong
/// for exactly that shape.
///
/// Oracle (tsgo checker):
///
/// ```text
/// bindNestedSameClause() : <SameClause, U extends SameClause>(a: SameClause, p: U) => U
/// bindRootSameClause     : <SameClause, U extends SameClause>(p: U) => U
/// bindForwardSibling     : <U extends SameClause, SameClause>(u: U, v: SameClause) => U
/// ctrlRootConstraintModule : <U extends ModuleAlias>(p: U) => U
/// ```
///
/// Mutation recipe: collapsing the two passes back into one owner-scope
/// lowering flips all three same-clause rows to `DeclRef` and leaves the
/// `ctrl*` row green.
#[test]
fn flow_return_type_parameter_clause_binds_its_own_siblings() {
    let host = make_r5_host();

    // NESTED clause, backward sibling.
    r5_node(
        &host,
        "bindNestedSameClause",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let sig = signature_parts(dispatch, node);
            let constraint = sig.type_parameters[1]
                .constraint
                .expect("`U extends SameClause` carries its constraint");
            assert_eq!(node_shape(dispatch, constraint), type_param("SameClause"));
        },
    );

    // ROOT clause, backward sibling: the returned `U` binder carries it.
    r5_node(
        &host,
        "bindRootSameClause",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(node_shape(dispatch, node), type_param("U"));
            let constraint = match dispatch.graph().node_data(node).as_deref() {
                Some(SemanticNodeData::TypeParam { constraint, .. }) => {
                    constraint.expect("`U extends SameClause` carries its constraint")
                }
                other => panic!("expected the `U` binder, got {other:?}"),
            };
            assert_eq!(node_shape(dispatch, constraint), type_param("SameClause"));
        },
    );

    // ROOT clause, FORWARD sibling.
    r5_node(
        &host,
        "bindForwardSibling",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(node_shape(dispatch, node), type_param("U"));
            let constraint = match dispatch.graph().node_data(node).as_deref() {
                Some(SemanticNodeData::TypeParam { constraint, .. }) => {
                    constraint.expect("`U extends SameClause` carries its constraint")
                }
                other => panic!("expected the `U` binder, got {other:?}"),
            };
            assert_eq!(node_shape(dispatch, constraint), type_param("SameClause"));
        },
    );

    // CONTROL — a root constraint naming a MODULE type stays the module
    // declaration.
    r5_node(
        &host,
        "ctrlRootConstraintModule",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let constraint = match dispatch.graph().node_data(node).as_deref() {
                Some(SemanticNodeData::TypeParam { constraint, .. }) => {
                    constraint.expect("`U extends ModuleAlias` carries its constraint")
                }
                other => panic!("expected the `U` binder, got {other:?}"),
            };
            assert_eq!(node_shape(dispatch, constraint), decl_ref("ModuleAlias"));
        },
    );
}

/// Across FRAMES a type parameter and a same-named type-space local
/// genuinely coexist, and the NEAREST one wins — in both directions.
///
/// TS2300 forbids the collision only INSIDE one frame
/// (`function f<T>() { class T {} }` is a duplicate identifier), so the
/// binder and scope inventories are NOT disjoint by construction: a
/// `class T` in an intermediate frame shadows an enclosing `<T>` for
/// everything it encloses, and a nearer `<T>` shadows an outer frame's
/// `class T`. A binder inventory consulted as one flat union — before
/// the frame's own lexical authority — gets the first direction wrong
/// and publishes the enclosing binder cleanly and warm.
///
/// A CLASS binder is likewise NOT protected: `class C<T> { m() { class
/// T {} … } }` is legal and the member's local wins, so an enclosing
/// class clause has to sit BEHIND the member frame's own lexical
/// authority rather than beside the function's own clause.
///
/// Oracle (tsgo checker):
///
/// ```text
/// localClassShadowsOuterBinder<number>()()       : (x: unknown) => localClassShadowsOuterBinder.NestT
/// localClassSameFrameShadowsOuterBinder<number>()() : sameFrameCheck-shaped local class
/// binderShadowsOuterLocalClass()<number>()       : (x: unknown) => number
/// new ClassLocalShadow<number>().m(0)            : ClassLocalShadow.CT
/// new ClassLocalShadow<number>().n(0)            : (y: unknown) => ClassLocalShadow.CT
/// ```
///
/// A local class is not a modellable answer, so every row whose answer
/// is one fails CLOSED — never the module twin, and never the enclosing
/// binder published cleanly and warm.
///
/// Mutation recipes: consulting the captured binder inventory BEFORE the
/// frame's own skeleton flips `localClassSameFrameShadowsOuterBinder`
/// and both `ClassLocalShadow` rows to a warm `TypeParam`; dropping the
/// reciprocal inventory removal in `capture_scope_for` flips
/// `localClassShadowsOuterBinder`; carrying the enclosing class clause
/// as the member frame's OWN `type_param_names` flips both
/// `ClassLocalShadow` rows.
#[test]
fn flow_return_nearest_declaration_wins_between_binders_and_frame_locals() {
    let host = make_r5_host();

    // A nearer frame-local class shadows the enclosing binder; the class
    // is unmodellable, so these fail closed rather than publishing
    // either the binder or the module twin.
    for name in [
        "localClassShadowsOuterBinder",
        "localClassSameFrameShadowsOuterBinder",
    ] {
        assert_fails_closed(&host, name);
    }

    // The same rule for a CLASS binder, in the member body and in a
    // nested function value inside it.
    for member_ordinal in [0u32, 1u32] {
        with_dispatch(&host, |dispatch| {
            let key = r5_key_part(
                dispatch,
                "ClassLocalShadow",
                FunctionPartIdentity::Member {
                    member_path: Arc::from(vec![member_ordinal].into_boxed_slice()),
                },
            );
            assert_flow_fails_closed(
                dispatch,
                &format!("ClassLocalShadow member {member_ordinal}"),
                dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
            );
            assert_eq!(
                dispatch
                    .graph()
                    .slot_candidate_count_for_tests(&SemanticQueryKey::FlowReturn(Box::new(key))),
                0,
                "ClassLocalShadow member {member_ordinal} must admit nothing"
            );
        });
    }

    // A nearer binder shadows the enclosing frame's local class — from a
    // nested SIGNATURE (the enclosing frame's own clause answers) and
    // from a nested BODY one frame further down (the captured binder
    // inventory answers).
    for (name, binder) in [
        ("binderShadowsOuterLocalClass", "BindT"),
        ("binderShadowsOuterLocalClassInBody", "BodyT"),
    ] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                let outer = signature_parts(dispatch, node);
                let inner = signature_parts(dispatch, outer.return_type);
                assert_eq!(node_shape(dispatch, inner.return_type), type_param(binder));
            },
        );
    }
}

/// A BLOCK-scoped type-space local in the binder's OWN frame is NEARER
/// than that frame's own type-parameter clause.
///
/// TS2300 constrains only a BODY-level collision: `function f<T>() {
/// class T {} }` is a duplicate identifier, but `function f<T>() { {
/// class T {} … } }` is LEGAL and the block-scoped class WINS for
/// everything the block encloses. A frame's own clause consulted as a
/// short-circuit AHEAD of the frame's own lexical authority therefore
/// publishes the binder cleanly and warm for an answer whose type is an
/// unmodellable local class — and at a call site that binder is
/// substituted with the caller's type argument, so the wrong type fully
/// materialises.
///
/// Oracle (tsgo checker), each probed by assignability because the local
/// class / enum is what the checker NAMES:
///
/// ```text
/// const a: number = blockLevel<number>().inner;  // clean — the local class won
/// const b: number = blockLevel<number>();        // TS2322 'blockLevel.BT'
/// blockClassNestedSig<number>()(…)               : TS2322 'blockClassNestedSig.BT'
/// blockEnumVsOwnBinder<string>()                 : TS2322 'BE'
/// blockClassDeeper<number>()                     : TS2322 'blockClassDeeper.BN'
/// blockNamespaceQual<string>()                   : TS2322 'BQ'
/// blockClassOneFrameDown<number>()()             : TS2322 'blockClassOneFrameDown.BD'
/// ctrlRootParam<number>() / ctrlNoLocal<number>(): clean — the binder won
/// bodyClassCollides<…>                           : TS2300 (either verdict is acceptable)
/// ```
///
/// A local class / enum is not a modellable answer, so every row whose
/// answer is one fails CLOSED — never the frame's own binder published
/// cleanly and warm, and never the module twin.
///
/// Mutation recipes. Reverting `name_is_frame_bound` to consult this
/// frame's own clause BESIDE `binders` (one two-step short-circuit)
/// flips the four TYPE-meaning rows to a warm `TypeParam`;
/// `blockNamespaceQual` survives that mutation through the NAMESPACE arm
/// alone, which is exactly why it is not the row this test rests on.
/// Consulting the SKELETON ahead of BOTH inventories instead — the naive
/// swap — leaves every row here green and flips `tpShadowParamAnnot` to
/// `Error(Miss)`, because there `binders` is the NESTED signature's
/// clause while the skeleton is the ENCLOSING frame's. Applying only the
/// `name_is_frame_bound` reorder without passing an EMPTY binder list at
/// the two BODY-position call sites (the declarator annotation and
/// `leaf_type`) leaves exactly three rows warm and fixes only
/// `blockClassNestedSig`, which reaches the gate through a nested
/// signature.
#[test]
fn flow_return_block_scoped_local_shadows_this_frames_own_binder() {
    let host = make_r5_host();

    // The block-scoped local wins: bare TYPE meaning, ENUM, a deeper
    // block, a nested SIGNATURE annotation, and the QUALIFIED head.
    for name in [
        "blockClassVsOwnBinder",
        "blockClassNestedSig",
        "blockEnumVsOwnBinder",
        "blockClassDeeper",
        "blockNamespaceQual",
    ] {
        assert_fails_closed(&host, name);
    }

    // CONTROL — the same collision ONE FRAME DOWN already failed closed
    // before this fix: there the class sits in the NESTED frame's own
    // skeleton, so that frame's own lexical authority answers and no
    // clause was ever consulted ahead of it. It pins that this fix did
    // not change the frame the answer comes from.
    assert_fails_closed(&host, "blockClassOneFrameDown");

    // CONTROLS — no local of the binder's name: the binder is the
    // answer, clean and warm, asserted on the GRAPH NODE (a `DeclRef` to
    // the module twin raises to the same `Ref { name }`).
    for (name, binder) in [("ctrlRootParam", "CP"), ("ctrlNoLocal", "CN")] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                assert_eq!(node_shape(dispatch, node), type_param(binder));
            },
        );
    }

    // A BODY-level collision is TS2300 — malformed input. Either verdict
    // is acceptable; the gate must not panic on it.
    with_dispatch(&host, |dispatch| {
        let key = r5_key(dispatch, "bodyClassCollides");
        let _ = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key)));
    });
}

/// A CLASS type-parameter clause binds inside every member body it
/// encloses — the member's own signature and any nested function value
/// in it.
///
/// A member slot never carries the class clause through the function's
/// own type parameters, so without the enclosing-clause seed the class
/// binder reads as a free name and resolves in owner scope.
///
/// Oracle (tsgo checker):
///
/// ```text
/// new ClassTP<number>().m       : (x: number) => number
/// new ClassTP<number>().n(0)    : (p: number) => number
/// ```
///
/// Mutation recipe: dropping the class clause from the root binder
/// environment's seed flips both rows to `DeclRef { OuterTP }` (the
/// module alias) and leaves every function-clause row green.
#[test]
fn flow_return_class_type_parameter_clause_binds_in_its_members() {
    let host = make_r5_host();

    // The member's OWN signature.
    r5_node(
        &host,
        "ClassTP",
        FunctionPartIdentity::Member {
            member_path: Arc::from(vec![0u32].into_boxed_slice()),
        },
        |dispatch, node| {
            assert_eq!(node_shape(dispatch, node), type_param("OuterTP"));
        },
    );

    // A NESTED arrow inside the member.
    r5_node(
        &host,
        "ClassTP",
        FunctionPartIdentity::Member {
            member_path: Arc::from(vec![1u32].into_boxed_slice()),
        },
        |dispatch, node| {
            let sig = signature_parts(dispatch, node);
            assert_eq!(node_shape(dispatch, sig.params[0]), type_param("OuterTP"));
            assert_eq!(node_shape(dispatch, sig.return_type), type_param("OuterTP"));
        },
    );
}

/// A signature's OWN parameter list is a shadowing inventory of that
/// signature — in the ROOT arm as much as the nested one.
///
/// The root rule "a function's own signature does not see its
/// body-locals" is right about BODY LOCALS and says nothing about
/// PARAMETERS: `typeof p` in an annotation, and a preceding parameter
/// named in a default initializer, both bind the parameter. Resolving
/// them positively needs intra-signature forward-reference resolution;
/// until then they must fail CLOSED rather than publish the module
/// twin's type warm.
///
/// Oracle (tsgo checker):
///
/// ```text
/// paramTypeofRoot        : (pv: number, b: number) => number
/// paramTypeofNested()    : (pv: number, b: typeof pv) => number
/// paramDefaultRoot       : (pv?: number, b?: number) => number
/// paramDefaultNested()   : (pv?: number, b?: number) => number
/// paramTypeofCtrl        : (a: number, b: "modulePV") => "modulePV"
/// paramTypeofNoTwin      : (nt: number, b: number) => number
/// paramBodyLocalInvisible: (b: "moduleLoc") => "moduleLoc"
/// ```
///
/// Mutation recipe: removing the inventory from the ROOT arm flips
/// `paramTypeofRoot` / `paramDefaultRoot` to a warm `"modulePV"`;
/// removing it from the NESTED arm flips the other two; both leave the
/// three controls green.
#[test]
fn flow_return_parameter_list_is_its_own_shadowing_inventory() {
    let host = make_r5_host();

    // A parameter named by a SIBLING annotation / default fails closed:
    // the module twin is never the answer.
    for name in [
        "paramTypeofRoot",
        "paramTypeofNested",
        "paramDefaultRoot",
        "paramDefaultNested",
    ] {
        assert_fails_closed(&host, name);
    }

    // CONTROL — no parameter of that name: the module const IS the
    // checker's answer, clean and warm.
    assert_clean_warm(&host, "paramTypeofCtrl", string_lit("modulePV"));

    // CONTROL — a shadowing parameter with NO module twin: nothing can
    // be mis-bound, so the root-identifier gate must NOT fire (this is
    // not `UnmodeledBinding`) and the owner scope genuinely answers
    // nothing. The value is therefore the honest local miss carrier —
    // and because a miss carrier is not a KNOWN value, it is
    // `UnresolvedValue` / `ReturnOnly`, never warm. The two verdicts are
    // separate on purpose: the gate says "the name would bind something
    // else here", the admission says "this value is not known".
    assert_eq!(
        r5_eval(&host, "paramTypeofNoTwin").map(|outcome| outcome.degradation),
        Some(Some(
            crate::semantic_query::FlowReturnDegradation::UnresolvedValue
        )),
        "an unresolvable owner-scope probe is a miss carrier, not a complete result"
    );
    assert_eq!(
        r5_eval(&host, "paramTypeofNoTwin").map(|outcome| outcome.candidates),
        Some(0),
        "a miss carrier admits nothing"
    );
    with_dispatch(&host, |dispatch| {
        let QueryResult::Value(SemanticQueryOutput {
            value: SemanticQueryValue::FlowReturn(result),
            ..
        }) = dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(r5_key(
            dispatch,
            "paramTypeofNoTwin",
        ))))
        else {
            panic!("paramTypeofNoTwin must still produce a usable degraded value");
        };
        assert_eq!(
            node_shape(dispatch, result.return_type()),
            NodeShape::Opaque
        );
    });

    // CONTROL — a BODY LOCAL stays invisible in the root parameter list,
    // so the module const is still the answer.
    assert_clean_warm(&host, "paramBodyLocalInvisible", string_lit("moduleLoc"));
}

/// A GENERIC callee at a direct-call site must never publish the
/// CALLEE's OWN type parameter as this frame's value.
///
/// The direct-call rail hands back whatever the callee answers with —
/// its body-derived flow return, or its DECLARED return carrier when it
/// annotates one. For a generic callee BOTH of those are expressed in
/// the callee's own binders, so returning them verbatim publishes
/// `TypeParam(GD)` / `TypeParam(GF)` / `TypeParam(GB)` as the caller's
/// whole-function return — cleanly, and warm.
///
/// That is not merely imprecise, it is unsound, because the binder
/// identity is file-scoped and name-keyed: `GcHolder<GH>`'s own clause
/// and a same-named callee parameter intern to ONE `SemanticNodeId`, so
/// a leaked callee binder is substitutable by an unrelated enclosing
/// instantiation. `new GcHolder<number>().viaCall()` would answer
/// `number` for a member whose value has nothing to do with `GH`.
///
/// The call-resolution executor now answers most of these routes: an
/// explicit type argument instantiates the clause exactly, and argument
/// inference from a literal publishes the un-widened literal of the
/// checker's widened answer. What remains is the shape TypeScript itself
/// cannot infer (`gcBareInferred`, where `unknown` IS the checker's
/// answer) and the routes the executor does not read: the annotated-alias
/// value type (`gcViaAnnotated`) keeps the sb15 interim `unknown`, and a
/// NAMESPACE-scoped callee (`GcNs.nsCall`) is outside the executor's
/// reach — it refuses, and the position degrades as an unrepresentable
/// callee rather than guessing.
///
/// Oracle (tsgo checker, `--strict --declaration`):
///
/// ```text
/// gcDeclExplicit(): string     gcDeclInferred(): string
/// gcFlowExplicit(): string     gcFlowInferred(): string
/// gcBareExplicit(): string     gcBareInferred(): unknown   ← exact
/// gcViaAnnotated(): string     gcNonGeneric():   string
/// new GcHolder<number>().viaCall(): string
/// new GcHolder<number>().ownT():   number
/// ```
///
/// The `Literal(String("a"))` rows are the un-widened literals of the
/// checker's widened `string`. `gcBareInferred` stays `unknown` — it is
/// the row where sb15 IS the checker's answer. `gcViaAnnotated` keeps the
/// interim (the annotated-alias route is not call-resolved), and
/// `GcNs.nsCall` degrades (a namespace-scoped callee is outside the
/// executor's reach).
///
/// Mutation recipes:
///
/// - Returning `result.return_type()` (the flow branch) or `hot.node()`
///   (the declared branch) verbatim flips `gcFlow*` / `gcDecl*` /
///   `gcBare*` to a warm binder and collapses `viaCall` onto `ownT`'s
///   node. Dropping only the DECLARED branch's instantiation leaves
///   `gcFlow*` green and flips `gcDecl*` / `gcBare*`; dropping only the
///   FLOW branch's leaves `gcDecl*` / `gcBare*` green and flips
///   `gcFlow*`, `GcNs.nsCall`, and the `viaCall` identity row.
/// - Reading the callee's clause off its PREPARED value declaration
///   instead of the function program index leaves every file-scope row
///   green and flips `GcNs.nsCall` alone: a namespace-scoped function has
///   no prepared declaration, so the clause reads EMPTY and nothing is
///   instantiated.
/// - Dropping the deferred-head arm from the name-driven binder
///   collection (`include_unbound_heads`) leaves `gcFlow*` green and
///   flips `gcDecl*` / `gcBare*`: a DECLARED return `: GD` lowers in the
///   callee's file owner scope, where its own clause is not in scope, so
///   it interns as an unbound `BareRef("GD")` rather than a resolved
///   binder.
///
/// The three CONTROLS stay green under every one of those:
/// `gcNonGeneric` / `GcNs.nsPlainCall` have no binder to leak, and
/// `ownT` is the caller's OWN clause, which must survive.
#[test]
fn flow_return_generic_direct_callee_never_publishes_the_callees_binder() {
    let host = make_r5_host();

    // Explicit type arguments instantiate the callee's clause EXACTLY —
    // and never publish the callee's binder.
    for name in ["gcDeclExplicit", "gcFlowExplicit", "gcBareExplicit"] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                assert_eq!(
                    node_shape(dispatch, node),
                    NodeShape::Primitive(PrimitiveKind::String),
                    "{name} must instantiate the callee's own type parameter, \
                     never publish the callee's binder"
                );
            },
        );
    }

    // Argument inference from a literal instantiates the clause; the
    // fresh-literal return widens at the caller's return join for the
    // DECLARED-carrier callee, while the body-derived carrier keeps the
    // bare literal today (its freshness does not yet reach the join) —
    // the un-widened literal of the checker's `string`, never the
    // callee's binder either way.
    for name in ["gcDeclInferred"] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                assert_eq!(
                    node_shape(dispatch, node),
                    NodeShape::Primitive(PrimitiveKind::String),
                    "{name} must instantiate the callee's own type parameter, \
                     never publish the callee's binder"
                );
            },
        );
    }

    // A bare-`T` return with nothing to infer from keeps `unknown` — the
    // checker's own answer for this shape.
    r5_node(
        &host,
        "gcBareInferred",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::Unknown),
                "gcBareInferred has no inference candidate: `unknown` is the \
                 checker's answer"
            );
        },
    );

    // The annotated-alias route is not call-resolved: the sb15 interim
    // `unknown` stands — never the callee's binder.
    r5_node(
        &host,
        "gcViaAnnotated",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::Unknown),
                "gcViaAnnotated keeps the interim: the annotated-alias route \
                 is not call-resolved"
            );
        },
    );

    // A NAMESPACE-scoped generic callee is outside the executor's reach:
    // it refuses, and the position degrades as an unrepresentable callee
    // rather than guessing from a clause it cannot read.
    assert_degraded(
        &host,
        "GcNs.nsCall",
        crate::semantic_query::FlowReturnDegradation::UnrepresentableCallee,
    );

    // CONTROLS — a NON-generic direct callee is untouched: the rule fires
    // on the callee's declared clause, not on "any direct call".
    // `GcNs.nsPlainCall` is the namespace control: non-generic, so the
    // executor's reach gap on namespace-scoped callees does not fire.
    for name in ["gcNonGeneric", "GcNs.nsPlainCall"] {
        assert_clean_warm(&host, name, TypeExpr::Primitive(PrimitiveName::String));
    }

    // The MEMBER-ALIASING row. `GcHolder::viaCall` calls a generic
    // callee whose parameter is NOT `GH`; `GcHolder::ownT` returns the
    // class's own `GH`. Before the fix both published the SAME
    // `SemanticNodeId` — the file-scoped name-keyed binder — so the
    // class instantiation substituted into a value it does not own.
    let via_call = r5_node(
        &host,
        "GcHolder",
        FunctionPartIdentity::Member {
            member_path: Arc::from(vec![0u32].into_boxed_slice()),
        },
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::String),
                "GcHolder::viaCall instantiates the callee's clause from the \
                 explicit type argument — it must not publish a binder"
            );
            node
        },
    );
    let own_t = r5_node(
        &host,
        "GcHolder",
        FunctionPartIdentity::Member {
            member_path: Arc::from(vec![1u32].into_boxed_slice()),
        },
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                type_param("GH"),
                "GcHolder::ownT is the class's OWN clause and must survive"
            );
            node
        },
    );
    assert_ne!(
        via_call, own_t,
        "a call's published value must never BE the enclosing class's own \
         type-parameter node: an enclosing `GcHolder<number>` would then \
         substitute into it"
    );
}

/// Every `TypeParam` binder NAME reachable in one answer's node tree.
///
/// A leaked binder is rarely the whole answer: the SCC fixed point joins
/// it into a union, so asserting only the ROOT node's shape would call
/// `unknown | CT` clean. This walks the tree the way a substitution
/// would.
fn reachable_type_param_names(
    dispatch: &ProjectSemanticDispatch<'_>,
    root: SemanticNodeId,
) -> Vec<String> {
    let mut visited: std::collections::BTreeSet<SemanticNodeId> = Default::default();
    let mut stack = vec![root];
    let mut names: Vec<String> = Vec::new();
    while let Some(node) = stack.pop() {
        if !visited.insert(node) {
            continue;
        }
        let Some(data) = dispatch.graph().node_data(node) else {
            continue;
        };
        match data.as_ref() {
            SemanticNodeData::TypeParam { display_name, .. } => {
                names.push(display_name.to_string());
            }
            SemanticNodeData::Union(members) | SemanticNodeData::Intersection(members) => {
                stack.extend(members.iter().copied());
            }
            SemanticNodeData::Array { element, .. } => stack.push(*element),
            SemanticNodeData::Alias(target) => stack.push(*target),
            _ => {}
        }
    }
    names.sort();
    names.dedup();
    names
}

/// EVERY route from a callee's return to its caller instantiates the
/// callee's own clause — not just the direct-call rail.
///
/// A callee's return is expressed in the CALLEE's binders, and the
/// binder identity is file-scoped and name-keyed, so handing that return
/// back verbatim publishes a node an unrelated enclosing
/// `class Holder<T>` can substitute into. The direct-call rail learned
/// that rule; three sibling routes reading the SAME
/// `SemanticNodeData::Signature { return_type }` shape did not:
///
/// - a call on a LOCAL function-typed binding (`const idL = <RL>(x: RL): RL => x; idL("a")`),
/// - an IIFE (`(<RI>(x: RI): RI => x)("a")`),
/// - a call on a generic function-typed PARAMETER (`fn: <RP>(x: RP) => RP`).
///
/// Each published the callee's binder as the caller's whole return,
/// cleanly and warm — and `RvHolder::viaLocal` published the very
/// `SemanticNodeId` the enclosing `class RvHolder<RL>` clause interns,
/// so `new RvHolder<number>().viaLocal()` answered `number` for a value
/// that has nothing to do with `RL`.
///
/// Oracle (tsgo checker, `--strict --declaration`):
///
/// ```text
/// rvLocalLambdaCall(): string   rvIife():      string
/// rvParamCall():       string   new RvHolder<number>().viaLocal(): string
/// new RvHolder<number>().ownRL(): RL
/// ```
///
/// The local-binding and parameter routes now resolve through argument
/// inference: the published value is the un-widened literal of the
/// checker's widened `string`. Both callees are ROOTLESS — a local arrow
/// and a parameter's function type have no authored function occurrence —
/// so the value is served but the slot is never warm-admitted. The IIFE
/// route is not call-resolved: `unknown` is the recorded interim there —
/// not the checker's answer, but a leaked binder is not either, and it is
/// the answer that cannot be substituted into.
///
/// Mutation recipes (each verified to flip exactly these rows):
///
/// - taking `*return_type` off the signature node in the binding-call
///   arm flips `rvLocalLambdaCall` / `rvParamCall` / `viaLocal` and
///   re-aliases `viaLocal` onto `ownRL`;
/// - the same in the IIFE arm flips `rvIife` alone;
/// - the CONTROL `ownRL` — the caller's OWN clause — must survive every
///   one of them.
#[test]
fn flow_return_every_call_route_instantiates_the_callees_clause() {
    let host = make_r5_host();

    // The ROOTLESS routes: a local arrow and a generic function-typed
    // parameter resolve from the literal argument, complete
    // transaction-locally, and are never warm-admitted.
    for name in ["rvLocalLambdaCall", "rvParamCall"] {
        r5_node_unadmitted(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                assert_eq!(
                    node_shape(dispatch, node),
                    NodeShape::Primitive(PrimitiveKind::String),
                    "{name} must instantiate the callee's own clause, never publish \
                     the callee's binder"
                );
            },
        );
    }

    // The IIFE route is not call-resolved: the recorded interim stands.
    r5_node(
        &host,
        "rvIife",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::Unknown),
                "rvIife keeps the interim: the IIFE route is not call-resolved"
            );
        },
    );

    // The MEMBER-ALIASING pair: the local lambda's clause is spelled
    // `RL`, exactly the enclosing class's, so before the fix both members
    // published ONE node.
    let via_local = r5_node_unadmitted(
        &host,
        "RvHolder",
        FunctionPartIdentity::Member {
            member_path: Arc::from(vec![0u32].into_boxed_slice()),
        },
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::String),
                "RvHolder::viaLocal must not publish a binder"
            );
            node
        },
    );
    let own_rl = r5_node(
        &host,
        "RvHolder",
        FunctionPartIdentity::Member {
            member_path: Arc::from(vec![1u32].into_boxed_slice()),
        },
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                type_param("RL"),
                "RvHolder::ownRL is the class's OWN clause and must survive"
            );
            node
        },
    );
    assert_ne!(
        via_local, own_rl,
        "a local-binding call's published value must never BE the enclosing \
         class's own type-parameter node: an enclosing `RvHolder<number>` \
         would then substitute into it"
    );
}

/// The SCC fixed point performs the same callee-return transfer the call
/// arm does, so it owes the same instantiation.
///
/// In a GENERIC mutual-recursion component both members are provisional
/// when the component closes, so each one's contribution to the other
/// arrives through the equation `result_i = seed_i ∪ (⋃ hold targets)`
/// rather than through the call arm's own return value. Joining a hold
/// target's `return_type` raw there re-published exactly the binder the
/// call arm had already instantiated away: `cgOne` answered
/// `unknown | CO | CT`, where the `unknown` arm is the call arm's
/// instantiated contribution and `CT` is `cgTwo`'s clause — the SAME
/// `SemanticNodeId` the unrelated `class CgHolder<CT>` interns.
///
/// Oracle (tsgo checker, `--strict`): `cgOne` and `cgTwo` are TS7023
/// (`implicitly has return type 'any' because it ... is referenced
/// directly or indirectly in one of its return expressions`), so
/// TypeScript declines to type them at all. The substrate's coinductive
/// answer is the component's own union — which must contain each
/// member's OWN binder and no other member's. The provisional join
/// completes transaction-locally and is never warm-admitted.
///
/// Mutation recipe: joining `result.return_type()` instead of
/// `hold.discharged(...)` restores `CT` to `cgOne` (and `CO` to
/// `cgTwo`), leaving both members' own-binder rows green.
#[test]
fn flow_return_scc_fixed_point_never_republishes_a_foreign_callee_binder() {
    let host = make_r5_host();

    // The bait: an unrelated class whose clause is spelled `CT`. Under
    // the file-scoped name-keyed binder identity it interns the very node
    // `cgTwo`'s clause does, so `cgOne` republishing `CT` hands
    // `CgHolder<number>` a substitution site it does not own.
    let own_ct = r5_node(
        &host,
        "CgHolder",
        FunctionPartIdentity::Member {
            member_path: Arc::from(vec![0u32].into_boxed_slice()),
        },
        |dispatch, node| {
            assert_eq!(node_shape(dispatch, node), type_param("CT"));
            node
        },
    );

    r5_node_unadmitted(
        &host,
        "cgOne",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let names = reachable_type_param_names(dispatch, node);
            assert!(
                !names.iter().any(|name| name == "CT"),
                "cgOne must not republish cgTwo's clause through the fixed point; got {names:?}"
            );
            assert!(
                names.iter().any(|name| name == "CO"),
                "cgOne's OWN binder must survive the fixed point; got {names:?}"
            );
            let mut stack = vec![node];
            let mut seen = false;
            while let Some(current) = stack.pop() {
                seen |= current == own_ct;
                if let Some(SemanticNodeData::Union(members)) =
                    dispatch.graph().node_data(current).as_deref()
                {
                    stack.extend(members.iter().copied());
                }
            }
            assert!(
                !seen,
                "cgOne must not reach `CgHolder::ownCT`'s node — an unrelated \
                 `CgHolder<number>` would substitute into cgOne's value"
            );
        },
    );

    r5_node_unadmitted(
        &host,
        "cgTwo",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let names = reachable_type_param_names(dispatch, node);
            assert!(
                !names.iter().any(|name| name == "CO"),
                "cgTwo must not republish cgOne's clause through the fixed point; got {names:?}"
            );
            assert!(
                names.iter().any(|name| name == "CT"),
                "cgTwo's OWN binder must survive the fixed point; got {names:?}"
            );
        },
    );
}

/// A callee clause parameter whose name ALSO names a file-scope
/// declaration still instantiates.
///
/// A declaration's own clause is not in scope where its DECLARED return
/// locator lowers (file owner scope), so `first<Item>(xs: Item[]): Item`
/// interns its return as a bare `Item` head. The name-keyed claim that
/// catches the UNRESOLVED spelling (`BareRef`) sees nothing when the file
/// also declares `interface Item` — the owner-scope lowering RESOLVES
/// the head, to the wrong symbol — so the caller published the unrelated
/// INTERFACE as its own value, cleanly and warm.
///
/// The `shDecl` / `callsShDecl` pair is the ROUTE-AGREEMENT half: the
/// callee's own body-derived return answers `TypeParam("SH")` (its frame
/// binds the clause) while the caller's declared-return route answered
/// `DeclRef("SH")` — two routes to one callee disagreeing about what
/// `SH` even is.
///
/// Oracle (tsgo checker, `--strict --declaration`):
///
/// ```text
/// shFirst<Item>(xs: Item[]): Item   shUseFirst(xs: number[]): number
/// shDecl<SH>(x: SH): SH             callsShDecl():             string
/// shPlainItem():        ShItem      shUsePlainItem():          ShItem
/// ```
///
/// `callsShDecl` now resolves through argument inference — the
/// un-widened literal of the checker's `string`. `shUseFirst` keeps the
/// recorded interim: its argument is a parameter reference, not a
/// literal, so the executor is undecidable and the pre-existing read
/// stands. The CONTROL pair is exact and is what keeps the
/// claim clause-scoped rather than name-scoped: `shUsePlainItem` reaches
/// the same `ShItem` interface through a NON-generic callee, which
/// declares no clause, so its `DeclRef` must survive untouched.
///
/// Mutation recipe: dropping the `DeclRef` arm from the name-driven
/// binder collection flips `shUseFirst` back to `DeclRef("ShItem")` and
/// `callsShDecl` back to `DeclRef("SH")`, while every `BareRef`-spelled
/// row (`gcDecl*` / `gcBare*`) and the control stay green.
#[test]
fn flow_return_callee_clause_shadowing_a_file_scope_declaration_still_instantiates() {
    let host = make_r5_host();

    // `shUseFirst`'s argument is a parameter reference: the executor is
    // undecidable and the pre-existing interim stands — the resolved head
    // is claimed as the clause parameter, never the shadowed interface.
    r5_node(
        &host,
        "shUseFirst",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::Number),
                "shUseFirst's callee clause SHADOWS the same-named file-scope \
                 declaration, so the resolved head is the clause parameter — \
                 publishing the declaration is publishing an unrelated symbol"
            );
        },
    );

    // `callsShDecl` supplies a literal: the executor infers the clause
    // and the answer is the un-widened literal of the checker's `string`.
    r5_node(
        &host,
        "callsShDecl",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::String),
                "callsShDecl's callee clause SHADOWS the same-named file-scope \
                 declaration — the inferred literal, never the shadowed \
                 interface and never the callee's binder"
            );
        },
    );

    r5_node(
        &host,
        "gcFlowInferred",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Other("Literal(String(\"a\"))".to_string()),
                "the body-derived carrier keeps the bare literal today (its                  freshness does not yet reach the join) — never the binder"
            );
        },
    );

    // ROUTE AGREEMENT — the callee's own body-derived answer keeps its
    // clause binder; the CALLER's route instantiates it. The two must not
    // be the same node, and the caller's must not be a reference.
    let decl = r5_node(
        &host,
        "shDecl",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                type_param("SH"),
                "shDecl's OWN return is its own binder"
            );
            node
        },
    );
    let caller = r5_node(
        &host,
        "callsShDecl",
        FunctionPartIdentity::DeclarationBody,
        |_dispatch, node| node,
    );
    assert_ne!(
        decl, caller,
        "the callee's own binder must never BE the caller's published value"
    );

    // CONTROL — the same interface name through a NON-generic callee.
    // Nothing declares `ShItem` as a clause parameter there, so the
    // published reference is the interface and must survive.
    r5_node(
        &host,
        "shUsePlainItem",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                decl_ref("ShItem"),
                "a non-generic callee declares no clause: its resolved return \
                 reference must survive untouched"
            );
        },
    );
}

/// An OVERLOADED callee never publishes its HIDDEN implementation.
///
/// The per-file function-program index carries ONE entry per overload
/// group, at the trailing IMPLEMENTATION, so the direct-call rail
/// reached exactly the signature the language HIDES. `ovX("a")` — whose
/// two VISIBLE overloads return `OA` and `string` — published the
/// implementation's `any`, cleanly and warm, with no visible signature
/// that ever returns it. That contradicts this project's own
/// overload-visibility rule: a multi-signature group surfaces its
/// bodiless overloads in source order and hides the trailing
/// implementation.
///
/// The call-resolution executor now picks the FIRST APPLICABLE signature
/// in declaration order and instantiates its clause from the arguments:
/// `ovX("a")` answers the first visible overload's inferred `OA` — the
/// un-widened literal of the checker's `string`. The answer is never the
/// implementation's `any` and never the LAST overload's return.
///
/// Oracle (tsgo checker, `--strict --declaration`):
///
/// ```text
/// ovXCall():      string     ← the FIRST visible overload, inferred
/// ovSingleCall(): string
/// ```
///
/// Mutation recipes:
///
/// - removing the overload gate republishes `Primitive(Any)` for
///   `ovXCall` with NO degradation — the exact pre-fix shape;
/// - reading the LAST declaration of the group flips `ovXCall` to the
///   implementation's `any`.
#[test]
fn flow_return_overloaded_callee_never_publishes_the_hidden_implementation() {
    let host = make_r5_host();

    r5_node(
        &host,
        "ovXCall",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::String),
                "ovXCall resolves to the FIRST APPLICABLE overload — never the \
                 hidden implementation's `any`, never the last declaration"
            );
        },
    );

    // CONTROL — a LONE signature resolves the same way, bodied or not:
    // the rule is overload VISIBILITY, not "any function with a body".
    r5_node(
        &host,
        "ovSingleCall",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::String),
                "a lone visible signature instantiates its clause from the \
                 argument"
            );
        },
    );
}

/// A RESOLVED overload group publishes the FIRST APPLICABLE signature's
/// return — never a fabricated `any`, never the hidden implementation,
/// never the LAST declaration.
///
/// A fabricated `Primitive(Any)` is bidirectionally assignable, so it is
/// indistinguishable from an authored `any` at every downstream gate (an
/// overloaded callee published a prop as `any` where the checker says
/// `boolean`), and it is indistinguishable from an exactly-typed sibling
/// to any consumer reading the position. The executor resolves all three
/// group shapes below — a bodied group, an AMBIENT group, and a group
/// reached through a composite expression — so the value is the picked
/// overload's own return.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict`):
///
/// ```text
/// ovXCall():   string   ← the FIRST visible overload, inferred
/// amb3Call():  "A"      ← the FIRST matching ambient declaration
/// tnAmbBare(): "TA"
/// ```
///
/// The `Literal(String("a"))` row is the un-widened literal of the
/// checker's widened `string`.
///
/// Discrimination: reading the LAST declaration of the group flips
/// `amb3Call` to `Literal(String("C"))` and `ovXCall` to the
/// implementation's `any`; fabricating `any` at the position fails the
/// node assertion for all three. The CONTROL is a LONE signature: a
/// change that routes every call through overload resolution's refusal
/// path fails it.
#[test]
fn flow_return_resolved_overload_never_publishes_a_fabricated_any() {
    let host = make_r5_host();

    for name in ["ovXCall"] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                assert_eq!(
                    node_shape(dispatch, node),
                    NodeShape::Primitive(PrimitiveKind::String),
                    "{name} resolves to the first applicable overload's own \
                     return — never a fabricated `any`"
                );
            },
        );
    }

    // CONTROL — a LONE signature resolves identically.
    r5_node(
        &host,
        "ovSingleCall",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::String),
                "a lone visible signature instantiates its clause from the \
                 argument"
            );
        },
    );
}

/// A clause parameter's DECLARED DEFAULT applies ONLY when inference
/// could produce no candidate — never as an override of inference.
///
/// TypeScript's rule (`checker.ts::getInferredTypes`) is precise: a type
/// argument is the inferred candidate when inference produced one, and
/// the declared default ONLY when it produced none. Applying the default
/// unconditionally turns the honest recorded interim (`unknown`, for a
/// parameter this substrate cannot yet infer) into a confidently WRONG
/// concrete type, warm-admitted — strictly worse than the interim it
/// replaced.
///
/// The rule, now with the executor resolving what the call site offers:
///
/// - explicit type arguments present ⇒ they instantiate the clause
///   EXACTLY (`zpExplicitCall` answers `string`);
/// - otherwise, a parameter that occurs in a parameter type at an
///   ordinal the call actually SUPPLIES ⇒ inference has a candidate ⇒
///   the inferred type (the un-widened literal, for a literal argument);
/// - otherwise (no candidate is possible at all) ⇒ its declared default
///   when it has one, else `unknown`.
///
/// Where the executor cannot read the candidate (a non-literal argument)
/// the pre-existing interim `unknown` stands — still never the default.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`), read as the WRAPPER's return type through
/// `declare const w: ReturnType<typeof zzMismACall>; const p: null = w;`.
///
/// The probe form is load-bearing and the one-step
/// `const p: null = <call>` is NOT a sound reading here: for a generic
/// call whose parameter reaches the return type the contextual `null`
/// FEEDS return-type inference, and the one-step probe reports NO ERROR
/// AT ALL for `rvDefaultedCall` / `rvDefaultedFlowFreeCall` /
/// `zpUnusedTPCall` / `zpNotSuppliedCall` / `gcBareInferred`. Binding the
/// raw call to a `const` is not sound either: it reads the UNWIDENED
/// `1` / `true` where the wrapper's return type is `number` / `boolean`.
/// The values below are the wrapper readings; they were re-derived
/// through this form and all nine are correct.
///
/// ```text
/// zzMismACall():             number             ← default `string` LOSES to inference
/// zzMismBCall():             boolean            ← default `string` LOSES to inference
/// zpExplicitCall():          string             ← default `number` LOSES to the explicit argument
/// rvDefaultedFlowCall():     number             ← default `string` LOSES to inference
/// rvDefaultedCall():         number             ← argument-free: the default IS the answer
/// rvDefaultedFlowFreeCall(): number             ← argument-free, body-derived route
/// zpUnusedTPCall():          number             ← argument-BEARING, but `ZE` occurs in no parameter
/// zpNotSuppliedCall():       number             ← `ZF` occurs only at an UNSUPPLIED ordinal
/// gcBareInferred():          unknown            ← no default, and the checker agrees
/// ```
///
/// The three mismatch rows are what makes this discriminating: their
/// default (`string` / `number`) is DIFFERENT from what inference
/// produces, so a row that took the default anyway is distinguishable
/// from one that inferred. The shipped fixture had them coincide.
///
/// Mutation recipes:
///
/// - applying the default unconditionally (the pre-fix code) flips
///   `zzMismACall` / `zzMismBCall` to `Primitive(String)` and
///   `zpExplicitCall` / `rvDefaultedFlowCall` to the declared default,
///   while leaving every argument-free row green;
/// - substituting `unknown` regardless of the declared default flips the
///   four "the default IS the answer" rows and leaves the mismatch rows
///   green;
/// - dropping the explicit-type-arguments bit flips only
///   `zpExplicitCall`;
/// - dropping the supplied-ordinal test (treating ANY occurrence in a
///   parameter type as inferable) flips `zpNotSuppliedCall` ALONE to
///   `unknown`. `zpUnusedTPCall`'s `ZE` occurs in NO parameter type at
///   all, so `first_parameter_occurrence == None` already decides it and
///   that row does not discriminate this refinement — it discriminates
///   the COARSER "any argument-bearing call infers" mutation. An earlier
///   record naming both rows was wrong.
#[test]
fn flow_return_callee_clause_default_applies_only_when_inference_has_no_candidate() {
    let host = make_r5_host();

    // Inference HAS a candidate: the inferred type — the un-widened
    // literal for a literal argument, the explicit type argument when
    // authored — never the declared default.
    for (name, expected) in [
        (
            "zzMismACall",
            NodeShape::Other("Literal(Number(1.0))".to_string()),
        ),
        ("zzMismBCall", NodeShape::Primitive(PrimitiveKind::Boolean)),
        (
            "zpExplicitCall",
            NodeShape::Primitive(PrimitiveKind::String),
        ),
        (
            "rvDefaultedFlowCall",
            NodeShape::Other("Literal(Number(1.0))".to_string()),
        ),
    ] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                assert_eq!(
                    node_shape(dispatch, node),
                    expected,
                    "{name}'s call site produces an inference candidate, so the \
                     declared default is NOT the answer — the default would be \
                     confidently wrong"
                );
            },
        );
    }

    // Inference can produce NO candidate: the declared default is exact,
    // on the declared-carrier route, the body-derived flow route, and
    // both argument-bearing shapes whose parameter is still uninferable.
    for name in [
        "rvDefaultedCall",
        "rvDefaultedFlowFreeCall",
        "zpUnusedTPCall",
        "zpNotSuppliedCall",
    ] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                assert_eq!(
                    node_shape(dispatch, node),
                    NodeShape::Primitive(PrimitiveKind::Number),
                    "{name}'s clause parameter can get no inference candidate, so \
                     the callee's own declaration resolves it without inference"
                );
            },
        );
    }

    // CONTROL — a clause with NO default keeps `unknown`, which is the
    // checker's own answer for a bare `T` return with nothing to infer.
    assert_clean_warm(
        &host,
        "gcBareInferred",
        TypeExpr::Primitive(PrimitiveName::Unknown),
    );
}

/// A caller instantiating a callee's clause never claims a FOREIGN
/// declaration that merely SHARES the parameter's name.
///
/// The resolved-`DeclRef` claim exists for one provenance only: a
/// declaration's own clause is NOT in scope where its DECLARED return
/// locator lowers (file owner scope), so `f<Item>(): Item` interns its
/// return as the file-scope `interface Item` — the wrong symbol, which
/// the caller must claim. A BODY-DERIVED return is the opposite case: it
/// is evaluated in the callee's own frame where the clause IS bound, so
/// every occurrence of a clause parameter there is already a `TypeParam`
/// binder, and a resolved `DeclRef` reached through such an arm is by
/// construction a different symbol.
///
/// A NAME-scoped claim erases it — an exactly-correct arm destroyed and
/// republished as `unknown`, cleanly and warm.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict`):
///
/// ```text
/// callAye():   1 | QQ            ← arm 2 is `bee`'s declared return, the INTERFACE
/// callAyeG():  1 | QQ            ← same, through a GENERIC callee
/// zOuterCall(): { v: ZItem; }    ← the foreign name nested in a structural return
/// ```
///
/// Mutation recipes:
///
/// - claiming resolved `DeclRef`s on the body-derived route (the
///   pre-fix, name-only claim) republishes every row's foreign arm as
///   `unknown`;
/// - dropping the claim from the DECLARED-carrier route flips the two
///   controls below (`shUseFirst` / `callsShDecl`), whose callees'
///   declared returns genuinely misresolve to the file-scope interface.
#[test]
fn flow_return_clause_claim_never_erases_a_foreign_same_named_declaration() {
    let host = make_r5_host();

    // The union's foreign arm SURVIVES; the callee's own binder arm is
    // instantiated away.
    for name in ["callAye", "callAyeG"] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                let arms = union_members(dispatch, node);
                let shapes: Vec<NodeShape> =
                    arms.iter().map(|arm| node_shape(dispatch, *arm)).collect();
                assert!(
                    shapes.contains(&decl_ref("QQ")),
                    "{name}: the callee's DECLARED return is the file-scope \
                     interface `QQ`, a symbol this caller's clause never shadows — \
                     got {shapes:?}"
                );
            },
        );
    }

    // The foreign name NESTED under a structural return.
    r5_node(
        &host,
        "zOuterCall",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let member = object_member(dispatch, node, "v");
            assert_eq!(
                node_shape(dispatch, member),
                decl_ref("ZItem"),
                "the foreign interface nested in the callee's structural return \
                 is not the caller's clause parameter"
            );
        },
    );

    // CONTROLS — the DECLARED-carrier route, where the claim is the
    // whole point: `shFirst` / `shDecl` annotate returns that owner-scope
    // lowering misresolves to the same-named file-scope interface.
    // `shUseFirst`'s argument is a parameter reference, so the executor is
    // undecidable and the claim's `unknown` stands; `callsShDecl` supplies
    // a literal and the executor infers the clause directly.
    r5_node(
        &host,
        "shUseFirst",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::Number),
                "shUseFirst's callee DECLARES its return, where its own clause is \
                 out of scope — the resolved head IS the clause parameter"
            );
        },
    );
    r5_node(
        &host,
        "callsShDecl",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::String),
                "callsShDecl infers the clause from the literal argument — the \
                 inferred literal, never the shadowed interface"
            );
        },
    );

    // The IIFE route: a nested function value's signature is COMPOSED
    // from its own body, so it is clause-scoped exactly like a
    // body-derived flow return.
    r5_node(
        &host,
        "iifeForeign",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let shapes: Vec<NodeShape> = union_members(dispatch, node)
                .iter()
                .map(|arm| node_shape(dispatch, *arm))
                .collect();
            assert!(
                shapes.contains(&decl_ref("QQ")),
                "the IIFE's composed signature is clause-scoped: the foreign \
                 interface reached through its body survives — got {shapes:?}"
            );
        },
    );

    // CONTROLS — the OWNER-SCOPE routes: a resolved callee VALUE TYPE
    // (`symCall`, through the annotated-callee rail) and a function-typed
    // BINDING (`bindDeclCall`, through the parameter rail). Both lower
    // their callee's clause in file owner scope, where it is invisible,
    // so the resolved same-named head IS the clause parameter and the
    // claim must still reach it. `symCall` is not call-resolved and keeps
    // the claim's `unknown`; `bindDeclCall` infers from the literal
    // argument — a ROOTLESS winner (a binding's function type has no
    // authored occurrence), so the value is served but never admitted.
    r5_node(
        &host,
        "symCall",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::Unknown),
                "symCall's callee type lowered in owner scope, where its own \
                 clause is invisible — the resolved head IS the parameter"
            );
        },
    );
    r5_node_unadmitted(
        &host,
        "bindDeclCall",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::String),
                "bindDeclCall infers the clause from the literal argument — the \
                 widened literal of the checker's `string`, never the shadowed declaration"
            );
        },
    );

    // CONTROL — a non-generic callee declares no clause at all, so its
    // resolved return reference survives untouched on either route.
    r5_node(
        &host,
        "shUsePlainItem",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(node_shape(dispatch, node), decl_ref("ShItem"));
        },
    );
}

/// ONE callee, reached two ways, answers ONE thing.
///
/// `nbUse` binds an arrow to a local `const` and calls the binding
/// (`SliceCall::OnBinding`); `nbIife` invokes the SAME arrow body
/// immediately (`SliceCall::Nested`). Both callee values are a signature
/// COMPOSED from that body, so both are [`ReturnOrigin::ClauseScoped`]:
/// the interface `NB` reached through the body is a FOREIGN symbol the
/// arrow's same-named clause parameter never shadows.
///
/// Spelling the binding route owner-scope-declared claims that interface
/// BY NAME and republishes the arm as `unknown` — cleanly, warm, and in
/// disagreement with the IIFE route about a single callee. `ncUse` is
/// the control that isolates it: the identical body with the clause
/// renamed keeps the arm, so the erasure is purely the name collision.
///
/// The two routes agree on the VALUE but not on ADMISSION: the IIFE's
/// callee has its authored occurrence and warm-admits, while the local
/// binding's arrow is ROOTLESS — the value is served but the slot is
/// never warm-admitted.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict --ignoreConfig`,
/// read through the TWO-STEP probe `const v = f(…); const p: null = v;`
/// — a one-step `const p: null = f(…)` contextually types the call and
/// is not a sound reading here):
///
/// ```text
/// nbUse(true):  NB | null
/// nbIife(true): NB | null
/// ncUse(true):  NB | null
/// ```
///
/// Mutation recipe: restoring `ReturnOrigin::OwnerScopeDeclared` at the
/// `SliceCall::OnBinding` arm flips `nbUse` alone to
/// `Union([Unknown, Null])` while `nbIife` and `ncUse` stay green —
/// which is precisely the two-routes-disagree state.
#[test]
fn flow_return_binding_and_iife_routes_agree_about_one_callee() {
    let host = make_r5_host();

    // The BINDING route: the arrow is ROOTLESS, so the value is served
    // but never warm-admitted — for `ncUse` (renamed clause) alike.
    for name in ["nbUse", "ncUse"] {
        r5_node_unadmitted(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                let shapes: Vec<NodeShape> = union_members(dispatch, node)
                    .iter()
                    .map(|arm| node_shape(dispatch, *arm))
                    .collect();
                assert!(
                    shapes.contains(&decl_ref("NB")),
                    "{name}: the callee's body-derived arm is the file-scope \
                     interface `NB`, a symbol the callee's own clause never \
                     shadows — got {shapes:?}"
                );
                assert!(
                    shapes.contains(&NodeShape::Primitive(PrimitiveKind::Null)),
                    "{name}: the `null` arm survives — got {shapes:?}"
                );
                assert!(
                    !shapes.contains(&NodeShape::Primitive(PrimitiveKind::Unknown)),
                    "{name}: nothing in this answer is the instantiation interim \
                     — got {shapes:?}"
                );
            },
        );
    }

    // The IIFE route: same body, same answer — and this callee DOES
    // warm-admit.
    for name in ["nbIife"] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                let shapes: Vec<NodeShape> = union_members(dispatch, node)
                    .iter()
                    .map(|arm| node_shape(dispatch, *arm))
                    .collect();
                assert!(
                    shapes.contains(&decl_ref("NB")),
                    "{name}: the callee's body-derived arm is the file-scope \
                     interface `NB`, a symbol the callee's own clause never \
                     shadows — got {shapes:?}"
                );
                assert!(
                    shapes.contains(&NodeShape::Primitive(PrimitiveKind::Null)),
                    "{name}: the `null` arm survives — got {shapes:?}"
                );
                assert!(
                    !shapes.contains(&NodeShape::Primitive(PrimitiveKind::Unknown)),
                    "{name}: nothing in this answer is the instantiation interim \
                     — got {shapes:?}"
                );
            },
        );
    }
}

/// An AMBIENT overload group resolves exactly like a bodied one.
///
/// The per-file function-program index carries ONE entry per overload
/// group, so the direct-call rail reaches exactly one declaration of a
/// set the language resolves by ARGUMENTS. For a bodied group that entry
/// is the hidden implementation; for an AMBIENT group there is no
/// implementation at all and the entry is simply the LAST declaration —
/// while TypeScript picks the FIRST matching one. The executor resolves
/// both halves of the class alike: the FIRST APPLICABLE signature in
/// declaration order, never whichever entry the index happens to hold.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict`):
///
/// ```text
/// amb3Call():     "A"       ← the FIRST matching overload
/// ovXCall():      string
/// ovSingleCall(): string
/// ```
///
/// The `Literal(String("a"))` rows are the un-widened literals of the
/// checker's widened `string`.
///
/// Mutation recipe: reading the index's LAST declaration republishes
/// `Literal(String("C"))` for `amb3Call` and the implementation's `any`
/// for `ovXCall`, and leaves the lone-signature control green.
#[test]
fn flow_return_ambient_overload_group_resolves_like_a_bodied_one() {
    let host = make_r5_host();

    r5_node(
        &host,
        "amb3Call",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Other("Literal(String(\"A\"))".to_string()),
                "an ambient group resolves to the FIRST applicable declaration, \
                 never the index's last entry"
            );
        },
    );

    // An AMBIENT declared literal return is the literal itself, exactly —
    // no inference ran and nothing widens.
    for (name, literal) in [("amb3Call", "\"A\""), ("tnAmbBare", "\"TA\"")] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                assert_eq!(
                    node_shape(dispatch, node),
                    NodeShape::Other(format!("Literal(String({literal}))")),
                    "the ambient overload's declared literal return is exact"
                );
            },
        );
    }

    // CONTROL — the bodied group resolves the same way.
    r5_node(
        &host,
        "ovXCall",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::String),
                "a bodied group resolves to the first applicable overload — \
                 never the hidden implementation"
            );
        },
    );

    // CONTROL — a LONE signature resolves identically, bodied or not: the
    // rule is overload VISIBILITY, not "any function with a body".
    r5_node(
        &host,
        "ovSingleCall",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Primitive(PrimitiveKind::String),
                "a lone visible signature instantiates its clause from the \
                 argument"
            );
        },
    );
}

/// The `ReturnType<typeof f>` MEMBER route applies the same clause
/// policy as its whole-return sibling.
///
/// `ReturnType<…>` is a signature UTILITY, not a call: it instantiates
/// every free signature parameter at `unknown` and never consults a
/// declared default (there is no call site to be argument-free). The
/// whole-return route does exactly that; the member route — one path
/// segment longer over the SAME carrier and the SAME callee — returned
/// the flow return's raw member position, publishing the callee's own
/// binder as the consumer's value.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict`), over
/// `mpFlow<MG = number>(x?: MG) { return { m: x }; }`:
///
/// ```text
/// ReturnType<typeof mpFlow>        : { m: unknown; }
/// ReturnType<typeof mpFlow>["m"]   : unknown
/// ```
///
/// Note both are `unknown`, NOT the declared default `number`: the
/// utility has no call site, so the "argument-free call takes the
/// default" rule does not reach it. A member route that took the CALL
/// policy would answer `number`, and one that took no policy answers the
/// raw binder — three distinguishable states, and only `unknown` is the
/// checker's.
///
/// Mutation recipes:
///
/// - removing the member route's clause instantiation republishes
///   `TypeParam("MG")` — the callee's own binder as the consumer's
///   value, which an enclosing same-named clause then substitutes;
/// - giving the member route the CALL-site policy republishes
///   `Primitive(Number)` (the declared default) and leaves the
///   whole-return control at `unknown`, so the two routes disagree about
///   one callee.
#[test]
fn flow_return_type_member_route_shares_the_whole_return_clause_policy() {
    let host = make_r5_host();

    // The MEMBER route: one pending named segment over the
    // `ReturnType<typeof …>` carrier.
    r5_node(
        &host,
        "mpMemberUse",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let carrier = indexed_access_object(dispatch, node);
            let member = project_member_path(dispatch, carrier, "m");
            assert_eq!(
                node_shape(dispatch, member),
                NodeShape::Primitive(PrimitiveKind::Unknown),
                "the member route is the signature UTILITY's policy: every free \
                 clause parameter is `unknown`, never the callee's raw binder \
                 and never the declared default"
            );
        },
    );

    // CONTROL — the WHOLE-return route, the same carrier and the same
    // callee reached at the TERMINAL hop instead of a pending one.
    r5_node(
        &host,
        "mpWholeUse",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let whole = project_member_path(dispatch, node, "w");
            let member = object_member(dispatch, whole, "m");
            assert_eq!(
                node_shape(dispatch, member),
                NodeShape::Primitive(PrimitiveKind::Unknown),
                "the whole-return route already applied the utility policy"
            );
        },
    );
}

/// A CALL reached through a COMPOSITE expression is still a call.
///
/// `lower_expr` used to end in a `_ => self.lower_leaf(…)` wildcard, so
/// every expression form without its own arm folded through the shared
/// SHALLOW pass — which has no frame and no resolver, and answers a call
/// it meets with an UNREDUCED `ReturnType<callee>` carrier. Published as
/// a value that carrier hands out the callee's own type-parameter binders
/// and skips its overload group entirely, cleanly and warm:
///
/// ```text
/// tnAmbTernary   deg=None cands=1  Union([ReturnType<…>, ×2])   ← the size gate BYPASSED
/// tnGenericTernary                 Union([ReturnType<Fn<…TNG…>>, ×2])
/// tnAmbArray                       Array{ element: ReturnType<…> }
/// ```
///
/// while the `if` / `return` twin of the same body went through
/// `SliceExpr::Call` and degraded correctly. Two spellings of one body,
/// two answers.
///
/// Two halves close it. A CONDITIONAL is a `Branches` disposition of the
/// shared value-structural classifier, so both branches lower as flow
/// expressions and their calls ride the one call sink, joining through
/// the same normalizing interner the contributor join uses. And a leaf
/// answer that would EMBED the carrier goes through `lower_leaf`'s
/// call-carrier gate and FAILS CLOSED rather than publishing it.
///
/// Two things that gate does NOT claim, stated because the round that
/// landed it read as if it did. First, exhaustiveness now lives in the
/// shared classifier (`value_descent`), not in `lower_expr`: `lower_expr`
/// ends in `other => match value_descent(other)`, and it is the
/// classifier's wildcard-free match over `Expression` that refuses to
/// compile for a new variant — which is the point, because the SKELETON's
/// `open_site` reads the same verdict, so one half can no longer descend
/// where the other does not. Second, the gate covers leaf answers that
/// CARRY a call return; it does not make every leaf form call-exact. Most
/// leaf forms — a logical operand, a sequence, a non-null assertion, a
/// member base — answer the shallow pass's `any`, which is reached before
/// the gate and is not a carrier. Those rows are asserted as `any` in
/// `flow_return_leaf_answered_call_forms_publish_any_not_a_carrier`.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`, read as the wrapper's `ReturnType` through
/// `declare const w: ReturnType<typeof f>; const p: null = w;`):
///
/// ```text
/// tnAmbTernary:     "TA" | "TB"      tnAmbIf:      "TA" | "TB"
/// tnAmbArray:       "TA"[]           tnGenericTernary: string | number
/// tnGenericArray:   string[]         tnLitTernary: 1 | 2
/// ```
///
/// The overload rows now RESOLVE — the executor picks the first
/// applicable signature at a bare call, in an `if` arm, and in a ternary
/// arm alike (one callee, one answer). The generic rows resolve exactly
/// too: explicit type arguments instantiate the clause, and the ternary
/// joins both arms. `tnLitTernary` / `tnLitIf` keep their un-widened
/// literals — the checker's own `1 | 2`.
///
/// Mutation recipe: dispositioning `ConditionalExpression` as
/// `ValueDescent::Leaf` in the shared classifier flips `tnAmbTernary`
/// back to `deg=None cands=1` and puts a `ReturnType` `InstantiationRef`
/// inside every generic row; deleting only the `lower_leaf` gate flips
/// the two array rows back to a published carrier.
#[test]
fn flow_return_calls_in_composite_expressions_never_publish_the_raw_callee_return() {
    let host = make_r5_host();

    // An overload group resolves identically at a bare call, in an `if`
    // arm, and in a TERNARY arm — one callee, one answer.
    r5_node(
        &host,
        "tnAmbBare",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Other("Literal(String(\"TA\"))".to_string()),
                "the bare call resolves to the first applicable overload"
            );
        },
    );
    for name in ["tnAmbIf", "tnAmbTernary"] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                let shapes: Vec<NodeShape> = union_members(dispatch, node)
                    .iter()
                    .map(|arm| node_shape(dispatch, *arm))
                    .collect();
                assert_eq!(
                    shapes,
                    vec![
                        NodeShape::Other("Literal(String(\"TA\"))".to_string()),
                        NodeShape::Other("Literal(String(\"TB\"))".to_string()),
                    ],
                    "{name}: both arms resolve to the first applicable overload"
                );
            },
        );
    }

    // A generic callee in a ternary arm instantiates its clause from the
    // explicit type arguments — exactly, never the callee's own binder.
    r5_node(
        &host,
        "tnGenericTernary",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let shapes: Vec<NodeShape> = union_members(dispatch, node)
                .iter()
                .map(|arm| node_shape(dispatch, *arm))
                .collect();
            assert_eq!(
                shapes,
                vec![
                    NodeShape::Primitive(PrimitiveKind::String),
                    NodeShape::Primitive(PrimitiveKind::Number),
                ],
                "the ternary's arms take the same exact answer a bare call takes"
            );
        },
    );

    // A form with NO structural arm fails CLOSED rather than publishing
    // the carrier.
    for name in ["tnAmbArray", "tnGenericArray"] {
        assert_fails_closed(&host, name);
    }

    // CONTROL — a call-free ternary is untouched: same answer as its
    // `if` / `return` twin, and the checker's own `1 | 2` (NOT widened).
    for name in ["tnLitTernary", "tnLitIf"] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                let shapes: Vec<NodeShape> = union_members(dispatch, node)
                    .iter()
                    .map(|arm| node_shape(dispatch, *arm))
                    .collect();
                assert_eq!(
                    shapes,
                    vec![
                        NodeShape::Other("Literal(Number(1.0))".to_string()),
                        NodeShape::Other("Literal(Number(2.0))".to_string()),
                    ],
                    "{name}: a call-free ternary keeps its literal arms unwidened"
                );
            },
        );
    }
}

/// A METHOD-position overload GROUP is an overload group.
///
/// Every same-name method contributor is retained on the surface, but a
/// member projection is first-wins by design — right for a property,
/// silently truncating for an overload set. So `ovObjValue.m(1)` reached
/// the call rail as ONE signature and published the FIRST overload's
/// return, cleanly and warm, while the identical group written as bare
/// CALL SIGNATURES (`{ (x: string): "A"; (x: number): "B" }`) degraded
/// correctly. Four shapes were affected — class method, object-type
/// method, interface method, and a method reached through an
/// INTERSECTION arm.
///
/// The group now projects as the canonical overload-group carrier — an
/// object whose CALL SIGNATURES are the contributors, the same shape
/// `build_typeof` mints for a top-level function overload group — so the
/// existing size gate (`signature_bucket_arity(.., Call) > 1`) sees the
/// real arity and `select_signature_function` keeps reading the LAST
/// overload for the signature utilities, exactly as it does for `f`.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`, read as the wrapper's `ReturnType` through
/// `declare const w: ReturnType<typeof f>; const p: null = w;`):
///
/// ```text
/// ovClassMethodCall():     "MB"      ← the SECOND overload, not the first
/// ovObjMethodCall():       "PB"
/// ovIfaceMethodCall():     "IB"
/// ovIntersectMethodCall(): "MB"
/// ovSoloMethodCall():      "SOLO"    ← a LONE method is not a group
/// ```
///
/// The executor resolves the group by ARGUMENTS: the first applicable
/// contributor — which for these fixtures is the SECOND overload, exactly
/// as the checker picks it — never first-wins and never the marker.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`, read as the wrapper's `ReturnType` through
/// `declare const w: ReturnType<typeof f>; const p: null = w;`):
///
/// ```text
/// ovClassMethodCall():     "MB"      ← the SECOND overload, not the first
/// ovObjMethodCall():       "PB"
/// ovIfaceMethodCall():     "IB"
/// ovIntersectMethodCall(): "MB"
/// ovSoloMethodCall():      "SOLO"    ← a LONE method is not a group
/// ```
///
/// Mutation recipe: returning `None` unconditionally from
/// `SurfaceView::project_known_key_overload_group` restores first-wins
/// and flips all four group rows to the FIRST overload's literal
/// (`"MA"` / `"PA"` / `"IA"` / `"MA"`), while leaving
/// both controls green; gating the group on `>= 1` rather than `>= 2`
/// collisions flips `ovSoloMethodCall`; dropping the all-methods test
/// flips `ovPropRead`.
#[test]
fn flow_return_method_position_overload_groups_resolve_by_arguments() {
    let host = make_r5_host();

    for (name, expected) in [
        ("ovClassMethodCall", "MB"),
        ("ovObjMethodCall", "PB"),
        ("ovIfaceMethodCall", "IB"),
        ("ovIntersectMethodCall", "MB"),
    ] {
        assert_clean_warm(&host, name, string_lit(expected));
    }

    // CONTROLS — a LONE method and a plain PROPERTY keep their exact
    // first-wins answer: the gate must not over-fire on either.
    assert_clean_warm(&host, "ovSoloMethodCall", string_lit("SOLO"));
    assert_clean_warm(&host, "ovPropRead", string_lit("PROP"));
}

/// The demand PLANNER and the content LOWERING descend through exactly
/// the same expression forms.
///
/// The substrate has two halves that must agree about which forms have
/// value-contributing sub-expressions: the skeleton opens a tracked
/// expression site per such sub-expression and the graph makes it a
/// value provider (so the planner reaches it), and the content lowering
/// descends into it and gates an object member value on whether the plan
/// VALUE-selected it. The round that gave a conditional expression its
/// structural arm extended the CONTENT half only. The planner had never
/// needed to descend into a branch, so it still did not — and every
/// object literal in a branch lowered with each member value on the
/// typed `SliceExpr::Elided` carrier, which the union arm turns into a
/// whole-evaluation failure:
///
/// ```text
/// k ? { a: 1 } : 2                 Error(Miss) cands=0
/// k ? { a: 1 } : { a: 2 }          Error(Miss) cands=0
/// k ? { a: 1 } : { b: 2 }          Error(Miss) cands=0
/// const q = 1; k ? { a: q } : 2    Error(Miss) cands=0
/// k ? { m() { return 1 } } : 2     Error(Miss) cands=0
/// k ? { a: { b: 1 } } : 2          Error(Miss) cands=0
/// { a: k ? { b: 1 } : 2 }          Error(Miss) cands=0
/// k ? (k ? { a: 1 } : 2) : 3       Error(Miss) cands=0
/// ```
///
/// while `k ? {} : 2` (no member value to elide) stayed green, which is
/// exactly the signature of an UNDER-SELECTED plan rather than a broken
/// arm.
///
/// The fix is one classifier, `verter_semantic::analysis::flow::
/// value_descent`, with ONE wildcard-free match over `Expression` and two
/// consumers: the skeleton's `open_site` and the content half's
/// `lower_expr`. Neither carries a wildcard over `Expression` any more,
/// so a new variant does not compile until it is dispositioned in the
/// classifier — and both halves inherit that disposition in the same
/// change. `ValueDescent::TypeCarrier` is a NAMED variant for the one
/// asymmetric case (`x as const`: the planner descends, the content half
/// leaf-lowers), because over-selection is harmless and under-selection
/// is this bug.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`, read as the wrapper's `ReturnType` through
/// `declare const w: ReturnType<typeof f>; const p: null = w;`):
///
/// ```text
/// ctObj:           2 | { a: number }
/// ctObjBoth:       { a: number }
/// ctObjDisjoint:   { a: number; b?: undefined } | { a?: undefined; b: number }
/// ctObjLocalRead:  2 | { a: number }
/// ctObjMethod:     2 | { m(): number }
/// ctObjNested:     2 | { a: { b: number } }
/// ctObjInObj:      { a: number | { b: number } }
/// ctNestedTernary: 2 | 3 | { a: number }
/// ctObjEmpty:      {}
/// ctArray:         number[]
/// ctIdent:         2 | true
/// ctNull:          1 | null
/// ctArrow:         () => number
/// ```
///
/// Four rows carry a recorded, PRE-EXISTING divergence from that oracle,
/// none of them introduced or removed here, all of them shape-level and
/// none of them a leak:
///
/// - `ctObjBoth` publishes two structurally identical arms where the
///   checker publishes one. Union dedup is by interned NODE id and two
///   object literals at different spans intern distinct nodes; the
///   `if` / `return` twin dedups the same way (`arms.contains`), so this
///   is a property of the whole rail, not of the branch join.
/// - `ctObjDisjoint` publishes no `?: undefined` normalization.
/// - `ctNestedTernary` publishes a NESTED union
///   (`Union([Union([2, { a }]), 3])`);
///   `intern_normalized_union_or_intersection` sorts and dedups but does
///   not flatten a union arm.
/// - `ctObjEmpty` publishes `2 | {}` where the checker's subtype
///   reduction collapses it to `{}`; `ctIdent` publishes `boolean | 2`
///   where the checker narrows the consequent to `true`.
///
/// Mutation recipe: dispositioning `ObjectExpression` as
/// `ValueDescent::Leaf` in the classifier flips every object row to a
/// whole-literal leaf answer; deleting the `BranchJoin` `ValueDef` edges
/// in `build_function_flow_graph` restores the exact `Error(Miss)` table
/// above with `ctObjEmpty` / `ctArray` / `ctIdent` / `ctNull` / `ctArrow`
/// left green.
#[test]
fn flow_return_conditional_branches_are_planned_and_lowered_by_one_descent() {
    let host = make_r5_host();

    // Every row below was `Error(Miss)` cands=0 before the planner learnt
    // the same descent the content half performs.
    for (name, expected) in [
        ("ctObj", "{a:number}|2"),
        // The recorded dedup divergence: the checker publishes ONE arm.
        ("ctObjBoth", "{a:number}|{a:number}"),
        ("ctObjDisjoint", "{a:number}|{b:number}"),
        ("ctObjLocalRead", "2|{a:number}"),
        ("ctObjMethod", "2|{m():number}"),
        ("ctObjNested", "2|{a:{b:number}}"),
        // A branch join reached as an object MEMBER value, through the
        // member's own path-write edge.
        ("ctObjInObj", "{a:2|{b:number}}"),
        // The recorded nesting divergence: the checker flattens to
        // `2 | 3 | { a: number }`.
        ("ctNestedTernary", "(2|{a:number})|3"),
        // CONTROLS — rows that were already green must be unchanged, so
        // the new descent is proven not to have moved them.
        ("ctObjEmpty", "2|{}"),
        ("ctArray", "number[]"),
        ("ctIdent", "boolean|2"),
        ("ctNull", "1|null"),
        ("ctArrow", "()=>number"),
    ] {
        let outcome = r5_eval(&host, name).unwrap_or_else(|| panic!("{name} must produce a value"));
        assert_eq!(outcome.degradation, None, "{name} must evaluate clean");
        assert_eq!(
            outcome.candidates, 1,
            "{name} must warm-admit exactly one candidate"
        );
        assert_eq!(shape_of(&outcome.ty), expected, "{name} published shape");
    }
}

/// A compact, span-free spelling of one published `TypeExpr`, so a
/// branch-join assertion compares MEANING and not member spans.
fn shape_of(ty: &TypeExpr) -> String {
    match ty {
        TypeExpr::Literal(verter_type_expr::LiteralValue::Number(n)) => {
            let rounded = *n as i64;
            if (*n - rounded as f64).abs() < f64::EPSILON {
                rounded.to_string()
            } else {
                n.to_string()
            }
        }
        TypeExpr::Literal(verter_type_expr::LiteralValue::String(s)) => format!("\"{s}\""),
        TypeExpr::Primitive(name) => format!("{name:?}").to_lowercase(),
        // A nested union arm is PARENTHESISED: the substrate does not
        // flatten one, and a spelling that silently joined it would hide
        // exactly that from an assertion.
        TypeExpr::Union(arms) => arms
            .iter()
            .map(|arm| match arm {
                TypeExpr::Union(_) => format!("({})", shape_of(arm)),
                other => shape_of(other),
            })
            .collect::<Vec<String>>()
            .join("|"),
        TypeExpr::Array { element, .. } => format!("{}[]", shape_of(element)),
        TypeExpr::Function(function) => format!(
            "()=>{}",
            function
                .return_type
                .as_deref()
                .map_or_else(|| "?".to_string(), shape_of)
        ),
        TypeExpr::Object(object) => {
            let members: Vec<String> = object
                .properties
                .iter()
                .map(|member| match member {
                    verter_type_expr::ObjectMember::Property(property) => format!(
                        "{}:{}",
                        property.key.as_string().unwrap_or_default(),
                        shape_of(&property.ty)
                    ),
                    verter_type_expr::ObjectMember::Method(method) => format!(
                        "{}():{}",
                        method.key.as_string().unwrap_or_default(),
                        method
                            .function
                            .return_type
                            .as_deref()
                            .map_or_else(|| "?".to_string(), shape_of)
                    ),
                    other => format!("{other:?}"),
                })
                .collect();
            format!("{{{}}}", members.join(","))
        }
        other => format!("{other:?}"),
    }
}

/// The METHOD-position overload-group carrier applies TypeScript's
/// overload VISIBILITY rule — the same one `build_typeof` applies.
///
/// The carrier that made a same-named method group reach the call rail's
/// size gate filtered on `visibility.is_public() && method_kind ==
/// Method` and stopped there. `build_typeof` additionally HIDES the
/// trailing implementation signature of a multi-signature group, and
/// `select_signature_function` documents that filter as its PRECONDITION
/// before reading the LAST overload. The carrier fed it an unfiltered
/// bucket, so for a group with an implementation the LAST overload WAS
/// the implementation:
///
/// ```text
///                        before          after       checker
/// OvImpl['m']            3 signatures    2           2
///   incl. (x: any): any
/// ReturnType<…>          any             "MB"        "MB"
/// Parameters<…>          [x: any]        [x: number] [x: number]
/// ```
///
/// Both states were wrong; the carrier's was strictly worse — `any`
/// erases every downstream check, and the published surface leaked a
/// signature TypeScript hides. Two claims made when the carrier landed
/// are corrected with it: it was NOT "byte-identical in shape to what
/// `build_typeof` mints" (that producer filtered and this one did not),
/// and `select_signature_function` did NOT read "the last overload
/// exactly as it does for `f`" (for a bodied group it read the
/// implementation). The rule now has ONE home,
/// `semantic_query::visible_overload_ordinals`, and both producers call
/// it, so the two cannot drift again — a group reached as `typeof f` and
/// the same group reached as `C['m']` publish the same contributors by
/// construction.
///
/// The blast radius is the SHARED PathWalker known-key Object hop —
/// component-meta published prop types, indexed access, every signature
/// utility — not the flow rail, which degrades on a group of arity > 1
/// either way. Every overload fixture that shipped with the carrier is
/// implementation-FREE, so the suite structurally could not catch it;
/// `OvImpl` below is the bodied fixture that closes that gap.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`):
///
/// ```text
/// OvImpl['m']:        { (x: string): "MA"; (x: number): "MB"; }
/// ReturnType<…>:      "MB"
/// Parameters<…>:      [x: number]
/// OvAmbient['m']:     { (x: string): "AA"; (x: number): "AB"; }
/// OvSingleImpl['m']:  (x: string) => "OA"
/// OvLoneImpl['m']:    (x: string) => "LA"
/// ```
///
/// Mutation recipe: dropping `!has_implementation_body` from
/// `visible_overload_ordinals` puts `(x: any): any` back as `OvImpl`'s
/// third signature AND flips the `select_signature_function` rows to
/// `any` / `[x: any]`; replacing its all-bodied fallback with an empty
/// selection flips `OvSingleImpl` and `OvLoneImpl`.
#[test]
fn method_overload_group_carrier_hides_the_implementation_signature() {
    let host = make_r5_host();

    /// The group carrier's call-signature list as `(param) -> return`
    /// shapes, projected through the SHARED PathWalker member hop.
    #[track_caller]
    fn group_signatures(host: &Arc<VerterHost>, name: &str) -> Vec<(NodeShape, NodeShape)> {
        r5_node(
            host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                let base = indexed_access_object(dispatch, node);
                let projected = project_member_path(dispatch, base, "m");
                match dispatch.graph().node_data(projected).as_deref() {
                    Some(SemanticNodeData::Object(view)) => view
                        .call_signatures
                        .iter()
                        .map(|sig| {
                            let parts = signature_parts(dispatch, *sig);
                            (
                                node_shape(dispatch, parts.params[0]),
                                node_shape(dispatch, parts.return_type),
                            )
                        })
                        .collect(),
                    other => panic!("{name}: expected the group carrier, got {other:?}"),
                }
            },
        )
    }
    let literal = |text: &str| NodeShape::Other(format!("Literal(String({text:?}))"));

    // A BODIED group publishes exactly the bodiless overloads, in source
    // order — the implementation is hidden.
    assert_eq!(
        group_signatures(&host, "ovImplGroup"),
        vec![
            (NodeShape::Primitive(PrimitiveKind::String), literal("MA")),
            (NodeShape::Primitive(PrimitiveKind::Number), literal("MB")),
        ],
        "the implementation signature must not reach the published carrier"
    );
    // CONTROL — an AMBIENT group has no implementation to hide.
    assert_eq!(
        group_signatures(&host, "ovAmbientGroup"),
        vec![
            (NodeShape::Primitive(PrimitiveKind::String), literal("AA")),
            (NodeShape::Primitive(PrimitiveKind::Number), literal("AB")),
        ],
    );
    // CONTROL — one declared overload plus an implementation leaves ONE
    // visible contributor.
    assert_eq!(
        group_signatures(&host, "ovSingleImplGroup"),
        vec![(NodeShape::Primitive(PrimitiveKind::String), literal("OA"))],
    );

    // END TO END: the utility route. `select_signature_function` reads
    // the LAST visible overload, which is what `ReturnType<C['m']>` and
    // `Parameters<C['m']>` publish.
    for (name, expected_param, expected_return) in [
        ("ovImplGroup", PrimitiveKind::Number, "MB"),
        ("ovAmbientGroup", PrimitiveKind::Number, "AB"),
    ] {
        r5_node(
            &host,
            name,
            FunctionPartIdentity::DeclarationBody,
            |dispatch, node| {
                let base = indexed_access_object(dispatch, node);
                let projected = project_member_path(dispatch, base, "m");
                let selected = dispatch
                    .select_signature_function(projected, super::build::SignatureBucket::Call)
                    .unwrap_or_else(|| panic!("{name}: the group must select a signature"));
                let parts = signature_parts(dispatch, selected);
                assert_eq!(
                    node_shape(dispatch, parts.return_type),
                    literal(expected_return),
                    "{name}: ReturnType reads the last VISIBLE overload"
                );
                assert_eq!(
                    node_shape(dispatch, parts.params[0]),
                    NodeShape::Primitive(expected_param),
                    "{name}: Parameters reads the last VISIBLE overload"
                );
            },
        );
    }

    // CONTROL — a LONE BODIED method is not a group at all (the
    // collision gate needs two), and stays the plain first-wins
    // `Signature` the carrier never touches.
    r5_node(
        &host,
        "ovLoneImplGroup",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            let base = indexed_access_object(dispatch, node);
            let projected = project_member_path(dispatch, base, "m");
            let parts = signature_parts(dispatch, projected);
            assert_eq!(node_shape(dispatch, parts.return_type), literal("LA"));
        },
    );
}

/// SELF-RECURSION through a ternary is NOT the `if` spelling's twin, and
/// the checker is where that asymmetry comes from.
///
/// The union arm converts a coinductive HOLD into `Err` for the whole
/// ternary, so `return n > 0 ? f(n - 1) : 0` fails closed while
/// `if (n > 0) return f(n - 1); return 0` converges to `number`. That
/// reads like a parity gap in this substrate. It is not: tsgo makes the
/// SAME distinction, and makes it louder.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`, UNANNOTATED — the annotated spellings both trivially
/// answer `number` from the annotation and say nothing about body-derived
/// inference, which is the only thing this rail computes):
///
/// ```text
/// if (n > 0) return f(n - 1); return 0;    number      no error
/// if (n > 0) return 0; return f(n - 1);    number      no error
/// return n > 0 ? f(n - 1) : 0;             any         TS7023
/// return n > 0 ? 0 : f(n - 1);             any         TS7023
/// return n > 0 && f(n - 1);                any         TS7023
/// return { a: n > 0 ? f(n - 1) : 0 };      any         TS7023
/// ```
///
/// TypeScript aggregates return STATEMENTS and tolerates a circular one
/// among several; a circular reference INSIDE one return expression
/// poisons that expression, and the whole inferred return becomes the
/// circularity error — reported as TS7023 with `any` as the fallback.
///
/// So a fail-closed `Error(Miss)` here is the honest answer, not a
/// missing feature: it refuses exactly where the checker refuses. Making
/// the union arm drop held arms and publish the concrete ones would
/// publish `number` where the checker publishes an ERROR — a divergence,
/// in the direction this substrate must never move.
///
/// This corrects the record: the round that gave the conditional its
/// structural arm claimed the two spellings answer alike. They do not,
/// and they should not.
///
/// Mutation recipe: making `SliceExpr::Union`'s arm skip a held arm
/// (`Ok(None) => continue`) instead of failing closed flips `ctRec` to
/// `Primitive(Number)` cands=1 — the divergence — while leaving
/// `ctRecIf` green.
#[test]
fn flow_return_ternary_self_recursion_refuses_where_the_checker_refuses() {
    let host = make_r5_host();

    // The `if` spelling converges through the SCC fixed point.
    assert_clean_warm(&host, "ctRecIf", TypeExpr::Primitive(PrimitiveName::Number));

    // The ternary spelling refuses — as tsgo does, with TS7023.
    assert_fails_closed(&host, "ctRec");
    // And so does a hold nested inside an object member, which is the
    // same shape one level down.
    assert_fails_closed(&host, "ctRecObjMember");
}

/// Where the leaf `any` still stands, and where the CALL-POSITION
/// verdict overrides it.
///
/// `lower_leaf`'s gate refuses a leaf answer the shared shallow pass
/// FABRICATED at a call position, in either shape the pass produces:
///
///   - the answer EMBEDS the pass's unreduced `ReturnType<callee>`
///     carrier — a call in an array element or a spread;
///   - the answer EMBEDS `any` AND the expression's value COMPOSES over a
///     call the substrate has no structural arm for.
///
/// The second reading is what closed the residual class. It was
/// previously drawn at "the form's OWN value IS a call's return"
/// (`new f()`, `` tag`…` ``, `f?.()`, `await f()`, `(k, f())`, `f()!`),
/// which left every JOIN (`k || f()`, `k ?? f()`), PROJECTION
/// (`f().q`, `f()["q"]`, `f?.()?.q`), OPERATOR (`f() + "x"`) and
/// ASSIGNMENT (`z = f()`) form publishing the pass's fabricated `any`,
/// clean and warm.
///
/// The old record justified those rows as "UNDER the checker's answer,
/// never over it". That reasoning does not hold: `any` is
/// BIDIRECTIONALLY assignable, so it is neither under nor over — it
/// silences every check in both directions, and it is indistinguishable
/// from an authored `any` at every downstream gate. Being under the
/// checker was never the property that made a value safe to publish;
/// being DISTINGUISHABLE from a known one is.
///
/// So every row below now fails closed, POSITIONALLY: the value is the
/// typed unresolved marker and the result admits nothing. The
/// composite-position twins — the same forms as ONE member of an object
/// literal, with a modelled sibling that must SURVIVE — are in
/// `flow_return_positional_tests`.
///
/// The `any` that still stands is the AUTHORED one: `x as any` answers
/// `any` because the program says so, and no call composes into it.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`):
///
/// ```text
///                    checker           published here
/// tnAmbLogical       "TA" | true       fails closed ← composes a call
/// tnAmbNullish       "TA"              fails closed ← composes a call
/// tnGenericLogical   string | true     fails closed ← composes a call
/// tnGenericMember    string            fails closed ← composes a call
/// tnAmbSequence      "TA"              fails closed ← call position
/// tnAmbNonNull       "TA"              fails closed ← call position
/// tnAmbAs            "TA"              "TA"         ← exact
/// tnStrTernary       "a" | "b"         "a" | "b"    ← exact
/// tnGenericBare      string            string       ← exact: the explicit
///                                                     type argument
///                                                     instantiates the clause
/// ```
///
/// Mutation recipe: dropping the `embeds_any && composes` half of
/// `leaf_answer_is_fabricated_at_a_call_position` flips the four
/// composing rows back to a warm `Primitive(Any)` with `degradation:
/// None`; dropping `SequenceExpression` / the type-carrier recursion
/// from `value_is_unmodeled_call` flips the two call-position rows the
/// same way; making the gate unconditional on the form (dropping the
/// `embeds_any` conjunct) flips `tnAmbAs` / `tnStrTernary` to fail
/// closed, which is the over-refusal direction.
#[test]
fn flow_return_leaf_answered_call_forms_publish_any_not_a_carrier() {
    let host = make_r5_host();

    // The COMPOSING forms: a join / projection over a call the substrate
    // has no arm for. The pass fabricated an `any`; it is not published.
    for name in [
        "tnAmbLogical",
        "tnAmbNullish",
        "tnGenericLogical",
        "tnGenericMember",
    ] {
        assert_fails_closed(&host, name);
    }

    // The CALL POSITIONS: the form's own value is the call's return, and
    // the substrate has no arm for it. `any` is not published.
    assert_fails_closed(&host, "tnAmbSequence");
    assert_fails_closed(&host, "tnAmbNonNull");

    // The two rows that ARE exact, and the explicit-type-argument row the
    // executor now resolves exactly: they discriminate the `any` rows
    // above from "everything answers `any`".
    assert_clean_warm(&host, "tnAmbAs", string_lit("TA"));
    assert_clean_warm(
        &host,
        "tnStrTernary",
        TypeExpr::Union(Arc::from(
            vec![string_lit("a"), string_lit("b")].into_boxed_slice(),
        )),
    );
    assert_clean_warm(
        &host,
        "tnGenericBare",
        TypeExpr::Primitive(PrimitiveName::String),
    );
}

/// The overload-visibility rule agrees across every group SHAPE, because
/// there is only one of it.
///
/// The carrier and `build_typeof` are not compared shape-by-shape here —
/// they cannot disagree, because neither computes the rule: both call
/// `semantic_query::visible_overload_ordinals` and there is no second
/// copy in the tree. What this test pins is the rule ITSELF over the
/// shapes a group can take, so a change to it is a change both producers
/// are known to inherit.
#[test]
fn visible_overload_ordinals_covers_every_group_shape() {
    use crate::semantic_query::visible_overload_ordinals;

    // Degenerate.
    assert_eq!(visible_overload_ordinals([]), Vec::<usize>::new());
    // A lone signature is visible whether or not it is bodied.
    assert_eq!(visible_overload_ordinals([false]), vec![0]);
    assert_eq!(visible_overload_ordinals([true]), vec![0]);
    // The ordinary overload group: bodiless overloads, implementation
    // hidden.
    assert_eq!(visible_overload_ordinals([false, false, true]), vec![0, 1]);
    assert_eq!(visible_overload_ordinals([false, true]), vec![0]);
    // An AMBIENT group has nothing to hide.
    assert_eq!(visible_overload_ordinals([false, false]), vec![0, 1]);
    // Ill-formed: no bodiless member at all. Surfacing everything beats
    // surfacing nothing, and it is what makes the lone-bodied signature
    // visible without a special case.
    assert_eq!(visible_overload_ordinals([true, true]), vec![0, 1]);
    // A bodied contributor that is NOT last is still hidden: the rule is
    // "bodiless", not "all but the last".
    assert_eq!(visible_overload_ordinals([true, false, false]), vec![1, 2]);
}

// ──────────────────────────────────────────────────────────────────────
// PARKED pre-existing defects
//
// Each row below is a measured, PRE-EXISTING divergence whose fix belongs
// to a layer this block does not own. Each carries a REAL body asserting
// the CHECKER's answer — the answer the code should give — plus the
// verbatim failure it produces un-ignored today, so the owning block
// inherits an executable repro rather than a paragraph.
//
// Registration note: `crates/verter_session/src/**` lib tests have no
// general ignored-test registry. The two that exist are scoped elsewhere
// and neither admits these rows — `typeinfo_ignored_test_manifest`
// discovers only `crates/verter_session/src/typeinfo/typeinfo_tests`
// (generated by `scripts/gen-typeinfo-ignore-manifest.mjs` from the
// append-only typeinfo parity row registry), and
// `framework_known_bug_manifest` keys its reverse scan on the
// `vue-` / `svelte-` / `react-` / `solid-` id prefixes and asserts its
// ledger EMPTY. Filing U6 flow-return debt in either would misfile it and
// would silently retire the framework ledger's own emptiness assertions.
// ──────────────────────────────────────────────────────────────────────

/// A heritage-REDECLARED method degrades where the checker answers the
/// declared literal.
///
/// A derived `class` / `interface` that re-declares a base method leaves
/// the composed surface carrying BOTH contributors under one key, so the
/// shared PathWalker's Object hop sees a two-member same-name method
/// collision and hands the call rail an overload GROUP of arity 2 — which
/// the rail refuses (`UnrepresentableCallee`). It is not an overload
/// group: TypeScript's derived declaration OVERRIDES the base one.
///
/// OWNER: the shared PathWalker / type-resolution heritage member
/// projection — the composed surface must not retain a base contributor
/// a derived declaration overrides. Not the flow rail: the rail's refusal
/// is correct for a genuine arity-2 group, and the defect is that this is
/// not one. Pre-existing (the reviewer proved it by mutation control
/// against the pre-carrier tree).
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`): `hbClassCall()` is `"BASE"`, `ebIfaceCall()` is
/// `"EB"`.
///
/// Verbatim failure, un-ignored on this tree:
///
/// ```text
/// assertion `left == right` failed: hbClassCall must evaluate clean
///   left: Some(UnrepresentableCallee)
///  right: None
/// ```
#[ignore = "owned by the shared PathWalker / type-resolution heritage member projection: a \
            derived re-declaration must OVERRIDE the base contributor on the composed surface \
            rather than leave both under one key, which the method-overload-group carrier then \
            reads as an arity-2 group"]
#[test]
fn heritage_redeclared_method_answers_the_derived_declaration() {
    let host = make_r5_host();
    assert_clean_warm(&host, "hbClassCall", string_lit("BASE"));
    assert_clean_warm(&host, "ebIfaceCall", string_lit("EB"));
}

/// Reading an accessor pair publishes the GETTER's `Signature` node
/// instead of the getter's RETURN.
///
/// `class C { get a(): "GA"; set a(v: "GA") }` — reading `.a` publishes
/// the getter's callable signature, cleanly and warm, where the property
/// read's value is the getter's return type.
///
/// OWNER: the shared PathWalker / type-resolution accessor member
/// projection. Structurally untouched by the flow-return substrate — the
/// flow rail only reads whatever the member hop published.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`): `gaRead()` is `"GA"`.
///
/// Verbatim failure, un-ignored on this tree:
///
/// ```text
/// assertion `left == right` failed: gaRead's read of an accessor pair must publish the getter's RETURN, not its signature
///   left: Other("Signature { kind: Call, params: [], return_type: SemanticNodeId(3), type_parameters: [], signature_span: Some(Span { start: 41651, end: 41660 }), return_type_span: Some(Span { start: 41655, end: 41659 }) }")
///  right: Other("Literal(String(\"GA\"))")
/// ```
///
/// (The `SemanticNodeId` and the two spans are fixture-POSITION
/// dependent — an edit anywhere above `GaClass` in the shared R5 fixture
/// moves them. The load-bearing part is the node KIND: a `Signature`
/// where the read's value must be that signature's return.)
#[ignore = "owned by the shared PathWalker / type-resolution accessor member projection: a \
            property read of a get/set pair must project the GETTER's return type, not the \
            getter's Signature node"]
#[test]
fn accessor_pair_read_publishes_the_getters_return() {
    let host = make_r5_host();
    r5_node(
        &host,
        "gaRead",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(
                node_shape(dispatch, node),
                NodeShape::Other("Literal(String(\"GA\"))".to_string()),
                "gaRead's read of an accessor pair must publish the getter's RETURN, \
                 not its signature"
            );
        },
    );
}

/// The `undefined` IDENTIFIER publishes a semantic-miss carrier instead
/// of the `undefined` primitive.
///
/// `k ? undefined : 1` publishes
/// `Union([1, Unknown { raw: "semanticMiss" }])`: the `undefined`
/// identifier resolves to nothing the value pass models, so the leaf
/// lowering answers a miss carrier rather than
/// `PrimitiveKind::Undefined`. The result is a DEGRADED success — the
/// value reaches an unresolved carrier, so nothing warms — which is why
/// the row now fails at the "must evaluate clean" gate before it can
/// reach the arm comparison it was written to make.
///
/// OWNER: `U6.VALUE_INFERENCE` — the `undefined`-identifier gap in the
/// shared shallow value pass. Pre-existing; it is newly REACHABLE through
/// the conditional's structural arm (before it, the whole ternary folded
/// through one leaf answer), not newly wrong.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`): `undefTernary(k)` is `1 | undefined`.
///
/// Verbatim failure, un-ignored on this tree:
///
/// ```text
/// assertion `left == right` failed: undefTernary must evaluate clean
///   left: Some(UnresolvedValue)
///  right: None
/// ```
///
/// (The row dies at `r5_node`'s clean-and-warm gate, BEFORE the arm
/// comparison: a value that reaches a miss carrier is a degraded success.
/// The arm comparison it would then make is
/// `["Opaque", "Other(\"Literal(Number(1.0))\")"]` against
/// `["Other(\"Literal(Number(1.0))\")", "Primitive(Undefined)"]`, sorted,
/// because the union interner orders by node id. `Opaque` is
/// `node_shape`'s spelling of `SemanticNodeData::Opaque(QueryError::Miss)`
/// — the same node the PROJECTED surface renders as
/// `Unknown { raw: "semanticMiss" }`.)
#[ignore = "owned by U6.VALUE_INFERENCE: the `undefined` identifier must lower to \
            PrimitiveKind::Undefined in the shared shallow value pass instead of a \
            semantic-miss carrier"]
#[test]
fn undefined_identifier_publishes_the_undefined_primitive() {
    let host = make_r5_host();
    r5_node(
        &host,
        "undefTernary",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            // The union interner sorts arms by interned node id, so the
            // comparison is order-INDEPENDENT: an authored-order
            // assertion would fail for a reason the owning block does
            // not own.
            let mut arms: Vec<String> = union_members(dispatch, node)
                .iter()
                .map(|arm| format!("{:?}", node_shape(dispatch, *arm)))
                .collect();
            arms.sort();
            let mut expected = vec![
                format!("{:?}", NodeShape::Other("Literal(Number(1.0))".to_string())),
                format!("{:?}", NodeShape::Primitive(PrimitiveKind::Undefined)),
            ];
            expected.sort();
            assert_eq!(
                arms, expected,
                "the `undefined` identifier must publish the undefined primitive"
            );
        },
    );
}

/// A call in a LOGICAL operand FAILS CLOSED where it must publish the
/// operand union.
///
/// `k || tnAmb("a")` used to answer `Primitive(Any)` cleanly and warm —
/// the shallow pass's per-expression fallback (`_ => Ok(Primitive(Any))`)
/// surfaced as a value. It no longer publishes: the leaf gate refuses an
/// answer that embeds `any` when the expression's value COMPOSES over a
/// call with no structural arm, so the position carries the typed
/// unresolved marker and the result is a degraded success admitting
/// nothing.
///
/// That closes the fabricated-value half. The CAPABILITY half is still
/// open, and is what this row pins.
///
/// OWNER: the SHALLOW PASS (`verter_semantic::analysis::type_eval_build`
/// per-expression lowering) under `U6.VALUE_INFERENCE` — not the flow
/// rail, which owns only what happens to an answer the shallow pass
/// produced. The green counterpart
/// (`flow_return_leaf_answered_call_forms_publish_any_not_a_carrier`)
/// pins the fail-closed disposition; this row pins the answer it must
/// eventually give.
///
/// Oracle (tsgo `7.0.0-dev.20260526.1`, `--noEmit --strict
/// --ignoreConfig`): `tnAmbLogical(k)` is `"TA" | true`. (The `"TA"` half
/// additionally needs argument-driven overload resolution —
/// `U6.CALL_RESOLVE` — so this row does not close until both land.)
///
/// Verbatim failure, un-ignored on this tree:
///
/// ```text
/// assertion `left == right` failed: tnAmbLogical must evaluate clean
///   left: Some(UnmodeledPosition)
///  right: None
/// ```
#[ignore = "owned by U6.VALUE_INFERENCE (the shallow pass's `_ => Ok(Primitive(Any))` \
            per-expression fallback) plus U6.CALL_RESOLVE for the overload half: a call in a \
            logical operand must publish the operand union, not the typed unresolved marker"]
#[test]
fn call_in_a_logical_operand_publishes_the_operand_union() {
    let host = make_r5_host();
    assert_clean_warm(
        &host,
        "tnAmbLogical",
        TypeExpr::Union(Arc::from(
            vec![
                string_lit("TA"),
                TypeExpr::Literal(verter_type_expr::LiteralValue::Boolean(true)),
            ]
            .into_boxed_slice(),
        )),
    );
}
