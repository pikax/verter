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
type __VerterUseBranch<S, P> = [S] extends [P] ? true : false;
type __VerterUseChecked<S, P, O extends PropertyKey> = S extends unknown ? ([true] extends [P extends unknown ? __VerterUseBranch<Omit<S, O>, Omit<P, O>> : never] ? true : false) : never;
type __VerterUseDrop<T, K extends PropertyKey> = T extends unknown ? Omit<T, K> : never;
type __VerterUseKnownSpread<S, P, O extends PropertyKey> = [__VerterUseResolvedKeys<S>] extends [never] ? S : __VerterUseResolvedKeys<S> extends 1 ? ([__VerterUseMemberOpen<S>] extends [false] ? ([__VerterUseChecked<S, P, O>] extends [true] ? S : S & __VerterUseDrop<P, O>) : S) : S;
declare function __VerterUseSpread<C, S, O extends PropertyKey>(component: C, spread: S & __VerterUseKnownSpread<S, __VerterUseComponentProps<C>, O>, overwritten: readonly O[]): S;
type __VerterUseKeyOn<P, K extends PropertyKey> = P extends unknown ? (K extends keyof P ? true : false) : never;
type __VerterUseDirectOk<P, K extends PropertyKey> = 0 extends 1 & P ? true : string extends keyof P ? true : number extends keyof P ? true : symbol extends keyof P ? true : ([__VerterUseKeyOn<P, K>] extends [false] ? false : true);
declare function __VerterUseDirect<C, I, K extends PropertyKey>(component: C, instance: I, key: 0 extends 1 & C ? K : (__VerterUseDirectOk<I extends { readonly $props: infer P } ? P : never, K> extends true ? K : never)): void;
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
__VerterUseDirect(Badge, __VerterUse_59bb64ff2cef7792, "level");
const __VerterUse_2133c520847fbc1a = new (__VerterUseComponent(Badge, __VerterUseConstructor(Badge)))({ "level": (1), "onDismiss": (dismiss) });
__VerterUseDirect(Badge, __VerterUse_2133c520847fbc1a, "level");
__VerterUseDirect(Badge, __VerterUse_2133c520847fbc1a, "onDismiss");
const __VerterUse_2133c520847fbc1a_check0: __VerterUseListener<typeof __VerterUse_2133c520847fbc1a, "onDismiss"> = ((level: string) => level);
const __VerterUse_b2332edd161df4f1 = new (__VerterUseComponent(BarrelPicker, __VerterUseConstructor(BarrelPicker)))({ "items": (rows), "field": "id", "format": (describe), "onPick": ((row) => row.id) });
__VerterUseDirect(BarrelPicker, __VerterUse_b2332edd161df4f1, "items");
__VerterUseDirect(BarrelPicker, __VerterUse_b2332edd161df4f1, "field");
__VerterUseDirect(BarrelPicker, __VerterUse_b2332edd161df4f1, "format");
__VerterUseDirect(BarrelPicker, __VerterUse_b2332edd161df4f1, "onPick");
const __VerterUse_b2332edd161df4f1_check0: __VerterUseListener<typeof __VerterUse_b2332edd161df4f1, "onPick"> = (count);
const __VerterUse_8c747e13a84a8163 = new (__VerterUseComponent(Picker, __VerterUseConstructor(Picker)))({ "items": (props.items), "field": "id", "format": (String) });
__VerterUseDirect(Picker, __VerterUse_8c747e13a84a8163, "items");
__VerterUseDirect(Picker, __VerterUse_8c747e13a84a8163, "field");
__VerterUseDirect(Picker, __VerterUse_8c747e13a84a8163, "format");
const __VerterUse_fe63724fd35964d9 = new (__VerterUseComponent(ErasedList, __VerterUseConstructor(ErasedList)))({ "items": (rows) });
__VerterUseDirect(ErasedList, __VerterUse_fe63724fd35964d9, "items");
}
