// Declaration pin for the public-constructor tsc probes, rendered from
// `Picker.vue` (the carrier inlined in the Rust tests
// `picker_probe_fixture_is_the_rendered_declaration` and
// `public_constructor_reads_admitted_carrier_blocks`). Everything from
// `type __VerterPublicProps` to the default export is the product's rendered
// declaration byte for byte; those tests fail if the two drift. The expose
// provider stands in for the checking body's declaration emit and the
// retained import for the public-dependency capture, neither of which is
// this product: the provider returns the authored
// `defineExpose({ reset, current })` argument over the same binder.
//
// <script setup lang="ts" generic="const T extends string | number = string">
// import { ref, type VNode } from "vue";
// const props = defineProps<{ test: T; label?: string }>();
// const emit = defineEmits<{ change: [value: T]; close: [] }>();
// defineSlots<{ default(props: { item: T }): VNode[] }>();
// const open = defineModel<boolean>("open");
// const secret = ref(0);
// const current = ref<T>();
// function reset(): void { secret.value = 0; }
// defineExpose({ reset, current });
// defineOptions({ name: "Picker", inheritAttrs: false });
// </script>
import type { Ref, VNode } from "vue";

declare function __VerterExpose<const T extends string | number = string>(): {
  reset: () => void;
  current: Ref<T | undefined>;
};

type __VerterPublicProps<T extends string | number = string> = import("vue").PublicProps & ({ test: T; label?: string }) & { "open"?: boolean; "openModifiers"?: Partial<Record<string, true>>; "onUpdate:open"?: (value: boolean) => void } & import("vue").EmitsToProps<import("vue").TypeEmitsToOptions<({ change: [value: T]; close: [] })>>;
type __VerterPublicInstance<T extends string | number = string> = Omit<import("vue").ComponentPublicInstance, "$props" | "$emit" | "$slots"> & {
  readonly $props: __VerterPublicProps<T>;
  $emit: import("vue").EmitFn<import("vue").TypeEmitsToOptions<({ change: [value: T]; close: [] })>> & ((event: "update:open", value: boolean) => void);
  readonly $slots: Readonly<{ default(props: { item: T }): VNode[] }>;
} & import("vue").ShallowUnwrapRef<ReturnType<typeof __VerterExpose<T>>>;
declare const __VerterPublicComponent: {
  new <const T extends string | number = string>(props: __VerterPublicProps<T>): __VerterPublicInstance<T>;
  readonly name: "Picker";
  readonly inheritAttrs: false;
};
export default __VerterPublicComponent;
