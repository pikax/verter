/**
 * STP3 coupled-inference contract.
 *
 * Whole-signature contextual construction infers T from rows (or a callback
 * channel) and U from the project return, then projects U across emit, slot,
 * model, and expose. Production use-site inference remains STP18; ABI
 * ratification remains STP8.
 */
export { callbackOnly, coupled, parented, siblingA, siblingB } from "./probes/accept-coupled";
export { permA, permB } from "./probes/accept-order-independent";

export const selectedWitness = "whole-signature-contextual-construction" as const;
export const rejectedWitness = "split-inference-plus-post-specialization" as const;

export const inferenceChannels = [
  "rows",
  "project",
  "onChange",
  "modelValue",
  "update:modelValue",
  "slot-default",
  "expose-value",
] as const;

export type InferenceChannel = (typeof inferenceChannels)[number];
export type SelectedWitness = typeof selectedWitness;
