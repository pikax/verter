/**
 * STS0-svelte-abi dirty twin: the forbidden encoding — a Vue constructor is
 * required for a modern Svelte Component. This file is never part of a
 * compiled program; the STS0 protocol must reject this spelling on sight
 * (constructor-shaped class, InstanceType-of-a-class, defineComponent, Vue
 * $emit event convention).
 */
import { defineComponent } from "vue";

export declare class SvelteWidget {
  constructor(internals: unknown, props: { item: unknown });
  $emit(event: "toggle", value: boolean): void;
}

export type SvelteInstance = InstanceType<typeof SvelteWidget>;

export const ShimmedWidget = defineComponent({});
