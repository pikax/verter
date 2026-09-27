/**
 * STP19 Advanced generic binders and external component interoperability
 * contract.
 *
 * Names the advanced generic use and foreign component contract products.
 * Construction lives in the compiler ProjectionBackend
 * (`advanced_generic_uses`); every use applies the component's exact
 * contract before an attribute-tolerant fallback, inside one scope over the
 * parent's authored binder; TypeScript remains the owner of overload
 * selection, inference and constraint checking.
 */
export type { Instance as AdvancedGenericUseInstance } from "./probes/positive";

export const acceptedProducts = [
  "AdvancedGenericUseProjection",
  "ForeignComponentContractAdapter",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];

export const forbiddenAuthorities = [
  "native-type-answer-substitution",
  "callable-vue-sfc-replacement",
  "broad-any-constructor",
  "fixed-n-signature-flattening",
  "open-argument-constructor-widening",
  "fabricated-generic-reconstruction",
  "constraint-collapse-of-forwarded-binders",
] as const;
