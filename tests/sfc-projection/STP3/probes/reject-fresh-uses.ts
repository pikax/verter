import { SharedSpecialization } from "./operations";

/**
 * Shared specialization: the second sibling is forced onto the first use's
 * string T, so a number obligation fails.
 */
export const second = new SharedSpecialization({
  rows: ["cached"],
  project: (row) => row,
});
export const contaminated: number = second.value;
export const stp3HoverTarget = contaminated;
export const stp3DefinitionTarget: typeof SharedSpecialization = SharedSpecialization;
