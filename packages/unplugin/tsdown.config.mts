import { defineConfig } from "tsdown";

// This file is `.mts` (ESM by extension) rather than `.ts` so tsdown loads it as
// an ES module WITHOUT requiring `"type": "module"` in package.json. The package
// stays CommonJS-by-default, which keeps the CJS e2e bundler configs
// (`e2e/bundlers/{webpack,rspack}/*.config.js`, which use `require`/`module.exports`)
// working unchanged. tsdown's config loader recognises the `.mts` extension.
//
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
    "src/vue.ts",
    "src/sveltejs.ts",
  ],
  format: ["cjs", "esm"],
  dts: true,
  unbundle: true,
  outputOptions: {
    exports: "named",
  },
});
