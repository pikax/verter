import { fileURLToPath } from "node:url";

import { defineConfig } from "vitest/config";

export default defineConfig({
  resolve: {
    alias: {
      // Resolve the sibling workspace dependency to its SOURCE so the suite runs
      // on a clean checkout without a prior `dist` build of @verter/lsp-test-client
      // (mirrors how that package runs its own tests against src).
      "@verter/lsp-test-client": fileURLToPath(
        new URL("../lsp-test-client/src/index.ts", import.meta.url),
      ),
    },
  },
  test: {
    globals: false,
    include: ["test/**/*.test.ts"],
    exclude: [
      "**/node_modules/**",
      "**/dist/**",
      // The endurance/soak lane is long-running and spawns the real verter-lsp
      // binary; it runs only via `test:endurance` (vitest.endurance.config.ts).
      "test/endurance.*.test.ts",
    ],
    // Integration tests spawn a child process (a fake bridge/materializer over
    // stdio); a bounded timeout guards against a stuck child keeping the suite
    // alive.
    testTimeout: 20000,
  },
});
