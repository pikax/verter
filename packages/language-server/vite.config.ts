/// <reference types="vitest" />
import { defineConfig } from "vite";

export default defineConfig({
  build: {
    lib: {
      // entry: "./src/old_server.ts",
      entry: "./src/server.ts",
      fileName: "server",
      formats: ["cjs"],
    },
    sourcemap: "inline",

    rollupOptions: {
      external: [
        /node_modules/,
        "vscode-uri",
        "vscode-languageserver",
        "vscode-languageserver/node.js",
        "vscode-languageserver/node",
        "vscode-languageserver-protocol",
        "vscode-languageserver/lib/node/main.js",
        "vscode-languageserver-textdocument",
        "fs/promises",
        "path",
        "node:path",
        "node:fs",
        "fs",

        // deps

        "@verter/core",
      ],
      output: {
        globals: {
          "vscode-languageserver/node": "vscodeLanguageserver/lib/node/main",
          "vscode-languageserver-protocol": "vscodeLanguageserverProtocol",
        },
      },
    },
  },
  define: {
    "import.meta.vitest": "undefined",
  },
  test: {
    includeSource: ["src/**/*.{js,ts}"],
  },
});
