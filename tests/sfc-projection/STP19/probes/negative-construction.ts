// Construction-negative parent probe for the STP19 advanced generic use
// products (pinned by the Rust test `probe_fixtures_are_the_rendered_products`
// like the other parents). Each construction below violates its component's
// exact contract and its attribute-tolerant fallback alike, so each is one
// TS2769 anchored at the offending authored member:
//
// - STP19-forward: `field` must be a key of the parent's own `T`.
// - STP19-instantiation-alias: the alias fixed `K = "id"`.
// - STP19-foreign: the Options API component's published `count` is a number.
// - STP19-overloads: no overload of six accepts a string `sides`.
// - STP19-overloads: a generic overload ahead of an open catch-all keeps
//   the catch-all closed to its published string `label`.
//
// <template>
//   <Picker :items="props.items" field="missing" :format="String" />
//   <BarrelPicker :items="rows" field="label" :format="describe" />
//   <Counter :count="'1'" />
//   <Shape kind="polygon" :sides="'five'" />
//   <Select :label="1" />
// </template>
import Picker from "./components/Picker.vue";
import { Counter, Select, Shape } from "./foreign";
import { BarrelPicker } from "./barrel";
import type { Row } from "./aliases";

declare function defineProps<P>(): Readonly<P>;

type __VerterUseOpenArgs<A> = A extends readonly any[] ? (number extends A["length"] ? (0 extends 1 & A[number] ? true : false) : false) : false;
type __VerterUseSame<X, Y> = (<T>() => T extends X ? 1 : 2) extends (<T>() => T extends Y ? 1 : 2) ? true : false;
type __VerterUsePeeled<N> = { readonly __verterUsePeeled: N };
type __VerterUseOrdered<T, Acc> = T extends readonly [infer H, ...infer R] ? __VerterUseOrdered<R, Acc & H> : Acc;
type __VerterUseConstruct<A extends readonly unknown[], I> = __VerterUseOpenArgs<A> extends true ? (I extends { readonly $props: infer P } ? new (props: P) => I : new (...args: A) => I) : new (...args: A) => I;
type __VerterUseConstructs<C, Seen, Prev, Out extends readonly unknown[]> = (Seen & C) extends abstract new (...args: infer A) => infer I ? (A extends readonly [__VerterUsePeeled<number>] ? __VerterUseOrdered<Out, unknown> : __VerterUseSame<[A, I], Prev> extends true ? __VerterUseOrdered<Out, unknown> : __VerterUseConstructs<C, Seen & { new (...args: A): I; new (...args: [__VerterUsePeeled<Out["length"]>]): never }, [A, I], [__VerterUseConstruct<A, I>, ...Out]>) : C;
type __VerterUseCall<A> = A extends readonly [infer P, ...infer X] ? new (props: P) => __VerterUseFunctional<P, X extends readonly [infer Y, ...unknown[]] ? Y : unknown> : new (props: Record<string, never>) => __VerterUseFunctional<unknown, unknown>;
type __VerterUseCalls<C, Seen, Prev, Out extends readonly unknown[]> = (Seen & C) extends (...args: infer A) => infer R ? (A extends readonly [__VerterUsePeeled<number>] ? (Out extends readonly [unknown, unknown, ...unknown[]] ? __VerterUseOrdered<Out, unknown> : unknown) : __VerterUseSame<[A, R], Prev> extends true ? (Out extends readonly [unknown, unknown, ...unknown[]] ? __VerterUseOrdered<Out, unknown> : unknown) : __VerterUseCalls<C, Seen & { (...args: A): R; (...args: [__VerterUsePeeled<Out["length"]>]): never }, [A, R], [__VerterUseCall<A>, ...Out]>) : unknown;
type __VerterUseContract<C> = C extends abstract new (...args: infer A) => unknown ? (__VerterUseOpenArgs<A> extends true ? __VerterUseConstructs<C, unknown, never, []> : C) : __VerterUseCalls<C, unknown, never, []>;
type __VerterUseTolerant<P, I> = (0 extends 1 & P ? (I extends { readonly $props: infer Q } ? Q : P) : P) & Record<string, unknown>;
type __VerterUseFunctional<P, X> = { readonly $props: P; readonly $slots: X extends { slots: infer S } ? S : {}; $emit: X extends { emit: infer E } ? E : never };
declare function __VerterUseComponent<C, A>(component: C, tolerant: A): __VerterUseContract<C> & A;
declare function __VerterUseConstructor<P, I>(component: abstract new (props: P) => I): new (props: __VerterUseTolerant<P, I>) => I;
declare function __VerterUseConstructor<P, X, R>(component: (props: P, ctx: X) => R): new (props: P & Record<string, unknown>) => __VerterUseFunctional<P, X>;
type __VerterUseProp<I, K extends PropertyKey> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : unknown) : unknown;
type __VerterUseListener<I, K extends PropertyKey, F extends PropertyKey = K> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : F extends keyof P ? P[F] : (...args: any[]) => unknown) : (...args: any[]) => unknown;
type __VerterUseSlotProps<I, K extends PropertyKey> = I extends { readonly $slots: infer S } ? (K extends keyof S ? (NonNullable<S[K]> extends (props: infer A, ...rest: any[]) => any ? A : unknown) : unknown) : unknown;
type __VerterUseModel<I, K extends PropertyKey> = NonNullable<__VerterUseProp<I, K>> extends (value: infer V, ...rest: any[]) => any ? V : unknown;
function __VerterGenericUseScope<T extends { id: number; label: string }>() {
const props = defineProps<{ items: T[] }>();
const rows: Row[] = [];
const describe = (value: unknown) => String(value);
const __VerterUse_71e18c9859e6b45c = new (__VerterUseComponent(Picker, __VerterUseConstructor(Picker)))({ "items": (props.items), "field": "missing", "format": (String) });
const __VerterUse_4780fd1fea8c1598 = new (__VerterUseComponent(BarrelPicker, __VerterUseConstructor(BarrelPicker)))({ "items": (rows), "field": "label", "format": (describe) });
const __VerterUse_4655af4bb204c4c0 = new (__VerterUseComponent(Counter, __VerterUseConstructor(Counter)))({ "count": ('1') });
const __VerterUse_e981986d24437340 = new (__VerterUseComponent(Shape, __VerterUseConstructor(Shape)))({ "kind": "polygon", "sides": ('five') });
const __VerterUse_09fb2e698bdf541a = new (__VerterUseComponent(Select, __VerterUseConstructor(Select)))({ "label": (1) });
}
