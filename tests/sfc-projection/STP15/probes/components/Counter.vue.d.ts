// Contract-shape pin for the STP15 tsc probes, not compiler output
// (full carrier-to-declaration generation is STP58-owned). Every member
// pinned here has a live-projection counterpart asserted by the
// `stp15_counter_fixture_matches_projection` Rust test over equivalent
// setup bytes plus `binding_views_reads_admitted_carrier_blocks` over
// real carrier bytes: fixture drift fails there instead of passing
// silently here.
import type { ComponentPublicInstance, Ref } from "vue";

// Script-side Ref wrapper: `<script setup>` still sees `Ref` (`.value`)
// while the template reads the value unwrapped.
export declare const countRef: Ref<number>;

export type CounterProps = {
  readonly title?: string;
};

// `<script setup>` component with a top-level ref, a getter-only computed,
// a writable computed whose setter domain (`string`) differs from its read
// type (`number`), a two-way model ref, and a readonly prop. The default
// export is constructor-shaped; instance members stay visible through
// `InstanceType<typeof Comp>`.
export declare class Comp {
  constructor(props?: CounterProps);
  readonly $props: CounterProps;
  // Template-unwrapped read view of the script-side `Ref<number>`.
  count: number;
  // Getter-only computed: readable, never assignable.
  readonly doubled: number;
  // Writable computed: reads `number`, accepts the setter domain
  // (`string`) through direct assignment, not merely the read type.
  get label(): number;
  set label(value: string);
  // Two-way model ref: readable and writable.
  modelValue: string;
  readonly title: string;
  increment(step?: number): void;
}

export interface Comp extends ComponentPublicInstance<
  CounterProps,
  { count: number; label: number; modelValue: string },
  {},
  { doubled: number; title: string },
  { increment(step?: number): void }
> {}

export default Comp;
