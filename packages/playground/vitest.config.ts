import { defineConfig } from "vitest/config";
import { resolve } from "path";

export default defineConfig({
  resolve: {
    alias: {
      "@": resolve(__dirname, "src"),
      "verter-wasm-glue": resolve(__dirname, "../wasm/wasm/verter_wasm.js"),
    },
  },
  test: {
    include: ["src/**/*.spec.ts"],
  },
});
