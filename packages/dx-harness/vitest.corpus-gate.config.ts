import { fileURLToPath } from "node:url";

import { defineConfig } from "vitest/config";

// Corpus benchmark gate lane: drives the REAL `verter-lsp` binary against an
// EXTERNAL corpus (`VERTER_CORPUS_GATE_DIR`) on all three provider routes.
// Deliberately EXCLUDED from the default hermetic `pnpm test` run (the lane
// file is not named `*.test.ts`); invoked via
// `pnpm --filter @verter/dx-harness test:corpus-gate`. Without the env the
// lane records an honest explicit skip. Serialized: one worker, one file.
// The real wall-clock bounding lives INSIDE the harness (per-route budgets +
// abandon races); the vitest timeout here is a generous outer backstop.
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
    include: ["test/corpusGate.lane.ts"],
    exclude: ["**/node_modules/**", "**/dist/**"],
    fileParallelism: false,
    maxWorkers: 1,
    testTimeout: 14_400_000,
    hookTimeout: 600_000,
  },
});
