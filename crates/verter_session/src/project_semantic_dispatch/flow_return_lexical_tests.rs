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

/// A QUALIFIED type reference is owned by its HEAD segment.
///
/// `x as QE.M` references exactly one binding — `QE`. The trailing
/// segments are member selections INSIDE whatever `QE` denotes, never
/// separate scope lookups. So when the frame declares `QE`, the whole
/// reference belongs to the frame and the owner-scope answer is wrong.
///
/// tsgo 7.0.2 `--strict --declaration --emitDeclarationOnly`, on exactly
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
/// Oracle (tsgo 7.0.2, checker not emitter — `const c: "outerNs" =
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
            assert!(
                matches!(
                    dispatch.execute(SemanticQueryKey::FlowReturn(Box::new(key.clone()))),
                    QueryResult::Error(QueryError::Miss)
                ),
                "ClassLocalShadow member {member_ordinal} must fail closed"
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
    // be mis-bound, so the gate must not fire and the owner scope
    // genuinely answers nothing.
    r5_node(
        &host,
        "paramTypeofNoTwin",
        FunctionPartIdentity::DeclarationBody,
        |dispatch, node| {
            assert_eq!(node_shape(dispatch, node), NodeShape::Opaque);
        },
    );

    // CONTROL — a BODY LOCAL stays invisible in the root parameter list,
    // so the module const is still the answer.
    assert_clean_warm(&host, "paramBodyLocalInvisible", string_lit("moduleLoc"));
}
