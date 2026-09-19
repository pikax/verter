/**
 * STP10 emission correspondence contract.
 *
 * Names the mapping products. Construction lives in the compiler
 * ProjectionBackend; TypeScript remains the type-answer owner.
 */
export type { Instance as EmittedInstance } from "./probes/positive";
export {
  coupled as coupledConstruction,
  coupledValue as coupledValueMember,
} from "./probes/positive";
export { inferred as inferredConstruction } from "./probes/positive";
export { explicit as explicitConstruction } from "./probes/positive";

export const acceptedProducts = [
  "ProjectionEmission",
  "ProjectionOrigin",
  "ObservationRole",
  "EditOrigin",
  "MappingProduct",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];

export const genericBinders = ["T", "U"] as const;

export const forbiddenAuthorities = [
  "independent-map-generator",
  "stale-map-reuse",
  "synthetic-authored-location",
] as const;
