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
//   <Gauge unit="celsius" :value="21" />
//   <Gauge unit="percent" :ratio="0.5" />
//   <Toggle mode="on" :level="2" />
//   <Toggle mode="off" reason="idle" />
//   <Menu :items="['a']" :flag="true" />
//   <Select value="a" :options="['a']" />
//   <Mix kind="g" value="v" />
// </template>
//
// STP19-forward: `Picker` receives the parent's own `T` (never its
// constraint). STP19-higher-rank: the slot's `map` and the `format` prop keep
// their own binders per call. STP19-explicit / STP19-instantiation-alias:
// `Picker<Row, "id">` reached through a renamed re-export and a namespace
// fixes every channel. STP19-foreign: Options API, generic setup-function,
// functional and generic functional components keep their published
// contracts. STP19-overloads: the fifth of six construct overloads is
// selected, the precise overloads ahead of an open catch-all constructor and
// the non-last call overloads of a functional component stay selectable, an
// earlier call overload never absorbs a later overload's extra prop, and a
// generic overload stays selectable ahead of a non-generic one or an open
// catch-all.
// STP19-erasure: an erased generic stays `unknown`, an untyped
// component stays `any`.
import Picker from "./components/Picker.vue";
import { Badge, Cell, Choice, Counter, ErasedList, Gauge, Menu, Mix, Select, Shape, Toggle, Untyped } from "./foreign";
import { BarrelPicker, pickers } from "./barrel";
import type { Row } from "./aliases";

declare function defineProps<P>(): Readonly<P>;
type IsAny<X> = 0 extends 1 & X ? true : false;
type IsExactlyUnknown<X> = IsAny<X> extends true ? false : unknown extends X ? true : false;

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
  const celsius: "celsius" = __VerterUse_0f68882e583e0974.unit;
  const percent: "percent" = __VerterUse_d206613c79de26f0.unit;
  const toggledOn: number = __VerterUse_b5063f18761348d2.$props.level;
  const toggledOff: string = __VerterUse_254f2d410a2b7939.$props.reason;
  const flagged: boolean = __VerterUse_8465246605fe46b7.$props.flag;
  const selected: string = __VerterUse_593dab2aba76bd47.value;
  const mixed: "g" = __VerterUse_9b18a6441c8960f4.$props.kind;
  return [forwarded, forwardedLabel, stp19HoverTarget, labels, formatted, explicitItem, explicitValue, explicitListener, namespaceInstance, bump, counted, chosen, level, cellValue, circle, polygon, erasedIsUnknown, untypedIsAny, celsius, percent, toggledOn, toggledOff, flagged, selected, mixed];
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
const __VerterUse_0f68882e583e0974 = new (__VerterUseComponent(Gauge, __VerterUseConstructor(Gauge)))({ "unit": "celsius", "value": (21) });
const __VerterUse_d206613c79de26f0 = new (__VerterUseComponent(Gauge, __VerterUseConstructor(Gauge)))({ "unit": "percent", "ratio": (0.5) });
const __VerterUse_b5063f18761348d2 = new (__VerterUseComponent(Toggle, __VerterUseConstructor(Toggle)))({ "mode": "on", "level": (2) });
const __VerterUse_254f2d410a2b7939 = new (__VerterUseComponent(Toggle, __VerterUseConstructor(Toggle)))({ "mode": "off", "reason": "idle" });
const __VerterUse_8465246605fe46b7 = new (__VerterUseComponent(Menu, __VerterUseConstructor(Menu)))({ "items": (['a']), "flag": (true) });
const __VerterUse_593dab2aba76bd47 = new (__VerterUseComponent(Select, __VerterUseConstructor(Select)))({ "value": "a", "options": (['a']) });
const __VerterUse_9b18a6441c8960f4 = new (__VerterUseComponent(Mix, __VerterUseConstructor(Mix)))({ "kind": "g", "value": "v" });
}

export type Instance = InstanceType<typeof pickers.RowPicker>;
export const stp19DefinitionTarget: typeof Picker = Picker;

// A rest whose element is `any` is not Vue's bare `...args: any[]` catch-all
// when it retains a required prefix. It must keep the constructor whole.
declare const RestNotOpen: {
  new <T extends string>(props: { value: T }): { readonly $props: { value: T }; readonly tag: T };
  new (label: string, ...rest: any[]): { readonly $props: { kind: "rest" }; readonly tag: "rest" };
};
type RestNotOpenArgsAreClosed = __VerterUseOpenArgs<[label: string, ...rest: any[]]>;
const restNotOpenArgsAreClosed: RestNotOpenArgsAreClosed = false;
const directRestNotOpen: "v" = new RestNotOpen({ value: "v" }).tag;
const adaptedRestNotOpen: "v" = new (__VerterUseComponent(RestNotOpen, __VerterUseConstructor(RestNotOpen)))({ value: "v" }).tag;
// @ts-expect-error positional-rest overloads do not accept a props object.
new RestNotOpen({ kind: "rest" });
// @ts-expect-error the adapter must not turn a required-prefix rest into props.
new (__VerterUseComponent(RestNotOpen, __VerterUseConstructor(RestNotOpen)))({ kind: "rest" });

// A generic rest is not Vue's non-generic open catch-all. Its binder and
// every preceding overload stay on the original constructor path.
declare const GenericRest: {
  new <T extends any[]>(...args: T): { readonly $props: { count: number }; readonly args: T };
};
declare const GenericRestOverloads: {
  new (props: { kind: "precise"; n: number }): { readonly $props: { kind: "precise"; n: number }; readonly tag: "precise" };
  new <T extends any[]>(...args: T): { readonly $props: { kind: "generic" }; readonly args: T; readonly tag: "generic" };
};
declare const ReadonlyGenericRest: {
  new <T extends readonly any[]>(...args: T): { readonly $props: { count: number }; readonly args: T };
};
declare const GenericRestWithoutProps: {
  new <T extends any[]>(...args: T): { readonly args: T };
};
const directGenericRest: [string, number] = new GenericRest("a", 1).args;
const adaptedGenericRest: [string, number] = new (__VerterUseComponent(GenericRest, __VerterUseConstructor(GenericRest)))("a", 1).args;
const directGenericRestProps: [{ count: number }] = new GenericRest({ count: 1 }).args;
const adaptedGenericRestProps: [{ count: number }] = new (__VerterUseComponent(GenericRest, __VerterUseConstructor(GenericRest)))({ count: 1 }).args;
const adaptedReadonlyGenericRest: readonly [string] = new (__VerterUseComponent(ReadonlyGenericRest, __VerterUseConstructor(ReadonlyGenericRest)))("a").args;
const directGenericRestWithoutProps: [{}] = new GenericRestWithoutProps({}).args;
const adaptedGenericRestWithoutProps: [{}] = new (__VerterUseComponent(GenericRestWithoutProps, __VerterUseConstructor(GenericRestWithoutProps)))({}).args;
const directGenericRestOverload: "precise" = new GenericRestOverloads({ kind: "precise", n: 1 }).tag;
const adaptedGenericRestOverload: "precise" = new (__VerterUseComponent(GenericRestOverloads, __VerterUseConstructor(GenericRestOverloads)))({ kind: "precise", n: 1 }).tag;
const directGenericRestOverloadTuple: [{ kind: string }] = new GenericRestOverloads({ kind: "generic" }).args;
const adaptedGenericRestOverloadTuple: [{ kind: string }] = new (__VerterUseComponent(GenericRestOverloads, __VerterUseConstructor(GenericRestOverloads)))({ kind: "generic" }).args;
