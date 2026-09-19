import { counter } from "./state-module.svelte";

/** STS0-anchor-negative: number is not assignable to string (TS2322). */
export const sts0Negative: string = counter.count;
