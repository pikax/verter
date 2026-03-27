import { defineConfig } from "vite";
import vue from "@verter/unplugin/vite";
import path from "path";

export default defineConfig({
  root: path.resolve(__dirname, "../../app"),
  plugins: [vue()],
  resolve: {
    alias: {
      vue: path.resolve(
        __dirname,
        "../../../node_modules/vue/dist/vue.runtime-with-vapor.esm-browser.js",
      ),
    },
  },
  server: {
    port: 3101,
    strictPort: true,
  },
  preview: {
    port: 4101,
    strictPort: true,
  },
  build: {
    outDir: path.resolve(__dirname, "dist"),
    emptyOutDir: true,
    minify: false,
  },
  css: {
    preprocessorOptions: {
      scss: { api: "modern-compiler" },
    },
  },
});
