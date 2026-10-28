import { defineConfig } from "tsdown";

export default defineConfig({
  entry: ["src/index.ts"],
  format: ["cjs", "esm"],
  outDir: "dist",
  deps: {
    dts: {
      alwaysBundle: [/^@verter\/native(\/.*)?$/],
    },
  },
  dts: {
    // The declarations bundle @verter/native sources reached through
    // node_modules. With TypeScript 7.1 installed, rolldown-plugin-dts would
    // pick its tsgo generator, which emits no declaration for a file reached
    // that way; the tsc generator emits them, as it did before.
    generator: "tsc",
    tsc: {
      eager: true,
    },
  },
});
