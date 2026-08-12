// Resolves the local scratch paths this harness needs for operations that
// touch the pinned Vue/Svelte GIT SOURCE checkouts directly (case-manifest
// re-enumeration, source/package drift-refusal self-tests). Golden
// generation and structural comparison do NOT need these — they only need
// the pinned npm packages, which are ordinary workspace devDependencies
// (see package.json + domain-pin.mjs).
//
// These checkouts are deliberately NOT committed to the repository (they are
// full upstream working trees, hundreds of files). A contributor or CI job
// that wants to run the git-checkout-dependent suites sets the two env vars
// below to local clones pinned at the exact commits in domain-pin.mjs. When
// unset, callers must treat the affected suite as SKIPPED with an explicit
// reason — never as a silent pass.

import { existsSync } from "node:fs";
import { resolve } from "node:path";

/**
 * @returns {{ vueSource: string|undefined, svelteSource: string|undefined }}
 */
export function oracleSourcePaths() {
  const vueSource = process.env.BF2_VUE_SOURCE ? resolve(process.env.BF2_VUE_SOURCE) : undefined;
  const svelteSource = process.env.BF2_SVELTE_SOURCE
    ? resolve(process.env.BF2_SVELTE_SOURCE)
    : undefined;
  return {
    vueSource: vueSource && existsSync(vueSource) ? vueSource : undefined,
    svelteSource: svelteSource && existsSync(svelteSource) ? svelteSource : undefined,
  };
}

export function oracleSourcePathsAvailable() {
  const { vueSource, svelteSource } = oracleSourcePaths();
  return Boolean(vueSource && svelteSource);
}
