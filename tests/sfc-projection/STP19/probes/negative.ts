// Negative parent probe for the STP19 advanced generic use products.
// Everything from the `__VerterUseOpenArgs` declaration to the closing brace
// of `__VerterGenericUseScope` is the product rendered from the template
// below over the setup statements (pinned by the Rust test
// `probe_fixtures_are_the_rendered_products`). Every customer diagnostic is
// TS2322:
//
// - STP19-foreign: a functional component's props reject `level` 4 at the
//   construction, and a collected `dismiss` listener taking a string is
//   checked against its published payload.
// - STP19-explicit: a collected `pick` listener taking a number is checked
//   against the alias's fixed `(item: Row, key: "id")` payload.
// - STP19-higher-rank: the slot's `map` binder follows its callback, so it
//   cannot return strings from ids.
// - STP19-erasure: an erased generic item stays `unknown`, never a fabricated
//   `any` or a reconstructed type.
//
// <template>
//   <Badge :level="4" />
//   <Badge :level="1" @dismiss="dismiss" v-on:dismiss="(level: string) => level" />
//   <BarrelPicker :items="rows" field="id" :format="describe" @pick="(row) => row.id" v-on:pick="count" />
//   <Picker :items="props.items" field="id" :format="String" />
//   <ErasedList :items="rows" />
// </template>
import Picker from "./components/Picker.vue";
import { Badge, ErasedList } from "./foreign";
import { BarrelPicker } from "./barrel";
import type { Row } from "./aliases";

declare function defineProps<P>(): Readonly<P>;

type __VerterUseOpenArgs<A> = A extends readonly any[] ? (number extends A["length"] ? (0 extends 1 & A[number] ? true : false) : false) : false;
type __VerterUseContract<C> = C extends abstract new (...args: infer A) => infer I ? (__VerterUseOpenArgs<A> extends true ? (I extends { readonly $props: infer P } ? new (props: P) => I : C) : C) : unknown;
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
function dismiss(level: 1 | 2 | 3) {
  return level;
}
function count(total: number) {
  return total;
}
function observe() {
  const pickerSlot = {} as __VerterUseSlotProps<typeof __VerterUse_8c747e13a84a8163, "default">;
  const labels: string[] = pickerSlot.map((entry) => entry.id);
  const erasedSlot = {} as __VerterUseSlotProps<typeof __VerterUse_fe63724fd35964d9, "default">;
  const erasedItem: number = erasedSlot.item;
  return [labels, erasedItem];
}
const __VerterUse_59bb64ff2cef7792 = new (__VerterUseComponent(Badge, __VerterUseConstructor(Badge)))({ "level": (4) });
const __VerterUse_2133c520847fbc1a = new (__VerterUseComponent(Badge, __VerterUseConstructor(Badge)))({ "level": (1), "onDismiss": (dismiss) });
const __VerterUse_2133c520847fbc1a_check0: __VerterUseListener<typeof __VerterUse_2133c520847fbc1a, "onDismiss"> = ((level: string) => level);
const __VerterUse_b2332edd161df4f1 = new (__VerterUseComponent(BarrelPicker, __VerterUseConstructor(BarrelPicker)))({ "items": (rows), "field": "id", "format": (describe), "onPick": ((row) => row.id) });
const __VerterUse_b2332edd161df4f1_check0: __VerterUseListener<typeof __VerterUse_b2332edd161df4f1, "onPick"> = (count);
const __VerterUse_8c747e13a84a8163 = new (__VerterUseComponent(Picker, __VerterUseConstructor(Picker)))({ "items": (props.items), "field": "id", "format": (String) });
const __VerterUse_fe63724fd35964d9 = new (__VerterUseComponent(ErasedList, __VerterUseConstructor(ErasedList)))({ "items": (rows) });
}
