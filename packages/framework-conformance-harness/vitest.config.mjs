import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // ONLY this harness's own self-tests. The pinned official checkouts live
    // under .oracle-checkouts/ and carry the upstream projects' own thousands of
    // spec files; they are DATA for this harness, never tests to run here.
    include: ["test/**/*.spec.mjs"],
    exclude: [".oracle-checkouts/**", "node_modules/**", "goldens/**", "fixtures/**"],
    // Practically every case here spawns a real child process, realizes an
    // isolated install, or drives a pinned official compiler/runtime, so
    // vitest's 5s default measures machine load rather than harness
    // behaviour: under parallel-worker contention it fails cases that assert
    // nothing about duration. A case that genuinely hangs still fails, just
    // at a bound that only a hang can reach.
    testTimeout: 120_000,
    hookTimeout: 120_000,
  },
});
