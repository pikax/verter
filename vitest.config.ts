import { defineConfig } from "vitest/config";

export default defineConfig({
  // NOTE: the `browser` export condition (needed so `svelte` resolves its CLIENT
  // build — `mount`/`flushSync` — under the DOM test environment) is scoped to
  // `packages/core/vitest.config.ts`, where the Svelte client smoke runs, NOT the
  // repo root: a root-level condition changes dependency resolution for every
  // package's tests (blast radius). Keep DOM-resolution conditions per-package.
  test: {
    globals: true,
    exclude: [
      "**/node_modules/**",
      "**/dist/**",
      "**/e2e/**",
      "**/.claude/worktrees/**",
      "**/tmp/**",
      "tmp/**",
      ".integration-tests/**",
      "packages/playground/**",
      // packages/types has its own vitest.config.ts with typecheck: { only: true }
      // — these are type-level tests that cannot run as runtime tests from the root config
      "packages/types/**",
    ],
    coverage: {
      provider: "v8",
      reporter: ["text", "lcov"],
      include: ["packages/*/src/**"],
      exclude: [
        "**/node_modules/**",
        "**/dist/**",
        "**/e2e/**",
        "**/tmp/**",
        "**/*.spec.ts",
        "**/*.test.ts",
        "**/__tests__/**",
      ],
    },
  },
});
