/**
 * STS0 Svelte projection profile and current-feature contract.
 *
 * Ratifies the framework-specific Svelte profile (function-shaped Component,
 * runes vs legacy semantics, legal .svelte/.svelte.ts/.svelte.js surfaces,
 * explicit checking/publishing behavior per profile) against the STP7/STP8
 * ledgers and the pinned svelte/typescript toolchain. Full Svelte typing and
 * IDE stays with STS15, module/instance binders with STS1, runtime
 * compilation with SCP, styles with SST; legacy retirement stays with STS15.
 */
export type { SvelteComponent, Item } from "./probes/positive";
export { Widget as svelteComponent } from "./probes/positive";
export { counter, derivedCount, bump } from "./probes/state-module.svelte";

export const acceptedProducts = [
  "SvelteProjectionPolicy",
  "SvelteCurrentFeatureInventory",
  "SvelteEngineFrameworkMatrix",
] as const;

export type AcceptedProduct = (typeof acceptedProducts)[number];

export const selectedProfile = {
  id: "framework-specific-svelte-function-shaped-component",
  publicShape: 'function-shaped Component (import("svelte").Component family)',
  vueConstructorRequired: false,
  instanceTypeRequirement: "none",
  binderFamily: "SvelteComponent<Props, Exports>",
} as const;

export const rejectedCandidates = [
  "vue-constructor-required-for-svelte-component",
  "vue-event-model-ref-conventions",
  "unspecified-dialect-checking",
  "latest-tool-claim-without-pinned-provenance",
] as const;

export type RejectedCandidate = (typeof rejectedCandidates)[number];

export const dialectFileKinds = [".svelte", ".svelte.ts", ".svelte.js"] as const;

export type DialectFileKind = (typeof dialectFileKinds)[number];

export const semanticsModes = ["runes", "legacy", "both"] as const;

export type SemanticsMode = (typeof semanticsModes)[number];
