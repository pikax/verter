// Parent probe for the STP19 advanced generic use products. Everything from
// the `__VerterUseOpenArgs` declaration to the closing brace of
// `__VerterGenericUseScope` is the advanced generic use product rendered
// from the parent template below over the setup statements (the Rust test
// `probe_fixtures_are_the_rendered_products` pins it byte for byte). The
// parent is `generic="T extends { id: number; label: string }"`; `observe`
// reads each use's witness through the product's observation types.
//
// <template>
//   <Picker :items="props.items" field="label" :format="(value) => String(value)" @pick="(item, key) => onPick(item, key)">
//     <template #default="{ item, map }">{{ item.label }}{{ map((entry) => entry.id) }}</template>
//   </Picker>
//   <BarrelPicker :items="rows" field="id" :format="describe" @pick="(row) => log(row.label)" />
//   <pickers.RowPicker :items="rows" field="id" :format="<V,>(value: V) => JSON.stringify(value)" />
//   <Counter :count="total" @bump="(by) => (total += by)" />
//   <Choice value="b" :options="choices" @change="(value) => choose(value)" />
//   <Badge :level="2" @dismiss="(level) => dismiss(level)">
//     <template #default="{ level }">{{ level }}</template>
//   </Badge>
//   <Cell :value="3" :render="(value) => value.toFixed(1)" />
//   <Shape kind="circle" :radius="1" />
//   <Shape kind="polygon" :sides="5" />
//   <ErasedList :items="rows" />
//   <Untyped :anything="rows" />
// </template>
//
// STP19-forward: `Picker` receives the parent's own `T` (never its
// constraint). STP19-higher-rank: the slot's `map` and the `format` prop keep
// their own binders per call. STP19-explicit / STP19-instantiation-alias:
// `Picker<Row, "id">` reached through a renamed re-export and a namespace
// fixes every channel. STP19-foreign: Options API, generic setup-function,
// functional and generic functional components keep their published
// contracts. STP19-overloads: the fifth of six construct overloads is
// selected. STP19-erasure: an erased generic stays `unknown`, an untyped
// component stays `any`.
import Picker from "./components/Picker.vue";
import { Badge, Cell, Choice, Counter, ErasedList, Shape, Untyped } from "./foreign";
import { BarrelPicker, pickers } from "./barrel";
import type { Row } from "./aliases";

declare function defineProps<P>(): Readonly<P>;
type IsAny<X> = 0 extends 1 & X ? true : false;
type IsExactlyUnknown<X> = IsAny<X> extends true ? false : unknown extends X ? true : false;

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
function onPick(item: T, key: keyof T) {
  return [item, key];
}
function log(text: string) {
  return text;
}
let total = 0;
const choices: ("a" | "b")[] = ["a", "b"];
function choose(value: "a" | "b") {
  return value;
}
function dismiss(level: 1 | 2 | 3) {
  return level;
}
function observe() {
  const pickerSlot = {} as __VerterUseSlotProps<typeof __VerterUse_7d12abeb7579a548, "default">;
  const forwarded: T = pickerSlot.item;
  const forwardedLabel: T["label"] = pickerSlot.value;
  const stp19HoverTarget = pickerSlot.map((entry) => entry.id);
  const labels: string[] = pickerSlot.map((entry) => entry.label);
  const format = {} as __VerterUseProp<typeof __VerterUse_0b8d07d8215f8d9f, "format">;
  const formatted: [string, string] = [format(1), format({ nested: true })];
  const barrelSlot = {} as __VerterUseSlotProps<typeof __VerterUse_de28af435ed10d58, "default">;
  const explicitItem: Row = barrelSlot.item;
  const explicitValue: number = barrelSlot.value;
  const explicitListener: __VerterUseListener<typeof __VerterUse_de28af435ed10d58, "onPick"> = (item, key) => [item.label, key] as const;
  const namespaceInstance: InstanceType<typeof pickers.RowPicker> = __VerterUse_0b8d07d8215f8d9f;
  const bump: __VerterUseListener<typeof __VerterUse_c00ddc48b3efc0e3, "onBump"> = (by) => by.toFixed();
  const counted: number = __VerterUse_c00ddc48b3efc0e3.$props.count;
  const chosen: "a" | "b" = __VerterUse_73fa95a4c37e26b8.$props.value;
  const badgeSlot = {} as __VerterUseSlotProps<typeof __VerterUse_38e3f1df3d80f389, "default">;
  const level: 1 | 2 | 3 = badgeSlot.level;
  const cellValue: number = __VerterUse_d9fde548ef6c26b1.$props.value;
  const circle: "circle" = __VerterUse_69386ace753053c3.kind;
  const polygon: "polygon" = __VerterUse_be4d6146599d76b8.kind;
  const erasedSlot = {} as __VerterUseSlotProps<typeof __VerterUse_fe63724fd35964d9, "default">;
  const erasedIsUnknown: IsExactlyUnknown<typeof erasedSlot.item> = true;
  const untypedIsAny: IsAny<typeof __VerterUse_88698eb00ab7e0aa> = true;
  return [forwarded, forwardedLabel, stp19HoverTarget, labels, formatted, explicitItem, explicitValue, explicitListener, namespaceInstance, bump, counted, chosen, level, cellValue, circle, polygon, erasedIsUnknown, untypedIsAny];
}
const __VerterUse_7d12abeb7579a548 = new (__VerterUseComponent(Picker, __VerterUseConstructor(Picker)))({ "items": (props.items), "field": "label", "format": ((value) => String(value)), "onPick": ((item, key) => onPick(item, key)) });
const __VerterUse_de28af435ed10d58 = new (__VerterUseComponent(BarrelPicker, __VerterUseConstructor(BarrelPicker)))({ "items": (rows), "field": "id", "format": (describe), "onPick": ((row) => log(row.label)) });
const __VerterUse_0b8d07d8215f8d9f = new (__VerterUseComponent(pickers.RowPicker, __VerterUseConstructor(pickers.RowPicker)))({ "items": (rows), "field": "id", "format": (<V,>(value: V) => JSON.stringify(value)) });
const __VerterUse_c00ddc48b3efc0e3 = new (__VerterUseComponent(Counter, __VerterUseConstructor(Counter)))({ "count": (total), "onBump": ((by) => (total += by)) });
const __VerterUse_73fa95a4c37e26b8 = new (__VerterUseComponent(Choice, __VerterUseConstructor(Choice)))({ "value": "b", "options": (choices), "onChange": ((value) => choose(value)) });
const __VerterUse_38e3f1df3d80f389 = new (__VerterUseComponent(Badge, __VerterUseConstructor(Badge)))({ "level": (2), "onDismiss": ((level) => dismiss(level)) });
const __VerterUse_d9fde548ef6c26b1 = new (__VerterUseComponent(Cell, __VerterUseConstructor(Cell)))({ "value": (3), "render": ((value) => value.toFixed(1)) });
const __VerterUse_69386ace753053c3 = new (__VerterUseComponent(Shape, __VerterUseConstructor(Shape)))({ "kind": "circle", "radius": (1) });
const __VerterUse_be4d6146599d76b8 = new (__VerterUseComponent(Shape, __VerterUseConstructor(Shape)))({ "kind": "polygon", "sides": (5) });
const __VerterUse_fe63724fd35964d9 = new (__VerterUseComponent(ErasedList, __VerterUseConstructor(ErasedList)))({ "items": (rows) });
const __VerterUse_88698eb00ab7e0aa = new (__VerterUseComponent(Untyped, __VerterUseConstructor(Untyped)))({ "anything": (rows) });
}

export type Instance = InstanceType<typeof pickers.RowPicker>;
export const stp19DefinitionTarget: typeof Picker = Picker;
