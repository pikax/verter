/**
 * STP8 evidence-based ABI and topology ratification contract.
 *
 * Ratifies the constructor-first Vue projection ABI (one ordinary public
 * constructor plus the merged ComponentPublicInstance inference witness) and
 * the frozen dialect topology against the STP2-STP7 ledgers. Production
 * constructor emit remains STP16; specialization runtime STP18; full Vue
 * typing/IDE STP59; legacy retirement STP58/STS15.
 */
export type { Instance as RatifiedInstance } from "./probes/positive";
export {
  coupled as coupledConstruction,
  coupledValue as coupledValueMember,
} from "./probes/positive";
export { inferred as inferredConstruction } from "./probes/positive";
export { explicit as explicitConstruction } from "./probes/positive";

export const acceptedProducts = [
  "AcceptedProjectionArchitecture",
  "AcceptedVueConstructorABI",
  "AcceptedTopologyMatrix",
  "HelperABI",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];

export const selectedCandidate = {
  id: "single-public-two-binder-constructor-plus-merged-instance-interface",
  constructorFirst: true,
  defaultExport: "constructor-shaped-declare-class",
  instanceTypeSpelling: "InstanceType<typeof Comp>",
  binderFamily: "Comp<T = unknown, U = unknown>",
} as const;

export const rejectedCandidates = [
  "callable-sfc-default-export",
  "broad-construct-overload",
  "split-inference-plus-post-specialization",
  "fixed-n-overload-claim",
] as const;

export type RejectedCandidate = (typeof rejectedCandidates)[number];

export const topologyDialects = ["ts", "tsx", "js", "jsx"] as const;

export type TopologyDialect = (typeof topologyDialects)[number];
