import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    globals: false,
    include: ["test/**/*.test.ts"],
    exclude: ["**/node_modules/**", "**/dist/**"],
    // Tests spawn a hermetic Node-based fake LSP server over stdio; a bounded
    // timeout guards against a stuck child process keeping the suite alive.
    testTimeout: 20000,
  },
});
