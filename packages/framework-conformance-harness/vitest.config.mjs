import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // ONLY this harness's own self-tests. The pinned official checkouts live
    // under .oracle-checkouts/ and carry the upstream projects' own thousands of
    // spec files; they are DATA for this harness, never tests to run here.
    include: ["test/**/*.spec.mjs"],
    exclude: [".oracle-checkouts/**", "node_modules/**", "goldens/**", "fixtures/**"],
  },
});
