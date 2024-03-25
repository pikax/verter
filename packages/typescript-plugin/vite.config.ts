import { defineConfig } from "vite";
export default defineConfig({
  build: {
    lib: {
      entry: "./src/index.ts",
      //   fileName: "index",
      formats: ["es", "cjs"],
    },
    sourcemap: true,
    rollupOptions: {
      external: [
        "node:path",
        "node:fs",

        // @verter/core

        // "typescript",
        // "source-map-js",
        // "glob",
        // "@vue/shared",
        // "@vue/compiler-core",
        // "@vue/compiler-sfc",
        "@types/ts-expose-internals",
        // "@babel/parser",
        // "@babel/types",
      ],
    },
  },
});

// import { defineConfig } from "vite";

// export default defineConfig({
//   build: {
//     lib: {
//       entry: "./src/extension.ts",
//       fileName: "extension",
//       formats: ["cjs"],
//     },
//     sourcemap: true,

//     rollupOptions: {
//       external: ["path", "vscode", "vscode-languageclient/node", "lodash"],
//     },
//   },
// });
