import type { FirstListener, SecondListener } from "./operations";

/**
 * Clean: two listeners after a spread accumulate. A handler that covers only
 * the first payload is rejected.
 */
declare const spread: { onChange: FirstListener };
type AfterSpread = typeof spread & { onChange: SecondListener };
export const accReject: AfterSpread["onChange"] = (_payload: { from: "first" }) => {};
export const stp3HoverTarget = accReject;
export const stp3DefinitionTarget = accReject;
