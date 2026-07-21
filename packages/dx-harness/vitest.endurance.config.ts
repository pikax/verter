import { fileURLToPath } from "node:url";

import { defineConfig } from "vitest/config";

// Endurance/soak lane: long-running scenarios against the REAL `verter-lsp`
// binary. Deliberately EXCLUDED from the default hermetic `pnpm test` run (see
// vitest.config.ts); invoked via `pnpm --filter @verter/dx-harness test:endurance`.
// The provider route comes from VERTER_ENDURANCE_PROVIDER (default tsgo) — the
// caller runs the suite once per route. Serialized: one worker, one file at a
// time, generous timeouts for env-extended soaks.
export default defineConfig({
  resolve: {
    alias: {
      "@verter/lsp-test-client": fileURLToPath(
        new URL("../lsp-test-client/src/index.ts", import.meta.url),
      ),
    },
  },
  test: {
    globals: false,
    include: ["test/endurance.*.test.ts"],
    exclude: ["**/node_modules/**", "**/dist/**"],
    fileParallelism: false,
    maxWorkers: 1,
    testTimeout: 3_600_000,
    hookTimeout: 600_000,
  },
});
