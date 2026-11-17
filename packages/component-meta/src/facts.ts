/** Machine causes are open numeric vocabulary; readers retain unfamiliar codes. */
export type FactCause = number;
export type FactCauses = readonly [FactCause, ...FactCause[]];
/** Availability follows the arm. A false value and proven absence remain complete values. */
export type FactResult<T> =
  | {
      readonly kind: "complete";
      readonly value: T;
      readonly open?: boolean;
      /** The wire publication's absence reason, independent of any supplied descriptor. */
      readonly absence?: "unannotated" | "branchDivergent" | "unclassifiedConstructor";
    }
  | {
      readonly kind: "approximate";
      readonly value: T;
      readonly causes: FactCauses;
      readonly open?: boolean;
    }
  | { readonly kind: "unavailable"; readonly causes: FactCauses };

/** Accepted props, events and fallthrough share the producer resolver claim. */
export function projectAcceptedSurfaceCompleteness(
  fact: FactResult<unknown>,
): "exact" | "lowerBound" {
  return fact.kind === "complete" && !fact.open ? "exact" : "lowerBound";
}
