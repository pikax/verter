// Parent probe for the STP18 component-use products. Everything from the
// `__VerterUseOpenArgs` declaration to the last `_check` statement is the
// component-use product rendered from the parent template below (the Rust
// test `probe_fixtures_are_the_rendered_products` pins it byte for byte);
// the setup bindings ahead of it are the parent's script, and the
// observations after it read each use's witness through the product's own
// observation types.
//
// <template>
//   <Table :rows="rows" :project="(row) => row.name" kind="list" v-model="selected" @change="(value) => log(value)" v-on:change="log">
//     <template #default="{ row, value }">{{ row.id }}{{ value }}</template>
//   </Table>
//   <Table :rows="ids" :project="(id) => id * 2" kind="grid" :columns="3" @change="total += $event" />
// </template>
import Table from "./components/Table.vue";

interface Row {
  id: number;
  name: string;
}
declare const rows: readonly Row[];
declare let selected: string;
declare function log(value: string): void;
declare const ids: readonly number[];
declare let total: number;

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
type __VerterUseProp<I, K extends PropertyKey> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : unknown) : unknown;
type __VerterUseListener<I, K extends PropertyKey, F extends PropertyKey = K> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : F extends keyof P ? P[F] : (...args: any[]) => unknown) : (...args: any[]) => unknown;
type __VerterUseSlotProps<I, K extends PropertyKey> = I extends { readonly $slots: infer S } ? (K extends keyof S ? (NonNullable<S[K]> extends (props: infer A, ...rest: any[]) => any ? A : unknown) : unknown) : unknown;
type __VerterUseModel<I, K extends PropertyKey> = NonNullable<__VerterUseProp<I, K>> extends (value: infer V, ...rest: any[]) => any ? V : unknown;
const __VerterUse_34ba6e5f483b1e90 = new (__VerterUseComponent(Table, __VerterUseConstructor(Table)))({ "rows": (rows), "project": ((row) => row.name), "kind": "list", "modelValue": (selected), "onChange": ((value) => log(value)) });
const __VerterUse_34ba6e5f483b1e90_check0: __VerterUseListener<typeof __VerterUse_34ba6e5f483b1e90, "onChange"> = (log);
const __VerterUse_e0840c5a5320209d = new (__VerterUseComponent(Table, __VerterUseConstructor(Table)))({ "rows": (ids), "project": ((id) => id * 2), "kind": "grid", "columns": (3) });
const __VerterUse_e0840c5a5320209d_check0: __VerterUseListener<typeof __VerterUse_e0840c5a5320209d, "onChange"> = ($event) => (total += $event);

// STP18-single-witness: every channel of the first use reads one
// specialization (T = Row, U = string); its slot props, model write and
// listener contract are that witness's, never the bare component's.
export type Instance = InstanceType<typeof Table<Row, string>>;
export const witnessInstance: Instance = __VerterUse_34ba6e5f483b1e90;
const slotProps = {} as __VerterUseSlotProps<typeof __VerterUse_34ba6e5f483b1e90, "default">;
export const stp18HoverTarget = slotProps.row.id;
export const slotValue: string = slotProps.value;
export const modelWrite: __VerterUseModel<typeof __VerterUse_34ba6e5f483b1e90, "onUpdate:modelValue"> = "picked";
export const listener: __VerterUseListener<typeof __VerterUse_34ba6e5f483b1e90, "onChange"> = (value) => {
  const text: string = value;
  void text;
};

// STP18-fresh-id: the sibling use keeps its own specialization (T = number,
// U = number) and its own discriminated `kind` branch.
const siblingSlot = {} as __VerterUseSlotProps<typeof __VerterUse_e0840c5a5320209d, "default">;
export const siblingRow: number = siblingSlot.row;
export const siblingModel: __VerterUseModel<typeof __VerterUse_e0840c5a5320209d, "onUpdate:modelValue"> = 2;

// STP18-script-template-parity: an explicit and an inferred script
// construction agree with the template witness in both directions.
export const explicit = new Table<Row, string>({ rows, project: (row) => row.name, kind: "list" });
export const inferred = new Table({ rows, project: (row) => row.name, kind: "list" });
export const explicitParity: typeof explicit = __VerterUse_34ba6e5f483b1e90;
export const inferredParity: typeof __VerterUse_34ba6e5f483b1e90 = inferred;

// STP18-handler-check: the listener contract keeps an author-written `any`
// (a declared `any` accepts a non-function), an event-option key falls back
// to its unsuffixed listener key, and an inline expression handler returns
// its value to a listener that expects one.
export const declaredAnyListener: __VerterUseListener<{ readonly $props: { onRaw: any } }, "onRaw"> = 1;
export const fallbackListener: __VerterUseListener<{ readonly $props: { onChange?: (value: string) => void } }, "onChangeOnce", "onChange"> = (value) => {
  const text: string = value;
  void text;
};
declare const flag: boolean;
export const returningListener: __VerterUseListener<{ readonly $props: { onSave: (value: string) => boolean } }, "onSave"> = ($event) => (flag === true);

export const stp18DefinitionTarget: typeof Table = Table;
