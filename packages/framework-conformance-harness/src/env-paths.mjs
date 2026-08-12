// Resolves the local scratch paths this harness needs for operations that
// touch the pinned Vue/Svelte GIT SOURCE checkouts directly (case-manifest
// re-enumeration, source/package drift-refusal self-tests). Golden
// generation and structural comparison do NOT need these — they only need
// the pinned npm packages, which are ordinary workspace devDependencies
// (see package.json + domain-pin.mjs).
//
// These checkouts are deliberately NOT committed to the repository (they are
// full upstream working trees, thousands of files). They are provisioned ONCE,
// reproducibly and fail-closed, by `node scripts/provision-oracle-checkouts.mjs`,
// which fetches exactly the commits pinned in domain-pin.mjs into
// <package>/.oracle-checkouts/<framework> (gitignored) and verifies them with
// checkout-pin.mjs before returning. That provisioning step is the ONLY
// network-touching operation in the package and is never invoked from a test:
// once it has run, every suite below runs entirely offline against local files.
//
// Resolution order: explicit BF2_VUE_SOURCE / BF2_SVELTE_SOURCE env vars (a
// contributor or CI job pointing at their own pinned clones), else the default
// provisioned cache (BF2_ORACLE_CACHE or <package>/.oracle-checkouts). When
// neither exists, callers must treat the affected suite as SKIPPED with an
// explicit reason — never as a silent pass.

import { existsSync } from "node:fs";
import { resolve, join } from "node:path";

import { HARNESS_ROOT } from "./paths.mjs";

export const DEFAULT_ORACLE_CACHE_ROOT = process.env.BF2_ORACLE_CACHE
  ? resolve(process.env.BF2_ORACLE_CACHE)
  : join(HARNESS_ROOT, ".oracle-checkouts");

function pick(envValue, framework) {
  const explicit = envValue ? resolve(envValue) : undefined;
  if (explicit && existsSync(explicit)) return explicit;
  const fallback = join(DEFAULT_ORACLE_CACHE_ROOT, framework);
  return existsSync(fallback) ? fallback : undefined;
}

/**
 * @returns {{ vueSource: string|undefined, svelteSource: string|undefined }}
 */
export function oracleSourcePaths() {
  return {
    vueSource: pick(process.env.BF2_VUE_SOURCE, "vue"),
    svelteSource: pick(process.env.BF2_SVELTE_SOURCE, "svelte"),
  };
}

export function oracleSourcePathsAvailable() {
  const { vueSource, svelteSource } = oracleSourcePaths();
  return Boolean(vueSource && svelteSource);
}
