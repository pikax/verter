import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const HARNESS_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const REPO_ROOT = resolve(HARNESS_ROOT, "..", "..");
export const EVIDENCE_ROOT = resolve(
  REPO_ROOT,
  "docs/arch/refactor/rev11/evidence/framework-conformance",
);
export const VUE_EVIDENCE_LOCK = resolve(EVIDENCE_ROOT, "oracles/vue/package-lock.json");
export const SVELTE_EVIDENCE_LOCK = resolve(EVIDENCE_ROOT, "oracles/svelte/package-lock.json");
export const GOLDENS_ROOT = resolve(HARNESS_ROOT, "goldens");
export const FIXTURES_ROOT = resolve(HARNESS_ROOT, "fixtures");
