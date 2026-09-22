// Contract-shape pin for the STP13 tsc probes: the constructor-shaped public
// surface `VueProjectionBackend::options_projection` must preserve (checked
// by `options_projection_reads_admitted_carrier_blocks` over real carrier
// bytes; full carrier-to-declaration generation is STP58-owned). Not
// compiler output.
import type { ComponentPublicInstance } from "vue";

export type CombinedProps = {
  readonly store?: string;
};

// Combined normal-plus-setup component. `mixinCount` and `mixinReset` are
// mixin/extends-inherited public members: they remain visible and correctly
// typed on the instance. `storeKey` is a named module export that coexists
// with the default export without leaking into template scope.
export declare class Merged {
  constructor(props?: CombinedProps);
  readonly $props: CombinedProps;
  readonly store: string;
  readonly mixinCount: number;
  mixinReset(): void;
  setupFlag: boolean;
}

export interface Merged extends ComponentPublicInstance<
  CombinedProps,
  { setupFlag: boolean },
  {},
  { store: string; mixinCount: number },
  { mixinReset(): void }
> {}

export declare const storeKey: string;

export default Merged;
