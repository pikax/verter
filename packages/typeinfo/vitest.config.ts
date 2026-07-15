import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    globals: false,
    include: ["tests/**/*.spec.ts"],
    exclude: ["**/node_modules/**", "**/dist/**"],
    // Tests upsert virtual files via VerterHost — they don't need a
    // real workspace, so a low timeout protects against host hangs.
    testTimeout: 30000,
  },
});
