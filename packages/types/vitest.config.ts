import { defineConfig } from "vitest/config";

export default defineConfig({
  // Vitest 4 uses OXC for import analysis. Tell it to transform JSX so it can
  // parse .tsx files. The actual type checking uses tsconfig's jsx: "preserve"
  // via the tsc checker — this only affects Vite's internal module graph.
  oxc: {
    jsx: "automatic",
  },
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
