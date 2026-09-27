// Declaration pin for the merged-interface constructor probe, rendered from
// `Merged.vue` (the carrier inlined in the Rust test
// `requirement_probe_fixtures_are_the_rendered_declarations`). Everything
// from the first generated `type` to the default export is the product's
// rendered declaration byte for byte. The interfaces stand in for the
// module-scope type declarations the checking module hoists, which is not
// this product.
//
// <script lang="ts">
// interface Props { label?: string }
// </script>
// <script setup lang="ts">
// interface Props { id: number }
// defineProps<Props>();
// defineModel<string>({ required: true as const });
// </script>
interface Props { label?: string }
interface Props { id: number }

type __VerterPublicProps = import("vue").PublicProps & (Props) & { "modelValue": string; "modelModifiers"?: Partial<Record<string, true>>; "onUpdate:modelValue"?: (value: string) => void };
type __VerterPublicInstance = Omit<import("vue").ComponentPublicInstance, "$props" | "$emit" | "$slots"> & {
  readonly $props: __VerterPublicProps;
  $emit: ((event: "update:modelValue", value: string) => void);
  readonly $slots: import("vue").Slots;
};
declare const __VerterPublicComponent: {
  new (props: __VerterPublicProps): __VerterPublicInstance;
};
export default __VerterPublicComponent;
