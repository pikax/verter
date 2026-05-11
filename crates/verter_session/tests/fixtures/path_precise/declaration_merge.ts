// Archetype: declaration merge — interface merge + overloaded function
// + namespace+value merge. Exercises merged-symbol identity (R7).
//
// Stage 0 baseline characterisation: today the policy walker collapses
// merged parts into a single resolved declaration; the parts cannot be
// individually invalidated.
//
// Stage 6d post-change discriminator: per R7 the
// `ResolvedDeclSlotIdentity.merged_symbol_name` is STABLE across
// declaration reordering and TS declaration merging. `merged_parts`
// in `VersionedDeclIdentity` is payload, not validation. A consumer
// that observes only one overload's body should NOT invalidate when
// a NEW overload is added.

// --- Interface merge ---
export interface MergedInterface {
  /** Part 1 — first declaration. */
  a: number;
}
export interface MergedInterface {
  /** Part 2 — merged second declaration. */
  b: string;
}

// --- Function overload merge ---
export function mergedFn(x: number): number;
export function mergedFn(x: string): string;
export function mergedFn(x: number | string): number | string {
  return x;
}

// --- Namespace + value merge ---
export function mergedNamespacedValue(): void {}
export namespace mergedNamespacedValue {
  /** Namespace contribution merged with the function. */
  export const tag: string = "merged";
}
