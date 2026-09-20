/**
 * STP11 statement-oriented setup and module lowering contract.
 *
 * Names the script products. Construction lives in the compiler
 * ProjectionBackend; TypeScript remains the type-answer owner.
 */
export type { Instance as SetupInstance } from "./probes/positive";
export { universalBody, asserted as angleAssertion } from "./probes/positive";

export const acceptedProducts = [
  "TsSetupProjection",
  "ModuleScopeProjection",
  "UniversalSetupBinder",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];

export const forbiddenAuthorities = [
  "specialized-generic-body",
  "duplicate-body-checker",
  "second-tsx-parse-for-ts-grammar",
] as const;
