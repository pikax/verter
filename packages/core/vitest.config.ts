import { defineConfig } from "vitest/config";

export default defineConfig({
  // Prefer the `browser` export condition so a package shipping separate
  // server/client builds behind export conditions (notably `svelte`, whose
  // `mount` / `flushSync` live ONLY in the client build) resolves the CLIENT
  // entry under the DOM (happy-dom) environment the Svelte client smoke uses.
  // Scoped to THIS package's standalone config (NOT the repo root) so it does
  // not change dependency resolution for unrelated Node-only test packages.
  resolve: {
    conditions: ["browser"],
  },
  test: {
    // ...
    globals: true,
    benchmark: {},
    exclude: ["**/node_modules/**", "**/dist/**"],
  },
});
