import { defineConfig } from "vitest/config";

export default defineConfig({
  test: {
    // ...
    globals: true,
    benchmark: {},
    exclude: ["**/node_modules/**", "**/dist/**"],
  },
});
