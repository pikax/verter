import { defineConfig } from "vitest/config";
import { resolve } from "path";

export default defineConfig({
  resolve: {
    alias: {
      "@": resolve(__dirname, "src"),
      "verter-wasm-glue": resolve(__dirname, "../wasm/wasm/verter_wasm.js"),
      // Resolve the descriptor-generated client framework manifest directly from
      // language-shared source, so the playground never couples to a built dist.
      "@verter/language-shared": resolve(__dirname, "../language-shared/src/index.ts"),
    },
  },
  test: {
    include: ["src/**/*.spec.ts"],
  },
});
