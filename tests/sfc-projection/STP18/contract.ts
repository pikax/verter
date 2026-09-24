/**
 * STP18 One component-use inference transaction and specialized observations contract.
 *
 * Names the component-use products. Construction lives in the compiler
 * ProjectionBackend (`component_uses`); each use is checked through one
 * construction of the used component and every observation reads that
 * use's witness; TypeScript remains the owner of inference, overload
 * selection and contextual typing.
 */
export type { Instance as ComponentUseInstance } from "./probes/positive";

export const acceptedProducts = [
  "ComponentUseWitness",
  "SpecializedUseObservation",
  "InferenceTransaction",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];

export const forbiddenAuthorities = [
  "native-type-answer-substitution",
  "callable-vue-sfc-replacement",
  "broad-any-constructor",
  "per-channel-uninstantiated-extraction",
  "split-inference-plus-post-specialization-for-contributing-channels",
  "fabricated-listener-array",
  "shared-specialization-across-uses",
] as const;
