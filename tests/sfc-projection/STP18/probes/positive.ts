// Parent probe for the STP18 component-use products. Everything from the
// `__VerterUseConstructor` declaration to the last `_check` statement is the
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

declare function __VerterUseConstructor<P, I>(component: abstract new (props: P) => I): new (props: P & Record<string, unknown>) => I;
type __VerterUseProp<I, K extends PropertyKey> = I extends { readonly $props: infer P } ? (K extends keyof P ? P[K] : unknown) : unknown;
type __VerterUseListener<I, K extends PropertyKey> = unknown extends __VerterUseProp<I, K> ? (...args: any[]) => unknown : __VerterUseProp<I, K>;
type __VerterUseSlotProps<I, K extends PropertyKey> = I extends { readonly $slots: infer S } ? (K extends keyof S ? (NonNullable<S[K]> extends (props: infer A, ...rest: any[]) => any ? A : unknown) : unknown) : unknown;
type __VerterUseModel<I, K extends PropertyKey> = NonNullable<__VerterUseProp<I, K>> extends (value: infer V, ...rest: any[]) => any ? V : unknown;
const __VerterUse_34ba6e5f483b1e90 = new (__VerterUseConstructor(Table))({ "rows": (rows), "project": ((row) => row.name), "kind": "list", "modelValue": (selected), "onChange": ((value) => log(value)) });
const __VerterUse_34ba6e5f483b1e90_check0: __VerterUseListener<typeof __VerterUse_34ba6e5f483b1e90, "onChange"> = (log);
const __VerterUse_e0840c5a5320209d = new (__VerterUseConstructor(Table))({ "rows": (ids), "project": ((id) => id * 2), "kind": "grid", "columns": (3) });
const __VerterUse_e0840c5a5320209d_check0: __VerterUseListener<typeof __VerterUse_e0840c5a5320209d, "onChange"> = ($event) => { total += $event; };

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

export const stp18DefinitionTarget: typeof Table = Table;
