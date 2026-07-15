// @ai-generated - Synthetic relation-engine probes via `A extends B ? T : F`.
//
// Each exported alias encodes one row of the relation-engine coverage
// matrix. The published value is a string-literal probe (`"yes"` /
// `"no"`) or a precise structural type, so a single typeinfo resolve
// per alias tells us which branch the relation engine picked.

// =====================================================================
// Row 1: Top / bottom / unknown rows.
//
// Notes on TS7 semantics:
//  - Direct `any extends string` distributes specially (both branches
//    survive) because `any` carries a "could be anything" union semantics
//    at the relation engine's check site.
//  - Direct `never extends string` does NOT distribute (the check is a
//    concrete `never`, not a bare type parameter). Since `never` is the
//    bottom type and is assignable to everything, the True branch wins
//    → `"yes"`.
//  - `IsStringDistributive<never>` (via generic helper) DOES distribute
//    over the bare type parameter T. `never` has zero constituents, so
//    the distributive conditional collapses to `never` itself.
// =====================================================================

export type IsStringDistributive<T> = T extends string ? "yes" : "no";

export type AnyExtendsString = any extends string ? "yes" : "no";
export type UnknownExtendsString = unknown extends string ? "yes" : "no";
export type NeverExtendsStringDirect = never extends string ? "yes" : "no";
export type NeverExtendsStringViaGeneric = IsStringDistributive<never>;
export type StringExtendsAny = string extends any ? "yes" : "no";
export type StringExtendsUnknown = string extends unknown ? "yes" : "no";
export type StringExtendsNever = string extends never ? "yes" : "no";

// =====================================================================
// Row 2: Optional property assignability.
// =====================================================================

export type RequiredToOptional = { a: string } extends { a?: string } ? "yes" : "no";
export type OptionalToRequired = { a?: string } extends { a: string } ? "yes" : "no";
export type EmptyToAllOptional = {} extends { a?: string } ? "yes" : "no";

// =====================================================================
// Row 3: Readonly property assignability.
// =====================================================================

export type MutableToReadonly = { a: string } extends { readonly a: string } ? "yes" : "no";
export type ReadonlyToMutable = { readonly a: string } extends { a: string } ? "yes" : "no";

// =====================================================================
// Row 4: Function parameter contravariance.
// =====================================================================

export type WiderParamToNarrower = ((x: "a" | "b") => void) extends (x: "a") => void ? "yes" : "no";
export type NarrowerParamToWider = ((x: "a") => void) extends (x: "a" | "b") => void ? "yes" : "no";

// =====================================================================
// Row 5: Tuple length / rest assignability.
// =====================================================================

export type FixedToFirstRest = [string, number] extends [string, ...unknown[]] ? "yes" : "no";
export type RestToFixed = [string, ...number[]] extends [string, number] ? "yes" : "no";
export type OneToOneOptional = [string] extends [string, number?] ? "yes" : "no";
export type EmptyToReadonlyArray = [] extends readonly string[] ? "yes" : "no";

// =====================================================================
// Row 6: Union distribution vs non-distribution.
//
// Reuses `IsStringDistributive<T>` from Row 1. The non-distributive
// form wraps the check type in a 1-tuple so the union is treated as a
// whole rather than distributed across the conditional.
// =====================================================================

export type IsStringNonDistributive<T> = [T] extends [string] ? "yes" : "no";

export type DistributiveOverUnion = IsStringDistributive<string | number>;
export type NonDistributiveOverUnion = IsStringNonDistributive<string | number>;

// =====================================================================
// Row 7: Intersection assignability.
// =====================================================================

export type IntersectionExtendsBase = { a: string } & { b: number } extends { a: string }
  ? "yes"
  : "no";
export type BaseExtendsIntersection = { a: string } extends { a: string } & { b: number }
  ? "yes"
  : "no";

// =====================================================================
// Row 8: `infer` bindings.
// =====================================================================

export type ExtractValue<T> = T extends { value: infer V } ? V : never;
export type ExtractHead<T> = T extends [infer H, ...unknown[]] ? H : never;
export type ExtractTail<T> = T extends [unknown, ...infer R] ? R : never;
export type ExtractReturn<T> = T extends (...args: any[]) => infer R ? R : never;
export type ExtractParams<T> = T extends (...args: infer A) => any ? A : never;
export type ExtractSingleParam<T> = T extends (x: infer X) => any ? X : never;

export type InferValueOfObject = ExtractValue<{ value: number }>;
export type InferHeadOfTuple = ExtractHead<[1, 2, 3]>;
export type InferTailOfTuple = ExtractTail<[1, 2, 3]>;
export type InferReturnOfFunction = ExtractReturn<() => "hello">;
export type InferParamsOfFunction = ExtractParams<(x: string, y?: number) => void>;
export type InferSingleParam = ExtractSingleParam<(s: string) => void>;
