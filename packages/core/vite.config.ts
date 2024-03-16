import { defineConfig } from "vite";
import dts from 'vite-plugin-dts'

export default defineConfig({
  build: {
    lib: {
      entry: "./src/index.ts",
      fileName: "index",
      formats: ["es", "cjs"],
    },
    sourcemap: true,
    rollupOptions: {
      external: [
        "typescript",
        "source-map-js",
        "glob",
        "@vue/shared",
        "@vue/compiler-core",
        "@vue/compiler-sfc",
        "@types/ts-expose-internals",
        "@babel/parser",
        "@babel/types",
      ],
    },
  },
  plugins: [
    dts({
      // rollupTypes: true
    })
  ]
});
