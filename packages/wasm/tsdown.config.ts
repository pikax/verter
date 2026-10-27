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
    tsc: {
      eager: true,
    },
  },
});
