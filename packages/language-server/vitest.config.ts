import { defineConfig } from "vitest/config";

export default defineConfig({
  define: {
    "import.meta.vitest": "undefined",
  },
  test: {
    exclude: ["**/node_modules/**", "**/dist/**"],
    includeSource: ["src/**/*.{js,ts}"],
    globals: true,
  },
});
