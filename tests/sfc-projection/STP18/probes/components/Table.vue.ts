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

type __VerterPublicProps<T, U> = import("vue").PublicProps & ({ rows: readonly T[]; project: (row: T) => U } & ({ kind: "list" } | { kind: "grid"; columns: number })) & { "modelValue"?: U; "modelModifiers"?: Partial<Record<string, true>>; "onUpdate:modelValue"?: (value: U) => void } & import("vue").EmitsToProps<import("vue").TypeEmitsToOptions<({ change: [value: U] })>>;
type __VerterPublicInstance<T, U> = Omit<import("vue").ComponentPublicInstance, "$props" | "$emit" | "$slots"> & {
  readonly $props: __VerterPublicProps<T, U>;
  $emit: import("vue").EmitFn<import("vue").TypeEmitsToOptions<({ change: [value: U] })>> & ((event: "update:modelValue", value: U) => void);
  readonly $slots: Readonly<{ default(props: { row: T; value: U }): VNode[] }>;
};
declare const __VerterPublicComponent: {
  new <T, U>(props: __VerterPublicProps<T, U>): __VerterPublicInstance<T, U>;
};
export default __VerterPublicComponent;
