// Declaration pin for the STP18 tsc probes, rendered from `Table.vue` by the
// public-constructor product (the script pair is inlined in the Rust test
// `table_probe_fixture_is_the_rendered_declaration`). Everything after the
// retained import is that product's rendered declaration byte for byte; the
// test fails if the two drift. The retained import stands in for the
// module-scope setup imports the checking module hoists.
//
// <script setup lang="ts" generic="T, U">
// import type { VNode } from "vue";
// defineProps<{ rows: readonly T[]; project: (row: T) => U } & ({ kind: "list" } | { kind: "grid"; columns: number })>();
// defineEmits<{ change: [value: U] }>();
// defineModel<U>();
// defineSlots<{ default(props: { row: T; value: U }): VNode[] }>();
// </script>
import type { VNode } from "vue";

type __VerterTypeEmitsToOptions<T> = { [K in keyof T & string]: T[K] extends [...args: infer A] ? (...args: A) => any : () => any } & (T extends (...args: any[]) => any ? __VerterTypeEmitsFns<__VerterTypeEmitsParameters<__VerterTypeEmitsOverloads<T>>> : {});
type __VerterTypeEmitsFns<T extends any[]> = { [K in T[0]]: K extends string ? (string extends K ? never : (...args: T extends [e: infer E, ...args: infer P] ? K extends E ? P : never : never) => any) : never };
type __VerterTypeEmitsParameters<F> = F extends (...args: infer P) => any ? P : never;
type __VerterTypeEmitsOverloadStep<T, Seen = unknown> = T extends (...args: infer A) => infer R ? Seen extends T ? never : __VerterTypeEmitsOverloadStep<Seen & T, Seen & ((...args: A) => R) & { [K in keyof T]: T[K] }> | ((...args: A) => R) : never;
type __VerterTypeEmitsOverloads<T> = __VerterTypeEmitsWithout<__VerterTypeEmitsOverloadStep<(() => never) & T>, T extends () => never ? never : () => never>;
type __VerterTypeEmitsWithout<U, E> = U extends E ? never : U;
type __VerterPublicProps<T, U> = import("vue").PublicProps & ({ rows: readonly T[]; project: (row: T) => U } & ({ kind: "list" } | { kind: "grid"; columns: number })) & { "modelValue"?: U; modelModifiers?: Record<string, true | undefined>; "onUpdate:modelValue"?: (value: U) => void } & import("vue").EmitsToProps<__VerterTypeEmitsToOptions<({ change: [value: U] })>>;
type __VerterPublicInstance<T, U> = Omit<import("vue").ComponentPublicInstance, "$props" | "$emit" | "$slots"> & {
  readonly $props: __VerterPublicProps<T, U>;
  $emit: import("vue").EmitFn<__VerterTypeEmitsToOptions<({ change: [value: U] })>> & ((event: "update:modelValue", value: U) => void);
  readonly $slots: Readonly<{ default(props: { row: T; value: U }): VNode[] }>;
};
declare const __VerterPublicComponent: {
  new <T, U>(props: __VerterPublicProps<T, U>): __VerterPublicInstance<T, U>;
};
export default __VerterPublicComponent;
