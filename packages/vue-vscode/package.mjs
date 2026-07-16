#!/usr/bin/env node
/**
 * VSIX packaging script.
 *
 * vsce with `dependencies: false` hard-excludes node_modules/ from the VSIX.
 * But VS Code's TS server resolves typescriptServerPlugins from
 * <extensionPath>/node_modules/<name>, so we need it included.
 *
 * This script:
 * 1. Materializes the complete production dependency graph as real files
 *    (replaces pnpm symlinks, which npm otherwise treats as workspace roots)
 * 2. Patches workspace ranges to the release version so npm list succeeds
 * 3. Temporarily sets vsce.dependencies = true in package.json
 * 4. Runs vsce package (which triggers esbuild.mjs via vscode:prepublish)
 * 5. Restores package.json
 *
 * Usage:
 *   node package.mjs --target win32-x64      # platform-specific VSIX (production)
 *   node package.mjs --allow-universal       # universal VSIX (DEV ONLY — stages the host binary)
 *
 * Production packaging REQUIRES `--target`: a targetless VSIX has no platform target and
 * would install anywhere while carrying the host-arch shim. A bare `node package.mjs`
 * (no `--target`, no `--allow-universal`) fails closed during shim staging.
 */
import { readFileSync, writeFileSync } from "fs";
import { execSync } from "child_process";
import path from "path";
import { fileURLToPath } from "url";
import { stageShimBinary } from "./stage-bin.mjs";
import { patchWorkspaceRanges, stageRuntimeDependencies } from "./stage-deps.mjs";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const pkgPath = path.join(__dirname, "package.json");

/** Parse the VSCE `--target` (space- or `=`-separated) from the packaging argv. */
function parseTarget(argv) {
  for (let i = 0; i < argv.length; i++) {
    const arg = argv[i];
    if (arg === "--target") return argv[i + 1];
    if (arg.startsWith("--target=")) return arg.slice("--target=".length);
  }
  return undefined;
}

// --- Read original package.json ---
const originalPkgText = readFileSync(pkgPath, "utf8");
const pkg = JSON.parse(originalPkgText);
const version = pkg.version;

// --- Step 0: Stage the verter-relay-shim binary into bin/ (fail-closed) FIRST ---
// The editor runs the bundled shim as its `tsgo`; the shim spawns the REAL tsgo and relays
// `--lsp` stdio. Only the Verter shim ships in the VSIX — never tsgo.
// This runs BEFORE the node_modules / package.json mutations below, so a missing binary fails
// closed HERE without leaving node_modules symlinks replaced or package.json patched.
// NOTE: CI supplies VERTER_RELAY_SHIM_BINARY pointing at the per-target shim it cross-compiled,
// so this stages that exact artifact; locally the binary is resolved from
// target/<rust-target>/{release,debug} (or the host target/ for a universal build). Packaging
// NEVER auto-builds — a missing binary fails closed.
const packagingArgv = process.argv.slice(2);
const vsceTarget = parseTarget(packagingArgv);
// A universal (targetless) build stages the HOST binary and installs anywhere — DEV ONLY,
// gated behind an explicit `--allow-universal`. Production requires `--target`; staging
// fails closed otherwise. Production ships the release profile only; the dev debug-profile
// fallback is never enabled here.
const allowUniversal = packagingArgv.includes("--allow-universal");
const repoRoot = path.resolve(__dirname, "..", "..");
const stagedShim = stageShimBinary({
  vsceTarget,
  allowUniversal,
  repoRoot,
  extensionDir: __dirname,
});
console.log(`Staged ${stagedShim.basename} -> ${path.relative(__dirname, stagedShim.dest)}`);

// --- Step 1: Materialize an npm-compatible production dependency tree ---
console.log("Preparing production node_modules/ for VSIX packaging...");
stageRuntimeDependencies({
  packageDir: __dirname,
  workspaceRoot: repoRoot,
  destinationNodeModules: path.join(__dirname, "node_modules"),
  packageVersion: version,
});
console.log("Production dependency graph materialized as real package files");

// --- Step 2: Patch package.json for vsce ---
pkg.vsce.dependencies = true;
patchWorkspaceRanges(pkg, version);

writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
console.log("package.json patched for vsce (dependencies: true, resolved versions)");

// --- Step 3: Run vsce package ---
try {
  // `--allow-universal` is a Verter packaging flag, not a vsce flag — strip it before
  // forwarding the argv to vsce (which would reject an unknown option).
  const args = packagingArgv.filter((a) => a !== "--allow-universal").join(" ");
  execSync(`npx @vscode/vsce package ${args}`, { stdio: "inherit", cwd: __dirname });
} finally {
  // --- Step 4: Restore package.json ---
  writeFileSync(pkgPath, originalPkgText);
  console.log("package.json restored");

  // --- Step 5: Restore pnpm symlinks (overwritten by dep copying) ---
  try {
    execSync("pnpm install", { stdio: "inherit", cwd: __dirname });
  } catch {
    console.warn("Warning: pnpm install failed — run it manually to restore symlinks");
  }
}
