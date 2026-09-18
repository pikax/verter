import type { LastWriteWinsListeners } from "./operations";

/**
 * Dirty twin: last-write-wins drops the first listener's payload obligation.
 * A first-payload-only handler is incorrectly accepted.
 */
declare const spread: { rows: { id: number }[] };
export const lwwAccept: LastWriteWinsListeners = (_payload: { from: "second" }) => {};
export const stp3HoverTarget = lwwAccept;
export const stp3DefinitionTarget = lwwAccept;
void spread;
