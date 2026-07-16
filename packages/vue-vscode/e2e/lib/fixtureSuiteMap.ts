/**
 * Fixture → suite path filters.
 *
 * Only suites matching a fixture's globs are loaded. Specialty fixtures do not
 * load the entire legacy tree (avoids hundreds of pending skips).
 *
 * Globs are matched against the path under `e2e/suite/` with posix slashes.
 */
const LEGACY_SUITE_GLOBS = [
  // Top-level legacy feature suites (not under parity/ or frameworks/)
  "activation.test",
  "attrs-fallthrough.test",
  "auto-close-tag.test",
  "barrel-exports.test",
  "barrel-type-integrity.test",
  "code-actions.test",
  "completion.test",
  "completionBenchmark.test",
  "component-meta.test",
  "decorations.test",
  "definition.test",
  "diagnostics.test",
  "document-symbols.test",
  "editor-owned-project.test",
  "external-file-changes.test",
  "generic-attrs.test",
  "hover.test",
  "hover-provenance.test",
  "import-resolution.test",
  "imported-props.test",
  "inlay-hints.test",
  "provider-parity.test",
  "references.test",
  "rename.test",
  "script-block.test",
  "startupBenchmark.test",
  "style-block.test",
  "svelte-carrier-parity.test",
  "timing.test",
  "_teardown.test",
] as const;

export const FIXTURE_SUITE_GLOBS: Readonly<Record<string, readonly string[]>> = {
  "single-project": LEGACY_SUITE_GLOBS,
  monorepo: LEGACY_SUITE_GLOBS,
  "tsconfig-extends": LEGACY_SUITE_GLOBS,
  "tsconfig-references": LEGACY_SUITE_GLOBS,
  "path-aliases": LEGACY_SUITE_GLOBS,
  "composite-paths": LEGACY_SUITE_GLOBS,
  "no-config": LEGACY_SUITE_GLOBS,
  "single-file": LEGACY_SUITE_GLOBS,
  "barrel-exports": LEGACY_SUITE_GLOBS,
  "editor-owned-project": ["editor-owned-project.test"],

  "vue-contract": ["frameworks/vue/"],
  "svelte-contract": ["frameworks/svelte/"],

  "vue-parity": ["parity/vue/", "parity/shared/", "parity/lsp-extras.test"],
  "svelte-parity": ["parity/svelte/", "parity/shared/", "parity/lsp-extras.test"],
  "mixed-parity": ["parity/mixed/"],
  "multi-root-parity": ["parity/multi-root/"],
  "ecosystem-parity": ["parity/ecosystem/"],
};

/**
 * Return true if a suite file (posix path under e2e/suite/) is allowed for fixture.
 */
export function suiteAllowedForFixture(fixture: string, suiteRelPosix: string): boolean {
  const globs = FIXTURE_SUITE_GLOBS[fixture];
  if (!globs) {
    // Unknown specialty names fail closed (load nothing) so typos do not green-wash.
    if (fixture.includes("parity") || fixture.includes("contract")) return false;
    // Unknown legacy-like fixture: legacy suites only
    return LEGACY_SUITE_GLOBS.some((g) => suiteRelPosix.includes(g));
  }
  const normalized = suiteRelPosix.replace(/\\/g, "/");
  return globs.some((g) => matchGlob(normalized, g));
}

function matchGlob(pathPosix: string, glob: string): boolean {
  const g = glob.replace(/\\/g, "/");
  if (g === "**/*") return true;
  if (g.endsWith("/")) {
    return pathPosix.includes(g);
  }
  return pathPosix.includes(g);
}
