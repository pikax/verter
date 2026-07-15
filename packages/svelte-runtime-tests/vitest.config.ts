import { defineConfig } from "vitest/config";

export default defineConfig({
  // Svelte exposes mount/flushSync only through its browser condition. Keep
  // that condition isolated to this behavioral package so Node-only suites
  // elsewhere in the workspace retain their normal dependency resolution.
  resolve: {
    conditions: ["browser"],
  },
  test: {
    globals: false,
    exclude: ["**/node_modules/**", "**/dist/**"],
  },
});
