import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    include: ["src/__benchmarks__/**/*.bench.ts"],
    benchmark: {
      include: ["src/__benchmarks__/**/*.bench.ts"],
      exclude: ["node_modules", "dist"],
    },
  },
});
