import { fileURLToPath } from "node:url";

import { defineConfig } from "vitest/config";

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
    include: ["test/editorNeutralLsp.contract.ts"],
    exclude: ["**/node_modules/**", "**/dist/**"],
    fileParallelism: false,
    maxWorkers: 1,
    testTimeout: 60_000,
    hookTimeout: 180_000,
  },
});
