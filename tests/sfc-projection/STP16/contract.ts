/**
 * STP16 Vue public constructor and instance contract.
 *
 * Names the contract products. Construction lives in the compiler
 * ProjectionBackend (`public_constructor`); the rendered declaration keeps
 * one generic construct signature and TypeScript remains the type-answer
 * owner.
 */
export type { Instance as PublicInstance, NumberInstance } from "./probes/positive";

export const acceptedProducts = [
  "VuePublicConstructorContract",
  "PublicInstanceProjection",
  "ConstructorCompatibilityReceipt",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];

export const forbiddenAuthorities = [
  "native-type-answer-substitution",
  "callable-vue-sfc-replacement",
  "broad-any-constructor",
  "overload-fallback",
  "private-setup-leak",
] as const;
