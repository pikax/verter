import { splitUse } from "./operations";

/**
 * Characterized split witness: infer from rows, then post-check the callback.
 * Callback-only T is lost (unknown), so a number obligation fails.
 */
export const splitLost = splitUse()((row: { id: number }) => String(row.id));
export const splitT: number = splitLost.t;
export const stp3HoverTarget = splitT;
export const stp3DefinitionTarget = splitUse;
