/**
 * SSR Baseline Comparison: Vue @vue/compiler-sfc (SSR mode) vs Verter.
 *
 * Compiles .vue files with both compilers, extracts ssrRender function bodies,
 * normalizes and compares them, then generates a report of mismatches.
 *
 * Usage:
 *   node scripts/ssr-baseline/compare.mjs [options]
 *
 * Options:
 *   --root <path>       Root directory to scan (required)
 *   --focus <pattern>   Only process files matching pattern
 *   --limit <n>         Max files to process
 *   --json <path>       Write JSON report to file
 *   --verbose           Show each file result
 *   --errors-only       Only show mismatches/errors
 */

import { createRequire } from "node:module";
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

import { extractSsrRenderBody, normalizeForComparison, extractImports } from "./normalize.mjs";
import { detectPattern, printSummary, writeJsonReport } from "./report.mjs";

const require = createRequire(import.meta.url);
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "../..");

// ── Load compilers ──────────────────────────────────────────────

const { parse, compileScript, compileTemplate } = require("@vue/compiler-sfc");
const { VerterHost } = require(path.join(ROOT, "packages/native/index.js"));

// ── CLI args ────────────────────────────────────────────────────

const args = process.argv.slice(2);

function getArg(name) {
  const idx = args.indexOf(name);
  return idx !== -1 ? args[idx + 1] : null;
}

const rootDir = getArg("--root");
if (!rootDir) {
  console.error("Missing --root argument. Usage: node compare.mjs --root /path/to/projects");
  process.exit(1);
}
const focusPattern = getArg("--focus");
const limit = getArg("--limit") ? parseInt(getArg("--limit"), 10) : 0;
const jsonPath = getArg("--json");
const verbose = args.includes("--verbose");
const errorsOnly = args.includes("--errors-only");

// ── Directories to skip ─────────────────────────────────────────

const SKIP_DIRS = new Set([
  "node_modules",
  "dist",
  ".git",
  ".integration-tests",
  ".nuxt",
  ".output",
  ".vitepress",
]);

const SCAN_DIRS = ["personal", "github", "github/verter-test-repos"];

// ── File discovery ──────────────────────────────────────────────

/**
 * Discover .vue files under rootDir.
 * For each subdirectory of scan dirs, try `git ls-files` first, fall back
 * to recursive fs walk.
 */
function discoverVueFiles() {
  const files = [];

  // If rootDir points directly at a project (has .git), scan it directly
  if (fs.existsSync(path.join(rootDir, ".git"))) {
    collectGitFiles(rootDir, files);
    return files;
  }

  for (const scanDir of SCAN_DIRS) {
    const base = path.join(rootDir, scanDir);
    if (!fs.existsSync(base)) continue;

    let entries;
    try {
      entries = fs.readdirSync(base, { withFileTypes: true });
    } catch {
      continue;
    }

    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      if (entry.name.startsWith(".")) continue;
      if (SKIP_DIRS.has(entry.name)) continue;

      const repoPath = path.join(base, entry.name);
      if (fs.existsSync(path.join(repoPath, ".git"))) {
        collectGitFiles(repoPath, files);
      } else {
        collectFsFiles(repoPath, files);
      }
    }
  }

  return files;
}

function collectGitFiles(repoPath, files) {
  try {
    const output = execSync('git ls-files -z "*.vue"', {
      cwd: repoPath,
      encoding: "utf-8",
      timeout: 10000,
    }).trim();
    if (!output) return;

    for (let relFile of output.split("\0")) {
      if (!relFile) continue;
      if (relFile.startsWith('"') && relFile.endsWith('"')) {
        relFile = relFile.slice(1, -1);
      }
      const parts = relFile.split("/");
      if (parts.some((p) => SKIP_DIRS.has(p))) continue;
      const fullPath = path.join(repoPath, relFile);
      if (!fs.existsSync(fullPath)) continue;
      files.push(fullPath);
    }
  } catch {
    // git ls-files failed, try fs walk
    collectFsFiles(repoPath, files);
  }
}

function collectFsFiles(dir, files, depth = 0) {
  if (depth > 10) return; // avoid deeply nested dirs
  let entries;
  try {
    entries = fs.readdirSync(dir, { withFileTypes: true });
  } catch {
    return;
  }
  for (const entry of entries) {
    if (entry.name.startsWith(".")) continue;
    if (SKIP_DIRS.has(entry.name)) continue;

    const fullPath = path.join(dir, entry.name);
    if (entry.isDirectory()) {
      collectFsFiles(fullPath, files, depth + 1);
    } else if (entry.isFile() && entry.name.endsWith(".vue")) {
      files.push(fullPath);
    }
  }
}

// ── Vue SSR compilation ─────────────────────────────────────────

function compileWithVue(source, filename) {
  try {
    const { descriptor, errors: parseErrors } = parse(source, { filename });

    if (parseErrors.length > 0) {
      return { error: parseErrors[0].message };
    }

    if (!descriptor.template) {
      return { error: "no template" };
    }

    let bindingMetadata = {};
    if (descriptor.script || descriptor.scriptSetup) {
      try {
        const scriptResult = compileScript(descriptor, {
          id: filename,
          inlineTemplate: false,
        });
        bindingMetadata = scriptResult.bindings || {};
      } catch {
        // proceed without bindings
      }
    }

    const result = compileTemplate({
      source: descriptor.template.content,
      filename,
      id: filename,
      ssr: true,
      compilerOptions: {
        mode: "module",
        bindingMetadata,
      },
    });

    if (result.errors && result.errors.length > 0) {
      const msg = result.errors.map((e) => (typeof e === "string" ? e : e.message)).join("; ");
      return { error: msg };
    }

    return { code: result.code };
  } catch (err) {
    return { error: err.message };
  }
}

// ── Verter SSR compilation ──────────────────────────────────────

const host = new VerterHost({ devMode: false, analysisLevel: "none" });

function compileWithVerter(source, filePath) {
  try {
    const filename = path.basename(filePath);
    const upsertResult = host.upsert({
      inputId: filePath,
      source,
      fileKind: "vue",
    });

    const profile = {
      filename,
      ssr: true,
      forceJs: true,
      sourceMap: false,
    };

    const result = host.getVirtualFile({
      canonicalId: upsertResult.canonicalId,
      nodeKind: { kind: "main" },
      compileProfile: profile,
    });

    if (!result || !result.code) {
      return { error: "null response from getVirtualFile" };
    }

    if (result.diagnostics?.hasErrors) {
      const msgs = (result.diagnostics.diagnostics || [])
        .filter((d) => d.severity === "error")
        .map((d) => d.message)
        .join("; ");
      if (msgs) return { error: msgs };
    }

    return { code: result.code };
  } catch (err) {
    return { error: err.message };
  }
}

// ── Main ────────────────────────────────────────────────────────

console.log(`Discovering .vue files under ${rootDir} ...`);
let allFiles = discoverVueFiles();
console.log(`Found ${allFiles.length} .vue files`);

if (focusPattern) {
  allFiles = allFiles.filter((f) => f.replace(/\\/g, "/").includes(focusPattern));
  console.log(`Filtered to ${allFiles.length} files matching "${focusPattern}"`);
}

if (limit > 0 && allFiles.length > limit) {
  allFiles = allFiles.slice(0, limit);
  console.log(`Limited to ${allFiles.length} files`);
}

// Stats
const stats = {
  total: 0,
  matches: 0,
  mismatches: 0,
  vueErrors: 0,
  verterErrors: 0,
  bothErrors: 0,
  noTemplate: 0,
};

const mismatches = [];
const errors = { vue: [], verter: [] };

const startTime = Date.now();
let processed = 0;

for (const filePath of allFiles) {
  processed++;
  stats.total++;

  // Progress indicator every 500 files
  if (processed % 500 === 0) {
    process.stdout.write(`\r  Processing: ${processed}/${allFiles.length} ...`);
  }

  const rel = path.relative(rootDir, filePath).replace(/\\/g, "/");

  let source;
  try {
    source = fs.readFileSync(filePath, "utf-8");
  } catch {
    continue;
  }

  const filename = path.basename(filePath);

  const vueResult = compileWithVue(source, filename);
  const verterResult = compileWithVerter(source, filePath);

  const vueErr = vueResult.error != null;
  const verterErr = verterResult.error != null;

  // Handle "no template" as a skip
  if (vueErr && vueResult.error === "no template") {
    stats.noTemplate++;
    continue;
  }

  if (vueErr && verterErr) {
    stats.bothErrors++;
    if (verbose) console.log(`  [BOTH ERR] ${rel}`);
    continue;
  }

  if (vueErr) {
    stats.vueErrors++;
    errors.vue.push({ file: rel, error: vueResult.error });
    if (errorsOnly || verbose)
      console.log(`  [VUE ERR]    ${rel}: ${vueResult.error.slice(0, 100)}`);
    continue;
  }

  if (verterErr) {
    stats.verterErrors++;
    errors.verter.push({ file: rel, error: verterResult.error });
    if (errorsOnly || verbose)
      console.log(`  [VERTER ERR] ${rel}: ${verterResult.error.slice(0, 100)}`);
    continue;
  }

  if (errorsOnly) {
    // Both succeeded, skip in errors-only mode
    stats.total--; // don't count in total for errors-only
    continue;
  }

  // Both succeeded — extract and compare ssrRender bodies
  const vueBody = extractSsrRenderBody(vueResult.code);
  const verterBody = extractSsrRenderBody(verterResult.code);

  // If Vue produced no ssrRender (template-only or weird edge case), skip
  if (vueBody == null) {
    stats.noTemplate++;
    continue;
  }

  // If Verter produced no ssrRender, that's a mismatch
  if (verterBody == null) {
    stats.mismatches++;
    const pattern = "Missing ssrRender";
    mismatches.push({
      file: rel,
      pattern,
      vue: normalizeForComparison(vueBody),
      verter: "(no ssrRender found)",
    });
    if (verbose) console.log(`  [MISMATCH] ${rel} — ${pattern}`);
    continue;
  }

  const vueNorm = normalizeForComparison(vueBody);
  const verterNorm = normalizeForComparison(verterBody);

  if (vueNorm === verterNorm) {
    stats.matches++;
    if (verbose) console.log(`  [MATCH]    ${rel}`);
  } else {
    stats.mismatches++;
    const pattern = detectPattern(vueBody, verterBody);
    mismatches.push({ file: rel, pattern, vue: vueNorm, verter: verterNorm });
    if (verbose) console.log(`  [MISMATCH] ${rel} — ${pattern}`);
  }
}

// Clear progress line
if (processed >= 500) process.stdout.write("\r" + " ".repeat(60) + "\r");

const elapsed = ((Date.now() - startTime) / 1000).toFixed(1);

// ── Output ──────────────────────────────────────────────────────

printSummary(stats, mismatches, errors, elapsed);

if (jsonPath) {
  writeJsonReport(jsonPath, stats, mismatches, errors);
}

if (!jsonPath) {
  console.log(`\nRe-run with --json <path> to save full report`);
}
