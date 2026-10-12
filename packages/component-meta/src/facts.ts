/** Machine causes are open numeric vocabulary; readers retain unfamiliar codes. */
export type FactCause = number;
export type FactCauses = readonly [FactCause, ...FactCause[]];
/** Availability follows the arm. A false value and proven absence remain complete values. */
export type FactResult<T> =
  | { readonly kind: "complete"; readonly value: T; readonly open?: boolean }
  | {
      readonly kind: "approximate";
      readonly value: T;
      readonly causes: FactCauses;
      readonly open?: boolean;
    }
  | { readonly kind: "unavailable"; readonly causes: FactCauses };
