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

type __VerterPublicProps<T extends { id: number }, K extends keyof T = keyof T, Extra extends unknown[] = []> = import("vue").PublicProps & ({ items: readonly T[]; field: K; format: <V>(value: V) => string; extra?: Extra }) & import("vue").EmitsToProps<import("vue").TypeEmitsToOptions<({ pick: [item: T, key: K] })>>;
type __VerterPublicInstance<T extends { id: number }, K extends keyof T = keyof T, Extra extends unknown[] = []> = Omit<import("vue").ComponentPublicInstance, "$props" | "$emit" | "$slots"> & {
  readonly $props: __VerterPublicProps<T, K, Extra>;
  $emit: import("vue").EmitFn<import("vue").TypeEmitsToOptions<({ pick: [item: T, key: K] })>>;
  readonly $slots: Readonly<{ default(props: { item: T; value: T[K]; map: <R>(project: (item: T) => R) => R[] }): VNode[] }>;
};
declare const __VerterPublicComponent: {
  new <const T extends { id: number }, K extends keyof T = keyof T, Extra extends unknown[] = []>(props: __VerterPublicProps<T, K, Extra>): __VerterPublicInstance<T, K, Extra>;
};
export default __VerterPublicComponent;
