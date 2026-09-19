import type { FirstListener, SecondListener } from "./operations";

/**
 * Dirty twin: object-spread last-write-wins drops the first listener's payload
 * obligation. A second-payload-only handler is incorrectly accepted.
 */
declare const first: FirstListener;
declare const second: SecondListener;
const spread = { onChange: first };
const after = { ...spread, onChange: second };
export const lwwAccept: typeof after.onChange = (_payload: { from: "second" }) => {};
export const stp3HoverTarget = lwwAccept;
export const stp3DefinitionTarget = lwwAccept;
