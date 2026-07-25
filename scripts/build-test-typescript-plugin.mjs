#!/usr/bin/env node
// build-test-typescript-plugin.mjs
//
// SINGLE shared builder for the source-built `@verter/typescript-plugin` test
// fixture consumed by the Rust real-provider recovery tests
// (`verter_type_runtime` resilient/ipc recovery suites and the `verter_lsp`
// lazy-managed recovery suite). Both call sites used to shell out to
// `node node_modules/esbuild/bin/esbuild ...`; that bin is the platform NATIVE
// executable (Mach-O/ELF/PE), so node parses its header as JavaScript and the
// harness dies with `SyntaxError: Invalid or unexpected token`.
//
// This helper never executes the native bin: it drives esbuild's JavaScript API
// (`import { build } from "esbuild"`) resolved from the WORKSPACE install
// (Node walks up from this script into the repo-root `node_modules` — no global
// install, no shell, no quoting).
//
// Usage:
//   node scripts/build-test-typescript-plugin.mjs <absolute-outfile>
//
// Cross-platform: `node:path` joins only, no path-separator literals, no
// per-OS binary names, no POSIX-only APIs. Missing prerequisites (workspace
// esbuild, plugin entry source, language-shared alias source) FAIL CLOSED with
// an explicit message — never a silent skip.

import { existsSync } from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const repoRoot = path.resolve(path.dirname(fileURLToPath(import.meta.url)), "..");

function fail(message) {
  console.error(`build-test-typescript-plugin: ${message}`);
  process.exit(1);
}

const outfile = process.argv[2];
if (!outfile) {
  fail("missing required argument: absolute path of the plugin entry to emit");
}

const entryPoint = path.join(repoRoot, "packages", "typescript-plugin", "src", "index.ts");
if (!existsSync(entryPoint)) {
  fail(`workspace typescript-plugin entry source not found at ${entryPoint}`);
}

const languageSharedEntry = path.join(repoRoot, "packages", "language-shared", "src", "index.ts");
if (!existsSync(languageSharedEntry)) {
  fail(`workspace language-shared entry source not found at ${languageSharedEntry}`);
}

let build;
try {
  ({ build } = await import("esbuild"));
} catch (error) {
  fail(
    `workspace esbuild package is not resolvable from ${repoRoot} — ` +
      `run \`pnpm install --frozen-lockfile\` at the repo root (${error.message})`,
  );
}

try {
  await build({
    entryPoints: [entryPoint],
    bundle: true,
    platform: "node",
    format: "cjs",
    target: "node18",
    alias: {
      "@verter/language-shared": languageSharedEntry,
    },
    outfile: path.resolve(outfile),
    logLevel: "silent",
  });
} catch (error) {
  fail(`esbuild build failed: ${error.message}`);
}

const resolvedOutfile = path.resolve(outfile);
if (!existsSync(resolvedOutfile)) {
  fail(`esbuild reported success but emitted no entry at ${resolvedOutfile}`);
}
