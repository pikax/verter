import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // Runtime specs (`src/**/*.spec.ts`) run as usual; type-level wire-contract
    // pins live in `src/**/*.test-d.ts` and are checked by the tsc checker
    // (same pattern as `packages/types`). `tsconfig.test.json` keeps the
    // test-d files out of the `tsc -b` build emit.
    typecheck: {
      enabled: true,
      include: ["src/**/*.test-d.ts"],
      checker: "tsc",
      tsconfig: "./tsconfig.test.json",
      ignoreSourceErrors: false,
    },
  },
});
