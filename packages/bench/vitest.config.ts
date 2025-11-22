import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // ...
    globals: true,
    benchmark: {},
    maxConcurrency: process.platform === "win32" ? 1 : undefined,
    // testTimeout: 200000,
  },
});
