// Negative parent probe for the STP18 component-use products. Everything
// from the `__VerterUseConstructor` declaration on is the component-use
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

declare function __VerterUseConstructor<P, I>(component: abstract new (props: P) => I): new (props: P) => I;
type __VerterUseComponentProps<C> = C extends abstract new (props: infer P) => unknown ? P : never;
type __VerterUseKnownSpread<S, P, O extends PropertyKey> = string extends keyof S ? S : Exclude<keyof S, keyof P | O> extends never ? Omit<S, O> extends Partial<Omit<P, O>> ? S : never : never;
declare function __VerterUseSpread<C, S, O extends PropertyKey>(component: C, spread: S & __VerterUseKnownSpread<S, __VerterUseComponentProps<C>, O>, overwritten: readonly O[]): S;
type __VerterUseProp<I, K extends PropertyKey> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : unknown) : unknown;
type __VerterUseListener<I, K extends PropertyKey, F extends PropertyKey = K> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : F extends keyof P ? P[F] : (...args: any[]) => unknown) : (...args: any[]) => unknown;
type __VerterUseSlotProps<I, K extends PropertyKey> = I extends { readonly $slots: infer S } ? (K extends keyof S ? (NonNullable<S[K]> extends (props: infer A, ...rest: any[]) => any ? A : unknown) : unknown) : unknown;
type __VerterUseModel<I, K extends PropertyKey> = NonNullable<__VerterUseProp<I, K>> extends (value: infer V, ...rest: any[]) => any ? V : unknown;
const __VerterUse_9d2b62cd64d42047 = new (__VerterUseConstructor(Table))({ "rows": (rows), "project": ((row) => row.name), "kind": "list", "onChange": (log) });
const __VerterUse_9d2b62cd64d42047_check0: __VerterUseListener<typeof __VerterUse_9d2b62cd64d42047, "onChange"> = (count);
const __VerterUse_5caed740157a2dee = new (__VerterUseConstructor(Table))({ "rows": (rows), "project": ((row) => row.name), "kind": "list" });
const __VerterUse_5caed740157a2dee_check0: __VerterUseListener<typeof __VerterUse_5caed740157a2dee, "onChangeOnce", "onChange"> = (count);
const __VerterUse_116e8eb456753747 = new (__VerterUseConstructor(Table))({ "rows": (rows), "project": ((row) => row.name), "kind": "list", "onChange": (log) });
const __VerterUse_116e8eb456753747_check0: __VerterUseListener<typeof __VerterUse_116e8eb456753747, "onChange"> = (handlers?.onChange);
