/**
 * STP13 Classic Options API and combined-script compatibility contract.
 *
 * Names the script products. Construction lives in the compiler
 * ProjectionBackend (`options_projection`); TypeScript remains the
 * type-answer owner.
 */
export type { Instance as OptionsInstance } from "./probes/positive";
export { bump as optionsMethod, fromMixin as mixinMember } from "./probes/positive";

export const acceptedProducts = [
  "OptionsComponentProjection",
  "CombinedScriptProjection",
  "OptionsTemplateBindingView",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];

export const forbiddenAuthorities = [
  "native-type-answer-substitution",
  "callable-vue-sfc-replacement",
  "duplicate-script-body-checker",
  "text-recognised-define-component-alias",
] as const;
