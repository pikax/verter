// Declaration pin for the STP19 tsc probes, rendered from `Picker.vue` by
// the public-constructor product (the script pair is inlined in the Rust test
// `picker_probe_fixture_is_the_rendered_declaration`). Everything after the
// retained import is that product's rendered declaration byte for byte; the
// test fails if the two drift. The binder is `const`, dependent (`K extends
// keyof T`), defaulted and variadic (`Extra extends unknown[] = []`).
//
// <script setup lang="ts" generic="const T extends { id: number }, K extends keyof T = keyof T, Extra extends unknown[] = []">
// import type { VNode } from "vue";
// defineProps<{ items: readonly T[]; field: K; format: <V>(value: V) => string; extra?: Extra }>();
// defineEmits<{ pick: [item: T, key: K] }>();
// defineSlots<{ default(props: { item: T; value: T[K]; map: <R>(project: (item: T) => R) => R[] }): VNode[] }>();
// </script>
import type { VNode } from "vue";

type __VerterTypeEmitsToOptions<T> = { [K in keyof T & string]: T[K] extends [...args: infer A] ? (...args: A) => any : () => any } & (T extends (...args: any[]) => any ? __VerterTypeEmitsFns<__VerterTypeEmitsParameters<__VerterTypeEmitsOverloads<T>>> : {});
type __VerterTypeEmitsFns<T extends any[]> = { [K in T[0]]: K extends string ? (string extends K ? never : (...args: T extends [e: infer E, ...args: infer P] ? K extends E ? P : never : never) => any) : never };
type __VerterTypeEmitsParameters<F> = F extends (...args: infer P) => any ? P : never;
type __VerterTypeEmitsOverloadStep<T, Seen = unknown> = T extends (...args: infer A) => infer R ? Seen extends T ? never : __VerterTypeEmitsOverloadStep<Seen & T, Seen & ((...args: A) => R) & { [K in keyof T]: T[K] }> | ((...args: A) => R) : never;
type __VerterTypeEmitsOverloads<T> = __VerterTypeEmitsWithout<__VerterTypeEmitsOverloadStep<(() => never) & T>, T extends () => never ? never : () => never>;
type __VerterTypeEmitsWithout<U, E> = U extends E ? never : U;
type __VerterPublicProps<T extends { id: number }, K extends keyof T = keyof T, Extra extends unknown[] = []> = import("vue").PublicProps & ({ items: readonly T[]; field: K; format: <V>(value: V) => string; extra?: Extra }) & import("vue").EmitsToProps<__VerterTypeEmitsToOptions<({ pick: [item: T, key: K] })>>;
type __VerterPublicInstance<T extends { id: number }, K extends keyof T = keyof T, Extra extends unknown[] = []> = Omit<import("vue").ComponentPublicInstance, "$props" | "$emit" | "$slots"> & {
  readonly $props: __VerterPublicProps<T, K, Extra>;
  $emit: import("vue").EmitFn<__VerterTypeEmitsToOptions<({ pick: [item: T, key: K] })>>;
  readonly $slots: Readonly<{ default(props: { item: T; value: T[K]; map: <R>(project: (item: T) => R) => R[] }): VNode[] }>;
};
declare const __VerterPublicComponent: {
  new <const T extends { id: number }, K extends keyof T = keyof T, Extra extends unknown[] = []>(props: __VerterPublicProps<T, K, Extra>): __VerterPublicInstance<T, K, Extra>;
};
export default __VerterPublicComponent;
