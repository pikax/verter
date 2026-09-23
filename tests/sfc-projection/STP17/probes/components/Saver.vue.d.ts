// Contract-shape pin for the STP17 tsc probes, not compiler output
// (full carrier-to-declaration generation is STP58-owned). The runtime-key
// and consumer-channel facts these probes rely on are asserted by the
// `attribute_ops_*` Rust tests over equivalent template bytes and by
// `attribute_operations_reads_admitted_carrier_blocks` over real carrier
// bytes: the `onSave` key reaches both the declared callback prop and the
// `save` emit handler lookup, so a value for it validates against both.
import type { ComponentPublicInstance } from "vue";

export type SaverProps = {
  readonly title?: string;
  // Declared callback prop sharing the `onSave` runtime key with the
  // `save` emit below: one raw key, two consumer contracts.
  readonly onSave?: (id: number) => void;
};

export type SaverEmits = {
  save: (id: number) => void;
};

// `<script setup>` component whose default export stays constructor-shaped;
// instance members stay visible through `InstanceType<typeof Comp>`.
export declare class Comp {
  constructor(props?: SaverProps);
  readonly $props: SaverProps;
  readonly title: string;
  readonly saveCount: number;
}

export interface Comp extends ComponentPublicInstance<
  SaverProps,
  {},
  {},
  { title: string; saveCount: number },
  {},
  SaverEmits
> {}

export default Comp;
