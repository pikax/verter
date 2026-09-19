/**
 * STP9 source-backed projection plan contract.
 *
 * Names the plan products. Construction lives in the compiler
 * ProjectionBackend; TypeScript remains the type-answer owner.
 */
export type { Instance as PlannedInstance } from "./probes/positive";
export {
  coupled as coupledConstruction,
  coupledValue as coupledValueMember,
} from "./probes/positive";
export { inferred as inferredConstruction } from "./probes/positive";
export { explicit as explicitConstruction } from "./probes/positive";

export const acceptedProducts = [
  "ProjectionPlan",
  "BindingOriginId",
  "ComponentUseId",
  "GenericBinderRef",
  "OrderedAttributeOp",
  "BranchEdge",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];

export const genericBinders = ["T", "U"] as const;

export const forbiddenPlanAuthorities = ["native-typeinfo", "native-assignability"] as const;
