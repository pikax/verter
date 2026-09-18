import { defineConfig } from "vite";

export default defineConfig({
  resolve: {
    alias: {
      "@ui": "/src/ui",
    },
  },
});
