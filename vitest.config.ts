import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    globals: true,
    exclude: [
      "**/node_modules/**",
      "**/dist/**",
      "**/e2e/**",
      "**/.claude/worktrees/**",
      "packages/playground/**",
    ],
    coverage: {
      provider: "v8",
      reporter: ["text", "lcov"],
      include: ["packages/*/src/**"],
      exclude: [
        "**/node_modules/**",
        "**/dist/**",
        "**/e2e/**",
        "**/*.spec.ts",
        "**/*.test.ts",
        "**/__tests__/**",
      ],
    },
  },
});
