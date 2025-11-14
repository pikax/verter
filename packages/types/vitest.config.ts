import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    globals: false,
    typecheck: {
      enabled: true,
      only: true,
      include: ["**/*.spec.ts"],
      checker: "tsc",
      tsconfig: "./tsconfig.test.json",
      ignoreSourceErrors: false,
    },
  },
});
