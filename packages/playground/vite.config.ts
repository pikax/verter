import { defineConfig } from "vite";
import verter from "@verter/unplugin/vite";
import vue from "@vitejs/plugin-vue";
import { resolve } from "path";
import { readFileSync } from "fs";

const pkg = JSON.parse(readFileSync(resolve(__dirname, "../wasm/package.json"), "utf8"));

export default defineConfig({
  define: {
    __VERTER_VERSION__: JSON.stringify(pkg.version),
  },
  plugins: [
    verter(),
    // vue(),
  ],
  resolve: {
    alias: {
      "@": resolve(__dirname, "src"),
      "verter-wasm-glue": resolve(__dirname, "../wasm/wasm/verter_wasm.js"),
      // Resolve the descriptor-generated client framework manifest directly from
      // language-shared source, so the playground never couples to a built dist.
      "@verter/language-shared": resolve(__dirname, "../language-shared/src/index.ts"),
    },
  },
  optimizeDeps: {
    exclude: ["@verter/wasm"],
  },
  server: {
    fs: {
      allow: ["..", "../../node_modules"],
    },
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
  },
  preview: {
    headers: {
      "Cross-Origin-Opener-Policy": "same-origin",
      "Cross-Origin-Embedder-Policy": "require-corp",
    },
  },
  build: {
    target: "esnext",
    minify: true,
    rolldownOptions: {
      output: {
        format: "es",
        // entryFileNames: "[name].js",
        // chunkFileNames: "[name].js",
        // manualChunks: (id) => {
        //   // Separate each .vue file into its own chunk
        //   if (id.includes(".vue")) {
        //     const match = id.match(/([^/\\]+)\.vue$/);
        //     if (match) {
        //       return match[1];
        //     }
        //   }
        // },

        manualChunks: (id) => {
          if (id.includes("monaco-editor-core")) return "monaco";
          if (id.includes("shiki") || id.includes("@shikijs/monaco")) return "shiki";
        },
      },
    },
  },
  worker: {
    format: "es",
  },
  assetsInclude: ["**/*.wasm"],
});
