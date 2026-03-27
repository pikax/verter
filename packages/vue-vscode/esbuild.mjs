import esbuild from "esbuild";
import { fileURLToPath } from "url";
import { cpSync, mkdirSync, existsSync, readdirSync, lstatSync, rmSync } from "fs";
import path from "path";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const production = process.argv.includes("--production");
const watch = process.argv.includes("--watch");
const monorepoRoot = path.resolve(__dirname, "..", "..");

/**
 * Copy workspace dependencies to dist/node_modules/ for the bundled extension.
 *
 * The bundled extension.js has `require("@verter/typescript-plugin")` as external.
 * Node resolves it by walking up from dist/ → finds dist/node_modules/@verter/...
 *
 * This does NOT touch root node_modules/ (those are pnpm-managed symlinks).
 * For VSIX packaging, package.mjs handles copying to root node_modules/.
 */
function prepareDeps() {
  const dst = path.join(__dirname, "dist", "node_modules", "@verter");

  // @verter/typescript-plugin
  const tsPluginSrc = path.resolve(__dirname, "..", "typescript-plugin");
  const tsPluginDst = path.join(dst, "typescript-plugin");
  removeSafe(tsPluginDst);
  mkdirSync(tsPluginDst, { recursive: true });
  cpSync(path.join(tsPluginSrc, "package.json"), path.join(tsPluginDst, "package.json"));
  if (existsSync(path.join(tsPluginSrc, "dist"))) {
    cpSync(path.join(tsPluginSrc, "dist"), path.join(tsPluginDst, "dist"), { recursive: true });
  }

  // @verter/native — copy selectively (only current platform .node files)
  const nativeSrc = path.resolve(__dirname, "..", "native");
  const nativeDst = path.join(dst, "native");
  removeSafe(nativeDst);
  mkdirSync(path.join(nativeDst, "dist"), { recursive: true });
  cpSync(path.join(nativeSrc, "package.json"), path.join(nativeDst, "package.json"));
  if (existsSync(path.join(nativeSrc, "index.js"))) {
    cpSync(path.join(nativeSrc, "index.js"), path.join(nativeDst, "index.js"));
  }
  if (existsSync(path.join(nativeSrc, "dist"))) {
    for (const file of readdirSync(path.join(nativeSrc, "dist"))) {
      if (file.endsWith(".node") && !file.endsWith(".old")) {
        cpSync(path.join(nativeSrc, "dist", file), path.join(nativeDst, "dist", file));
      }
    }
  }

  console.log("Workspace deps copied to dist/node_modules/");
}

/** Remove a path that might be a symlink or real directory. */
function removeSafe(p) {
  if (!existsSync(p) && !lstatSafe(p)) return;
  try {
    const stat = lstatSync(p);
    if (stat.isSymbolicLink()) {
      rmSync(p);
    } else {
      rmSync(p, { recursive: true });
    }
  } catch {}
}

function lstatSafe(p) {
  try {
    return lstatSync(p);
  } catch {
    return null;
  }
}

/**
 * Copy the LSP binary into bin/ so it gets bundled in the VSIX.
 *
 * Search order: target/release/ then target/debug/.
 * In CI, the binary is placed here by the workflow before vsce package runs.
 * Locally, this picks up whatever you last built with cargo build.
 */
function prepareLspBinary() {
  const ext = process.platform === "win32" ? ".exe" : "";
  const binName = `verter-lsp${ext}`;
  const binDir = path.join(__dirname, "bin");
  const binDst = path.join(binDir, binName);

  // Already placed by CI or previous run
  if (existsSync(binDst)) {
    console.log(`LSP binary already at bin/${binName}`);
    return;
  }

  // Find from cargo build output
  for (const profile of ["release", "debug"]) {
    const src = path.join(monorepoRoot, "target", profile, binName);
    if (existsSync(src)) {
      mkdirSync(binDir, { recursive: true });
      cpSync(src, binDst);
      console.log(`LSP binary copied from target/${profile}/${binName}`);
      return;
    }
  }

  console.warn(`Warning: LSP binary not found — run 'cargo build --release -p verter_lsp' first`);
}

/** @type {import('esbuild').BuildOptions} */
const config = {
  entryPoints: [path.join(__dirname, "src", "extension.ts")],
  bundle: true,
  outfile: path.join(__dirname, "dist", "extension.js"),
  external: ["vscode", "@verter/typescript-plugin"],
  format: "cjs",
  platform: "node",
  target: "node18",
  mainFields: ["module", "main"],
  sourcemap: !production,
  minify: production,
  treeShaking: true,
};

if (!watch) {
  prepareDeps();
  prepareLspBinary();
}

if (watch) {
  const ctx = await esbuild.context(config);
  await ctx.watch();
  console.log("Watching for changes...");
} else {
  await esbuild.build(config);
  console.log("Build complete");
}
