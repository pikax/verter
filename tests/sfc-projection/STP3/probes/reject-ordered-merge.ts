import type { AccumulatedListeners } from "./operations";

/**
 * Clean: two listeners after a spread accumulate. A handler that covers only
 * the first payload is rejected.
 */
declare const spread: { rows: { id: number }[] };
export const accReject: AccumulatedListeners = (_payload: { from: "first" }) => {};
export const stp3HoverTarget = accReject;
export const stp3DefinitionTarget = accReject;
void spread;
