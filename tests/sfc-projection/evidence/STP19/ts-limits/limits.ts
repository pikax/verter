// Why a callable whose overload set mixes generic and non-generic call
// signatures cannot be rebuilt into construct signatures, and why the use
// cannot be checked as a direct call instead. Each `@ts-expect-error` marks
// a limit; a clean run on both pinned engines means every limit still holds
// (an unused directive fails if a later TypeScript lifts one).
export {};

type Is<X, Y> = (<Q>() => Q extends X ? 1 : 2) extends <Q>() => Q extends Y ? 1 : 2 ? true : false;

declare function Mix(props: { kind: "a"; n: number }): { n: number };
declare function Mix<T>(props: { kind: "g"; value: T }): { value: T };
declare function Mix(props: { kind: "z"; s: string }): { s: string };

// Ground truth: a direct call selects every overload and infers T.
const direct: [number, "v", string] = [
  Mix({ kind: "a", n: 1 }).n,
  Mix({ kind: "g", value: "v" as const }).value,
  Mix({ kind: "z", s: "s" }).s,
];

// L1: conditional inference reads only the last signature of a set.
type Last = typeof Mix extends (...args: infer A) => infer R ? [A, R] : never;
const l1: Is<Last, [[props: { kind: "z"; s: string }], { s: string }]> = true;

// L2: inference from a generic signature erases its binder (T -> unknown).
declare function G<T>(props: { kind: "g"; value: T }): { value: T };
type GenericRead = typeof G extends (...args: infer A) => infer R ? [A, R] : never;
const l2: Is<GenericRead, [[props: { kind: "g"; value: unknown }], { value: unknown }]> = true;

// L3: an intersection drops only an identical copy of a signature, which is
// how a rebuild walks past a signature it has read. The copy L2 yields is
// not identical to the generic signature, so the walk stalls there and never
// reaches `kind: "a"`; only a hand-written generic copy would pass it.
type ErasedCopy = ((props: { kind: "z"; s: string }) => { s: string }) &
  ((props: { kind: "g"; value: unknown }) => { value: unknown }) &
  typeof Mix;
type AfterErased = ErasedCopy extends (...args: infer A) => unknown ? A : never;
const l3a: Is<AfterErased, [props: { kind: "g"; value: unknown }]> = true;
type LiteralCopy = ((props: { kind: "z"; s: string }) => { s: string }) &
  (<T>(props: { kind: "g"; value: T }) => { value: T }) &
  typeof Mix;
type AfterLiteral = LiteralCopy extends (...args: infer A) => unknown ? A : never;
const l3b: Is<AfterLiteral, [props: { kind: "a"; n: number }]> = true;

// L4: value-level higher-order inference keeps a binder for a single call
// signature only; an overload set contributes its last signature.
declare function lift<A extends unknown[], R>(f: (...args: A) => R): new (...args: A) => R;
const LiftedG = lift(G);
const l4a: { value: "v" } = new LiftedG({ kind: "g", value: "v" as const });
const LiftedMix = lift(Mix);
// @ts-expect-error only `kind: "z"` survives
const l4b = new LiftedMix({ kind: "a", n: 1 });

// L5: `new` on a value with call signatures only resolves every overload,
// but its result is `any` and noImplicitAny reports TS7009.
// @ts-expect-error TS7009
const l5 = new Mix({ kind: "a", n: 1 });
const l5IsAny: 0 extends 1 & typeof l5 ? true : false = true;

// L6: a call on a value with construct signatures only is TS2348, so no one
// application syntax checks both component kinds.
declare const Ctor: new (props: { a: 1 }) => { readonly $props: { a: 1 } };
// @ts-expect-error TS2348
Ctor({ a: 1 });

// L7: converting constructors to calls instead moves the loss to them: a
// generic construct overload beside another overload is dropped.
declare function asCall<A extends unknown[], I>(
  c: abstract new (...args: A) => I,
): (...args: A) => I;
declare const Select: {
  new <T extends string>(props: { value: T; options: T[] }): { readonly value: T };
  new (props: { label: string }): { readonly label: string };
};
const l7a = asCall(Select)({ label: "x" });
// @ts-expect-error the generic construct overload is gone
const l7b = asCall(Select)({ value: "a", options: ["a"] });

export const evidence = [direct, l1, l2, l3a, l3b, l4a, l4b, l5IsAny, l7a, l7b];

// L8: a direct call of the component's own value, gated by a conditional
// that returns the callable whole, does select every overload and infer T,
// so it can validate a callable use. But its result is the component's
// return type (a VNode for a Vue functional component), never the chosen
// overload's props, slots or emit, so it cannot be the use's one
// construction; as an extra check it renders the authored props a second
// time, and every argument diagnostic would be reported twice.
declare function callable<C>(
  component: C,
): C extends (...args: any) => any ? C : (props: any) => void;
declare const node: unique symbol;
declare function Fn(props: { kind: "a"; n: number }): typeof node;
declare function Fn<T extends string>(props: { kind: "one"; item: T }): typeof node;
declare function Fn(props: { kind: "z"; s: string }): typeof node;
const l8a = callable(Fn)({ kind: "a", n: 1 });
const l8b: typeof node = callable(Fn)({ kind: "one", item: "x" });
const l8c = callable(Mix)({ kind: "g", value: "v" as const });
const l8cValue: "v" = l8c.value;
const l8d = callable(Ctor)({ anything: true });
// @ts-expect-error the call result carries no props
l8b.$props;

export const directCall = [l8a, l8b, l8cValue, l8d];
