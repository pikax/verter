import { fileURLToPath } from "node:url";

import { defineConfig } from "vitest/config";

// Native TypeScript reference lane: drives the REAL provider binaries (tsgo /
// tsserver) DIRECTLY against an EXTERNAL corpus (`VERTER_CORPUS_GATE_DIR`) on
// plain `.ts`/`.tsx` files — no Verter process in the loop. Deliberately
// EXCLUDED from the default hermetic `pnpm test` run (the lane file is not
// named `*.test.ts`); invoked via
// `pnpm --filter @verter/dx-harness test:native-reference`. Without the env
// the lane records an honest explicit skip. Serialized: one worker, one file.
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
    include: ["test/nativeReference.lane.ts"],
    exclude: ["**/node_modules/**", "**/dist/**"],
    fileParallelism: false,
    maxWorkers: 1,
    testTimeout: 7_800_000,
    hookTimeout: 600_000,
  },
});
