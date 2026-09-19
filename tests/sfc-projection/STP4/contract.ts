/**
 * STP4 dialect/topology contract.
 *
 * Supplemental files are checked outputs, not named import targets or a shared
 * lexical scope. External scripts keep their own source identity. Vue-illegal
 * script-setup src stays rejected even when generated TypeScript typechecks.
 * Production topology ABI remains STP8.
 */
export { asserted } from "./probes/angle-assertion";
export { a as ownerA, b as ownerB } from "./probes/accept-external-owner";
export { element as authoredTsx } from "./probes/accept-tsx-authored";

export const dialects = ["ts", "tsx", "js", "jsx"] as const;
export type Dialect = (typeof dialects)[number];
