/**
 * STP6 packed-consumer / declaration-closure contract.
 *
 * Fresh consumers resolve only published declarations. Source and packed
 * InstanceType/generic channels agree. Production declaration emit remains
 * STP16/STP59; ABI ratification remains STP8.
 */
export type { Instance as PackedInstance } from "./probes/accept-package-instance";
export { packed as packedGeneric, source as sourceGeneric } from "./probes/accept-package-generics";

export const consumptionModes = [
  "direct-import",
  "alias",
  "barrel",
  "namespace",
  "project-references",
  "package-exports",
] as const;

export const resolutionModes = ["bundler", "node16", "nodenext"] as const;

export type ConsumptionMode = (typeof consumptionModes)[number];
export type ResolutionMode = (typeof resolutionModes)[number];
