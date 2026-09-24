// Declaration pin for the `withDefaults` constructor probe, rendered from
// `Defaulted.vue` (the carrier inlined in the Rust test
// `requirement_probe_fixtures_are_the_rendered_declarations`). Everything
// from the first generated `type` to the default export is the product's
// rendered declaration byte for byte.
//
// <script setup lang="ts">
// withDefaults(defineProps<{ test: string; label?: string }>(), { test: "x" });
// </script>

type __VerterPropsWithDefaults<P, K extends PropertyKey> = { [Q in keyof P as Q extends K ? never : Q]: P[Q] } & { [Q in keyof P as Q extends K ? Q : never]?: P[Q] };
type __VerterPublicProps = import("vue").PublicProps & __VerterPropsWithDefaults<({ test: string; label?: string }), "test">;
type __VerterPublicInstance = Omit<import("vue").ComponentPublicInstance, "$props" | "$emit" | "$slots"> & {
  readonly $props: __VerterPublicProps;
  $emit: import("vue").EmitFn<{}>;
  readonly $slots: import("vue").Slots;
};
declare const __VerterPublicComponent: {
  new (props?: __VerterPublicProps): __VerterPublicInstance;
};
export default __VerterPublicComponent;
