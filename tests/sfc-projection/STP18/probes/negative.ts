// Negative parent probe for the STP18 component-use products. Everything
// from the `__VerterUseConstructor` declaration on is the component-use
// product rendered from the template below (pinned by the Rust test
// `probe_fixtures_are_the_rendered_products`).
//
// STP18-handler-check: the second `change` listener takes a number while
// the use specializes `U = string`; the collected listener is validated
// against the specialized contract, so the customer diagnostic is TS2322.
//
// <template>
//   <Table :rows="rows" :project="(row) => row.name" kind="list" @change="log" v-on:change="count" />
// </template>
import Table from "./components/Table.vue";

interface Row {
  id: number;
  name: string;
}
declare const rows: readonly Row[];
declare function log(value: string): void;
declare function count(value: number): void;

declare function __VerterUseConstructor<P, I>(component: abstract new (props: P) => I): new (props: P & Record<string, unknown>) => I;
type __VerterUseProp<I, K extends PropertyKey> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : unknown) : unknown;
type __VerterUseListener<I, K extends PropertyKey> = unknown extends __VerterUseProp<I, K> ? (...args: any[]) => unknown : __VerterUseProp<I, K>;
type __VerterUseSlotProps<I, K extends PropertyKey> = I extends { readonly $slots: infer S } ? (K extends keyof S ? (NonNullable<S[K]> extends (props: infer A, ...rest: any[]) => any ? A : unknown) : unknown) : unknown;
type __VerterUseModel<I, K extends PropertyKey> = NonNullable<__VerterUseProp<I, K>> extends (value: infer V, ...rest: any[]) => any ? V : unknown;
const __VerterUse_9d2b62cd64d42047 = new (__VerterUseConstructor(Table))({ "rows": (rows), "project": ((row) => row.name), "kind": "list", "onChange": (log) });
const __VerterUse_9d2b62cd64d42047_check0: __VerterUseListener<typeof __VerterUse_9d2b62cd64d42047, "onChange"> = (count);
