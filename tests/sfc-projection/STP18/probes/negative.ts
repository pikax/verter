// Negative parent probe for the STP18 component-use products. Everything
// from the `__VerterUseOpenArgs` declaration on is the component-use
// product rendered from the template below (pinned by the Rust test
// `probe_fixtures_are_the_rendered_products`).
//
// STP18-handler-check: each use has a `change` listener taking a number
// while the use specializes `U = string` — a second collected listener, an
// event-option (`.once`) listener and an optional member reference. Each is
// validated against the specialized contract, so every customer diagnostic
// is TS2322.
//
// <template>
//   <Table :rows="rows" :project="(row) => row.name" kind="list" @change="log" v-on:change="count" />
//   <Table :rows="rows" :project="(row) => row.name" kind="list" @change.once="count" />
//   <Table :rows="rows" :project="(row) => row.name" kind="list" @change="log" v-on:change="handlers?.onChange" />
// </template>
import Table from "./components/Table.vue";

interface Row {
  id: number;
  name: string;
}
declare const rows: readonly Row[];
declare function log(value: string): void;
declare function count(value: number): void;
declare const handlers: { onChange?: (value: number) => void } | undefined;

type __VerterUseOpenArgs<A> = __VerterUseSame<A, any[]>;
type __VerterUseSame<X, Y> = (<T>() => T extends X ? 1 : 2) extends (<T>() => T extends Y ? 1 : 2) ? true : false;
type __VerterUsePeeled<N> = { readonly __verterUsePeeled: N };
type __VerterUseOrdered<T, Acc> = T extends readonly [infer H, ...infer R] ? __VerterUseOrdered<R, Acc & H> : Acc;
type __VerterUseTupleRest<C> = C extends new <T extends any[]>(...args: T) => { readonly args: T } ? true : C extends new <T extends readonly any[]>(...args: T) => { readonly args: T } ? true : false;
type __VerterUseConstruct<A extends readonly unknown[], I> = __VerterUseOpenArgs<A> extends true ? (I extends { readonly $props: infer P } ? new (props: P) => I : new (props: Record<string, never>) => I) : new (...args: A) => I;
type __VerterUseConstructs<C, Seen, Prev, Out extends readonly unknown[]> = (Seen & C) extends abstract new (...args: infer A) => infer I ? (A extends readonly [__VerterUsePeeled<number>] ? __VerterUseOrdered<Out, unknown> : __VerterUseSame<[A, I], Prev> extends true ? __VerterUseOrdered<Out, unknown> : __VerterUseConstructs<C, Seen & { new (...args: A): I; new (...args: [__VerterUsePeeled<Out["length"]>]): never }, [A, I], [__VerterUseConstruct<A, I>, ...Out]>) : C;
type __VerterUseCall<A> = A extends readonly [infer P, ...infer X] ? new (props: P) => __VerterUseFunctional<P, X extends readonly [infer Y, ...unknown[]] ? Y : unknown> : new (props: Record<string, never>) => __VerterUseFunctional<unknown, unknown>;
type __VerterUseCalls<C, Seen, Prev, Out extends readonly unknown[]> = (Seen & C) extends (...args: infer A) => infer R ? (A extends readonly [__VerterUsePeeled<number>] ? (Out extends readonly [unknown, unknown, ...unknown[]] ? __VerterUseOrdered<Out, unknown> : unknown) : __VerterUseSame<[A, R], Prev> extends true ? (Out extends readonly [unknown, unknown, ...unknown[]] ? __VerterUseOrdered<Out, unknown> : unknown) : __VerterUseCalls<C, Seen & { (...args: A): R; (...args: [__VerterUsePeeled<Out["length"]>]): never }, [A, R], [__VerterUseCall<A>, ...Out]>) : unknown;
type __VerterUseContract<C> = __VerterUseTupleRest<C> extends true ? C : C extends abstract new (...args: infer A) => unknown ? (__VerterUseOpenArgs<A> extends true ? __VerterUseConstructs<C, unknown, never, []> : C) : __VerterUseCalls<C, unknown, never, []>;
type __VerterUseTolerant<P, I> = (0 extends 1 & P ? (I extends { readonly $props: infer Q } ? Q : (0 extends 1 & I ? P : {})) : P) & Record<string, unknown>;
type __VerterUseFunctional<P, X> = { readonly $props: P; readonly $slots: X extends { slots: infer S } ? S : {}; $emit: X extends { emit: infer E } ? E : never };
declare function __VerterUseComponent<C, A>(component: C, tolerant: A): __VerterUseContract<C> & A;
declare function __VerterUseConstructor<C extends new <T extends any[]>(...args: T) => { readonly args: T }>(component: C): C;
declare function __VerterUseConstructor<C extends new <T extends readonly any[]>(...args: T) => { readonly args: T }>(component: C): C;
declare function __VerterUseConstructor<P, I>(component: abstract new (props: P) => I): new (props: __VerterUseTolerant<P, I>) => I;
declare function __VerterUseConstructor<P, X, R>(component: (props: P, ctx: X) => R): new (props: P & Record<string, unknown>) => __VerterUseFunctional<P, X>;
type __VerterUseComponentProps<C> = C extends abstract new (props: infer P) => unknown ? P : never;
type __VerterUseResolvedKeys<S> = [keyof S] extends [infer K] ? ([K] extends [PropertyKey] ? 1 : 0) : 0;
type __VerterUseMemberOpen<S> = S extends unknown ? (string extends keyof S ? true : number extends keyof S ? true : symbol extends keyof S ? true : false) : never;
type __VerterUseBranch<S, P> = Exclude<keyof S, keyof P> extends never ? ([S] extends [Partial<P>] ? true : false) : false;
type __VerterUseChecked<S, P, O extends PropertyKey> = S extends unknown ? ([true] extends [P extends unknown ? __VerterUseBranch<Omit<S, O>, Omit<P, O>> : never] ? true : false) : never;
type __VerterUseKnownSpread<S, P, O extends PropertyKey> = [__VerterUseResolvedKeys<S>] extends [never] ? S : __VerterUseResolvedKeys<S> extends 1 ? ([__VerterUseMemberOpen<S>] extends [false] ? ([__VerterUseChecked<S, P, O>] extends [true] ? S : never) : S) : S;
declare function __VerterUseSpread<C, S, O extends PropertyKey>(component: C, spread: S & __VerterUseKnownSpread<S, __VerterUseComponentProps<C>, O>, overwritten: readonly O[]): S;
type __VerterUseProp<I, K extends PropertyKey> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : unknown) : unknown;
type __VerterUseListener<I, K extends PropertyKey, F extends PropertyKey = K> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : F extends keyof P ? P[F] : (...args: any[]) => unknown) : (...args: any[]) => unknown;
type __VerterUseSlotProps<I, K extends PropertyKey> = I extends { readonly $slots: infer S } ? (K extends keyof S ? (NonNullable<S[K]> extends (props: infer A, ...rest: any[]) => any ? A : unknown) : unknown) : unknown;
type __VerterUseModel<I, K extends PropertyKey> = NonNullable<__VerterUseProp<I, K>> extends (value: infer V, ...rest: any[]) => any ? V : unknown;
const __VerterUse_9d2b62cd64d42047 = new (__VerterUseComponent(Table, __VerterUseConstructor(Table)))({ "rows": (rows), "project": ((row) => row.name), "kind": "list", "onChange": (log) });
const __VerterUse_9d2b62cd64d42047_check0: __VerterUseListener<typeof __VerterUse_9d2b62cd64d42047, "onChange"> = (count);
const __VerterUse_5caed740157a2dee = new (__VerterUseComponent(Table, __VerterUseConstructor(Table)))({ "rows": (rows), "project": ((row) => row.name), "kind": "list" });
const __VerterUse_5caed740157a2dee_check0: __VerterUseListener<typeof __VerterUse_5caed740157a2dee, "onChangeOnce", "onChange"> = (count);
const __VerterUse_116e8eb456753747 = new (__VerterUseComponent(Table, __VerterUseConstructor(Table)))({ "rows": (rows), "project": ((row) => row.name), "kind": "list", "onChange": (log) });
const __VerterUse_116e8eb456753747_check0: __VerterUseListener<typeof __VerterUse_116e8eb456753747, "onChange"> = (handlers?.onChange);
