// The SHIPPED extension bundle's esbuild configuration.
//
// Split out of `esbuild.mjs` (which has build-time side effects: staging deps
// and the LSP binary) so the artifact guard can bundle the SAME entry point and
// the SAME graph the VSIX ships, instead of a hand-copied approximation that
// could drift away from what users install.

import { fileURLToPath } from "node:url";
import path from "node:path";

const packageDir = path.dirname(fileURLToPath(import.meta.url));

/** The production entry point — the module that becomes `dist/extension.js`. */
export const PRODUCTION_ENTRY_POINT = path.join(packageDir, "src", "extension.ts");

/**
 * The shipped bundle's esbuild options.
 *
 * @param {{ production?: boolean, sourcemap?: boolean }} [options]
 * @returns {import('esbuild').BuildOptions}
 */
export function productionBundleConfig(options = {}) {
  const production = options.production ?? false;
  return {
    entryPoints: [PRODUCTION_ENTRY_POINT],
    bundle: true,
    outfile: path.join(packageDir, "dist", "extension.js"),
    // `verter-mcp` (the standalone MCP server's launcher) must resolve at
    // RUNTIME: its binary discovery derives the workspace root from its own
    // on-disk location, which inlining into the bundle would falsify.
    external: ["vscode", "@verter/typescript-plugin", "verter-mcp"],
    format: "cjs",
    platform: "node",
    target: "node18",
    mainFields: ["module", "main"],
    sourcemap: options.sourcemap ?? !production,
    minify: production,
    treeShaking: true,
  };
}
