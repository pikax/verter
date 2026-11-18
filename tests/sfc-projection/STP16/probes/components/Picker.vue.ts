// Declaration pin for the public-constructor tsc probes, rendered from
// `Picker.vue` (the carrier inlined in the Rust tests
// `picker_probe_fixture_is_the_rendered_declaration` and
// `public_constructor_reads_admitted_carrier_blocks`). Everything from
// `function __VerterExpose` to the default export is the product's rendered
// declaration byte for byte; those tests fail if the two drift. The expose
// provider is part of that declaration: TypeScript types the exposed
// `reset` and `current` from the authored setup bindings. The retained
// import stands in for the module-scope setup imports the checking module
// hoists, which is not this product.
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
import { ref, type VNode } from "vue";

function __VerterExpose<const T extends string | number = string>() {
const props = defineProps<{ test: T; label?: string }>();
const emit = defineEmits<{ change: [value: T]; close: [] }>();
defineSlots<{ default(props: { item: T }): VNode[] }>();
const open = defineModel<boolean>("open");
const secret = ref(0);
const current = ref<T>();
function reset(): void { secret.value = 0; }
defineExpose({ reset, current });
defineOptions({ name: "Picker", inheritAttrs: false });
return ({ reset, current });
}
type __VerterTypeEmitsToOptions<T> = { [K in keyof T & string]: T[K] extends [...args: infer A] ? (...args: A) => any : () => any } & (T extends (...args: any[]) => any ? __VerterTypeEmitsFns<__VerterTypeEmitsParameters<__VerterTypeEmitsOverloads<T>>> : {});
type __VerterTypeEmitsFns<T extends any[]> = { [K in T[0]]: K extends string ? (string extends K ? never : (...args: T extends [e: infer E, ...args: infer P] ? K extends E ? P : never : never) => any) : never };
type __VerterTypeEmitsParameters<F> = F extends (...args: infer P) => any ? P : never;
type __VerterTypeEmitsOverloadStep<T, Seen = unknown> = T extends (...args: infer A) => infer R ? Seen extends T ? never : __VerterTypeEmitsOverloadStep<Seen & T, Seen & ((...args: A) => R) & { [K in keyof T]: T[K] }> | ((...args: A) => R) : never;
type __VerterTypeEmitsOverloads<T> = __VerterTypeEmitsWithout<__VerterTypeEmitsOverloadStep<(() => never) & T>, T extends () => never ? never : () => never>;
type __VerterTypeEmitsWithout<U, E> = U extends E ? never : U;
type __VerterPublicProps<T extends string | number = string> = import("vue").PublicProps & ({ test: T; label?: string }) & { "open"?: boolean; openModifiers?: Record<string, true | undefined>; "onUpdate:open"?: (value: boolean) => void } & import("vue").EmitsToProps<__VerterTypeEmitsToOptions<({ change: [value: T]; close: [] })>>;
type __VerterPublicInstance<T extends string | number = string> = Omit<import("vue").ComponentPublicInstance, "$props" | "$emit" | "$slots"> & {
  readonly $props: __VerterPublicProps<T>;
  $emit: import("vue").EmitFn<__VerterTypeEmitsToOptions<({ change: [value: T]; close: [] })>> & ((event: "update:open", value: boolean) => void);
  readonly $slots: Readonly<{ default(props: { item: T }): VNode[] }>;
} & import("vue").ShallowUnwrapRef<ReturnType<typeof __VerterExpose<T>>>;
declare const __VerterPublicComponent: {
  new <const T extends string | number = string>(props: __VerterPublicProps<T>): __VerterPublicInstance<T>;
  readonly name: "Picker";
  readonly inheritAttrs: false;
};
export default __VerterPublicComponent;
