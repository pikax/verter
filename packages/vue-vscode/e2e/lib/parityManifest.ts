/**
 * Accepted authored parity inventory. Counts are AST-verified by the e2e/lib unit
 * gate; the 73 declarative matrix cases are registered by the two zero-literal
 * matrix loader files and attested separately above.
 */
export const PARITY_LITERAL_TEST_COUNTS = {
  "ecosystem/paths.test.ts": 9,
  "lsp-extras.test.ts": 6,
  "mixed/workspace.test.ts": 7,
  "multi-root/workspace.test.ts": 6,
  "shared/code-action-apply.test.ts": 2,
  "shared/confidence.test.ts": 8,
  "shared/depth-apply.test.ts": 5,
  "shared/editor-extras.test.ts": 4,
  "shared/find-rename-exact.test.ts": 6,
  "shared/generics-advanced.test.ts": 16,
  "shared/ide-navigation.test.ts": 14,
  "shared/intrinsic-elements.test.ts": 5,
  "shared/js-surface.test.ts": 2,
  "shared/js-ts-actions.test.ts": 7,
  "shared/mapping-fidelity.test.ts": 4,
  "shared/perf-smoke.test.ts": 2,
  "shared/product-lifecycle.test.ts": 5,
  "shared/rename-symbols.test.ts": 5,
  "shared/slots.test.ts": 7,
  "shared/strict-props.test.ts": 5,
  "shared/style-css.test.ts": 7,
  "shared/testing-api-surface.test.ts": 7,
  "shared/type-negatives.test.ts": 5,
  "shared/typing-dx-deep.test.ts": 4,
  "shared/typing-edit.test.ts": 5,
  "svelte/daily.test.ts": 11,
  "svelte/features.test.ts": 6,
  "svelte/intellisense.test.ts": 3,
  "svelte/matrix.test.ts": 0,
  "svelte/public-surface.test.ts": 2,
  "svelte/runes-control.test.ts": 7,
  "vue/daily.test.ts": 12,
  "vue/fallthrough.test.ts": 8,
  "vue/features.test.ts": 7,
  "vue/intellisense.test.ts": 4,
  "vue/macros-control.test.ts": 8,
  "vue/matrix.test.ts": 0,
  "vue/public-surface.test.ts": 2,
} as const;

const authoredParityFiles = Object.keys(PARITY_LITERAL_TEST_COUNTS);
const sharedParityFiles = authoredParityFiles.filter((file) => file.startsWith("shared/"));

function compiled(files: readonly string[]): readonly string[] {
  return files.map((file) => `parity/${file.replace(/\.ts$/, ".js")}`).sort();
}

export function requiredParitySuiteFiles(fixture: string): readonly string[] | undefined {
  if (fixture === "vue-parity") {
    return compiled([
      "lsp-extras.test.ts",
      ...sharedParityFiles,
      ...authoredParityFiles.filter((file) => file.startsWith("vue/")),
    ]);
  }
  if (fixture === "svelte-parity") {
    return compiled([
      "lsp-extras.test.ts",
      ...sharedParityFiles,
      ...authoredParityFiles.filter((file) => file.startsWith("svelte/")),
    ]);
  }
  if (fixture === "mixed-parity") return compiled(["mixed/workspace.test.ts"]);
  if (fixture === "multi-root-parity") return compiled(["multi-root/workspace.test.ts"]);
  if (fixture === "ecosystem-parity") return compiled(["ecosystem/paths.test.ts"]);
  return undefined;
}
