#!/usr/bin/env node
/**
 * VSIX packaging script.
 *
 * vsce with `dependencies: false` hard-excludes node_modules/ from the VSIX.
 * But VS Code's TS server resolves typescriptServerPlugins from
 * <extensionPath>/node_modules/<name>, so we need it included.
 *
 * This script:
 * 1. Copies workspace deps to node_modules/ as real files (replaces pnpm symlinks)
 * 2. Patches workspace:^ versions so npm list succeeds
 * 3. Temporarily sets vsce.dependencies = true in package.json
 * 4. Runs vsce package (which triggers esbuild.mjs via vscode:prepublish)
 * 5. Restores package.json
 *
 * Usage:
 *   node package.mjs                         # universal VSIX
 *   node package.mjs --target win32-x64      # platform-specific VSIX
 */
import {
  readFileSync,
  writeFileSync,
  cpSync,
  mkdirSync,
  existsSync,
  readdirSync,
  lstatSync,
  rmSync,
} from "fs";
import { execSync } from "child_process";
import path from "path";
import { fileURLToPath } from "url";

const __dirname = path.dirname(fileURLToPath(import.meta.url));
const pkgPath = path.join(__dirname, "package.json");

// --- Read original package.json ---
const originalPkgText = readFileSync(pkgPath, "utf8");
const pkg = JSON.parse(originalPkgText);
const version = pkg.version;

// --- Step 1: Copy workspace deps to node_modules/ with patched versions ---
console.log("Preparing node_modules/ for VSIX packaging...");

const nmVerter = path.join(__dirname, "node_modules", "@verter");

// @verter/typescript-plugin
const tsPluginSrc = path.resolve(__dirname, "..", "typescript-plugin");
const tsPluginDst = path.join(nmVerter, "typescript-plugin");
removeSafe(tsPluginDst);
mkdirSync(tsPluginDst, { recursive: true });

// Copy + patch package.json (replace workspace:^ with real version)
const tsPluginPkg = JSON.parse(readFileSync(path.join(tsPluginSrc, "package.json"), "utf8"));
patchWorkspaceDeps(tsPluginPkg, version);
writeFileSync(path.join(tsPluginDst, "package.json"), JSON.stringify(tsPluginPkg, null, 2) + "\n");

if (existsSync(path.join(tsPluginSrc, "dist"))) {
  cpSync(path.join(tsPluginSrc, "dist"), path.join(tsPluginDst, "dist"), { recursive: true });
}

// @verter/native
const nativeSrc = path.resolve(__dirname, "..", "native");
const nativeDst = path.join(nmVerter, "native");
removeSafe(nativeDst);
mkdirSync(path.join(nativeDst, "dist"), { recursive: true });

const nativePkg = JSON.parse(readFileSync(path.join(nativeSrc, "package.json"), "utf8"));
patchWorkspaceDeps(nativePkg, version);
writeFileSync(path.join(nativeDst, "package.json"), JSON.stringify(nativePkg, null, 2) + "\n");

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

console.log("Workspace deps copied to node_modules/ with patched versions");

// --- Step 2: Patch package.json for vsce ---
pkg.vsce.dependencies = true;
patchWorkspaceDeps(pkg, version);

writeFileSync(pkgPath, JSON.stringify(pkg, null, 2) + "\n");
console.log("package.json patched for vsce (dependencies: true, resolved versions)");

// --- Step 3: Run vsce package ---
try {
  const args = process.argv.slice(2).join(" ");
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

// --- Helpers ---

/** Replace all workspace:^ dependency values with a real version. */
function patchWorkspaceDeps(pkgJson, ver) {
  for (const field of [
    "dependencies",
    "devDependencies",
    "peerDependencies",
    "optionalDependencies",
  ]) {
    const deps = pkgJson[field];
    if (!deps) continue;
    for (const [name, value] of Object.entries(deps)) {
      if (typeof value === "string" && value.startsWith("workspace:")) {
        deps[name] = ver;
      }
    }
  }
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
