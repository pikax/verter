import { fileURLToPath } from "node:url";

import { defineConfig } from "vitest/config";

export default defineConfig({
  resolve: {
    alias: {
      // Resolve the sibling @verter/lsp-test-client to its TypeScript source for
      // tests. The package's runtime entry is its gitignored `dist` (built in CI
      // for the benchmark RUN), which is absent on a fresh checkout — without
      // this alias `pnpm --filter @verter/benchmark test` (and root `pnpm test`)
      // would fail to resolve the import. Aliasing to `src` lets Vitest transform
      // the source directly, so the test gate needs no prior build step. Only the
      // bare package specifier is imported, so an exact-match alias suffices and
      // leaves the runtime bench (tsx → dist) untouched.
      "@verter/lsp-test-client": fileURLToPath(
        new URL("../lsp-test-client/src/index.ts", import.meta.url),
      ),
    },
  },
  test: {
    globals: false,
    exclude: ["**/node_modules/**", "**/dist/**"],
    // The LSP specs spawn a hermetic Node-based fake server over stdio; a bounded
    // timeout guards against a stuck child process keeping the suite alive.
    testTimeout: 20000,
  },
});
