/**
 * STS0 dirty twin: callback-form `$derived` against the pinned expression
 * signature is TS2322 (`() => number` is not assignable to `number`).
 * Callback evaluation belongs to `$derived.by`.
 */
declare function $derived<T>(expression: T): T;

/** STS0-anchor-negative: function is not assignable to number (TS2322). */
export const sts0Negative: number = $derived(() => 0);
