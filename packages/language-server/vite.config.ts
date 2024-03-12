import { defineConfig } from "vite";

export default defineConfig({
  build: {
    lib: {
      entry: "./src/server.ts",
      fileName: "server",
      formats: ["cjs"],
    },
    sourcemap: true,

    rollupOptions: {
      external: [
        "vscode-languageserver",
        "vscode-languageserver/node.js",
        "vscode-languageserver/node",
        "vscode-languageserver-protocol",
        "vscode-languageserver/lib/node/main.js",
        "vscode-languageserver-textdocument",

        'fs/promises',

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
});
