/**
 * STP17 Ordered Vue attribute operations and runtime-key interpretation contract.
 *
 * Names the attribute products. Construction lives in the compiler
 * ProjectionBackend (`attribute_operations`); runtime keys follow the
 * runtime compiler's own property assembly; TypeScript remains the
 * type-answer owner for every consumer channel.
 */
export type { Instance as AttributeOperationsInstance } from "./probes/positive";

export const acceptedProducts = [
  "VueAttributeSequence",
  "RuntimePropertyKeyPlan",
  "AttributeConsumerRelation",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];

export const forbiddenAuthorities = [
  "native-type-answer-substitution",
  "callable-vue-sfc-replacement",
  "immediate-attrs-object-flattening",
  "unconditional-optional-spread-overwrite",
  "single-channel-collision-validation",
] as const;
