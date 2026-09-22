/**
 * STP15 Live template read/write views and actual usage accounting contract.
 *
 * Names the view products. Construction lives in the compiler
 * ProjectionBackend (`binding_views`); usage accounting is built from
 * authored references only; TypeScript remains the type-answer owner.
 */
export type { Instance as BindingViewsInstance } from "./probes/positive";
export { countRef as scriptSideRef } from "./probes/positive";

export const acceptedProducts = [
  "TemplateReadView",
  "TemplateWriteTarget",
  "BindingUsageSet",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];

export const forbiddenAuthorities = [
  "native-type-answer-substitution",
  "callable-vue-sfc-replacement",
  "universal-mutable-write-alias",
  "synthetic-void-usage-scaffold",
  "immutable-snapshot-narrowing",
] as const;
