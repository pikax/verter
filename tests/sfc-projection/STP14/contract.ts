/**
 * STP14 binder-aware public dependency capture contract.
 *
 * Names the capture products. Construction lives in the compiler
 * ProjectionBackend; TypeScript remains the type-answer owner, so nothing
 * here re-implements a type answer.
 */
export type { Instance as CaptureInstance } from "./probes/positive";
export { marker as capturedMarker, tagConstructor as capturedTag } from "./probes/positive";

export const acceptedProducts = [
  "PublicTypeDependencySlice",
  "BinderCapturePlan",
  "LiftedSourceDeclaration",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];

export const forbiddenAuthorities = [
  "eager-native-alias-expansion",
  "hidden-source-diagnostic",
  "invented-duplicate-declaration",
  "binder-captured-by-module-scope",
] as const;
