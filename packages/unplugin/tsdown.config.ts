import { defineConfig } from "tsdown";

// One entry per bundler adapter. Each adapter module intentionally exposes a
// named public API (e.g. `unpluginFactory`, `parseVueRequest`) alongside a
// default export (the plugin factory). Rolldown warns MIXED_EXPORTS for that
// combination on the cjs output; `outputOptions.exports: "named"` selects the
// named-export interop while preserving both the named and default exports.
export default defineConfig({
  entry: [
    "src/index.ts",
    "src/vite.ts",
    "src/rollup.ts",
    "src/webpack.ts",
    "src/esbuild.ts",
    "src/rspack.ts",
    "src/rolldown.ts",
    "src/farm.ts",
  ],
  format: ["cjs", "esm"],
  dts: true,
  unbundle: true,
  outputOptions: {
    exports: "named",
  },
});
