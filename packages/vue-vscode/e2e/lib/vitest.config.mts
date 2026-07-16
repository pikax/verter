import { dirname } from "node:path";
import { fileURLToPath } from "node:url";
import { defineConfig } from "vitest/config";

/** Hermetic unit gate for pure e2e/lib helpers (no VS Code host). */
export default defineConfig({
  test: {
    root: dirname(fileURLToPath(import.meta.url)),
    include: ["**/*.unit.test.ts"],
    exclude: ["**/node_modules/**", "**/dist/**", "**/out-test/**"],
    globals: false,
    testTimeout: 20_000,
  },
});
