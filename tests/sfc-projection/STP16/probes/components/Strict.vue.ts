// Declaration pin for the binder-dependent runtime options probe, rendered
// from `Strict.vue` (the carrier inlined in the Rust test
// `requirement_probe_fixtures_are_the_rendered_declarations`). Everything
// from the first generated `const` to the default export is the product's
// rendered declaration byte for byte. The retained import stands in for the
// module-scope setup imports the checking module hoists.
//
// <script setup lang="ts" generic="T extends string">
// import type { PropType } from "vue";
// defineProps({ value: { type: String as unknown as PropType<T>, required: true as const } });
// defineEmits({ change: (payload: T) => true });
// </script>
import type { PropType } from "vue";

const __VerterRuntimeProps = <T extends string,>() => (({ value: { type: String as unknown as PropType<T>, required: true as const } }) satisfies import("vue").ComponentObjectPropsOptions);
const __VerterRuntimeEmits = <T extends string,>() => ({ change: (payload: T) => true });
type __VerterPublicProps<T extends string> = import("vue").PublicProps & import("vue").ExtractPublicPropTypes<ReturnType<typeof __VerterRuntimeProps<T>>> & import("vue").EmitsToProps<ReturnType<typeof __VerterRuntimeEmits<T>>>;
type __VerterPublicInstance<T extends string> = Omit<import("vue").ComponentPublicInstance, "$props" | "$emit" | "$slots"> & {
  readonly $props: __VerterPublicProps<T>;
  $emit: import("vue").EmitFn<ReturnType<typeof __VerterRuntimeEmits<T>>>;
  readonly $slots: import("vue").Slots;
};
declare const __VerterPublicComponent: {
  new <T extends string>(props: __VerterPublicProps<T>): __VerterPublicInstance<T>;
};
export default __VerterPublicComponent;
