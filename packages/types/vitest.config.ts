import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    globals: false,
    passWithNoTests: true,
    typecheck: {
      enabled: true,
      only: true,
      include: ["**/*.spec.ts", "**/*.spec.tsx"],
      checker: "tsc",
      tsconfig: "./tsconfig.test.json",
      ignoreSourceErrors: false,
    },
  },
});
