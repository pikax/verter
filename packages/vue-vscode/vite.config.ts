import { defineConfig } from "vite";

export default defineConfig({
  build: {
    lib: {
      entry: "./src/extension.ts",
      fileName: "extension",
      formats: ["cjs"],
    },
    sourcemap: true,

    rollupOptions: {
      external: ["path", "vscode", "vscode-languageclient/node", "lodash"],
    },
  },
});
