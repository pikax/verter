import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";

export const HARNESS_ROOT = resolve(dirname(fileURLToPath(import.meta.url)), "..");
export const REPO_ROOT = resolve(HARNESS_ROOT, "..", "..");
// BF2_EVIDENCE_ROOT exists only so drift-refusal self-tests can point the
// harness at a mutated copy of the evidence tree in a child process and
// prove rejection before any compiler invocation. Never set in normal
// operation.
export const EVIDENCE_ROOT = process.env.BF2_EVIDENCE_ROOT
  ? resolve(process.env.BF2_EVIDENCE_ROOT)
  : resolve(HARNESS_ROOT, "evidence");
export const VUE_EVIDENCE_LOCK = resolve(EVIDENCE_ROOT, "oracles/vue/package-lock.json");
export const SVELTE_EVIDENCE_LOCK = resolve(EVIDENCE_ROOT, "oracles/svelte/package-lock.json");
export const VUE_EVIDENCE_CLOSURE = resolve(EVIDENCE_ROOT, "oracles/vue/closure.tsv");
export const SVELTE_EVIDENCE_CLOSURE = resolve(EVIDENCE_ROOT, "oracles/svelte/closure.tsv");
// BF2_GOLDENS_ROOT exists only so self-tests can run the generator's
// `--check` against a doctored copy of the golden set in a child process
// and prove refusal. Never set in normal operation.
export const GOLDENS_ROOT = process.env.BF2_GOLDENS_ROOT
  ? resolve(process.env.BF2_GOLDENS_ROOT)
  : resolve(HARNESS_ROOT, "goldens");
export const FIXTURES_ROOT = resolve(HARNESS_ROOT, "fixtures");
