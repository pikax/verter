/**
 * STP2 constructor-compatibility contract.
 *
 * Specialization kinds observed through real Vue `.vue` imports:
 * unbound extraction, explicit specialization, props-inferred construction,
 * and template/utility use. Production constructor emit remains STP16;
 * ABI ratification remains STP8.
 */
export type { Instance as ConcreteInstance } from "./probes/accept-instance-concrete";
export type { Unbound as UnboundGenericInstance } from "./probes/accept-instance-generic";
export type { NumberInstance as ExplicitGenericInstance } from "./probes/accept-instance-explicit";
export {
  inferred as inferredConstruction,
  sibling as independentSibling,
} from "./probes/accept-constructor-inferred";
export {
  app as vueCreateApp,
  vnode as vueH,
  element as vueTsx,
} from "./probes/accept-vue-utilities";

export const specializationKinds = [
  "unbound-extraction",
  "explicit-specialization",
  "props-inferred-construction",
  "template-use",
] as const;

export type SpecializationKind = (typeof specializationKinds)[number];
