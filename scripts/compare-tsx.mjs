/**
 * Compare TS Core vs Rust TSX output for .vue files across D:\dev.
 *
 * Usage:
 *   node scripts/compare-tsx.mjs [--write] [--verbose] [--filter <pattern>]
 *
 * Scans all git repos under D:\dev for .vue files, processes each through
 * both the TS Core pipeline (@verter/core buildSingle) and the Rust pipeline
 * (@verter/native VerterHost), then reports structural marker differences.
 *
 * Options:
 *   --write        Write side-by-side output files to scripts/compare-tsx-output/
 *   --verbose      Print per-file details even when markers match
 *   --filter <pat> Only process files whose path contains <pat>
 *   --errors-only  Only show files where one pipeline errored and the other didn't
 *
 * Note: TS Core buildSingle may have bugs — errors from it are tracked
 * separately and do not count as marker diffs.
 */

import { createRequire } from "node:module";
import { execSync } from "node:child_process";
import fs from "node:fs";
import path from "node:path";
import { fileURLToPath } from "node:url";

const require = createRequire(import.meta.url);
const __dirname = path.dirname(fileURLToPath(import.meta.url));
const ROOT = path.resolve(__dirname, "..");

// ── Load pipelines ──────────────────────────────────────────────

const { parser, buildSingle } = require(path.join(ROOT, "packages/core/dist/v5/index.js"));
const { VerterHost } = require(path.join(ROOT, "packages/native/index.js"));

// ── CLI args ────────────────────────────────────────────────────

const args = process.argv.slice(2);
const writeOutput = args.includes("--write");
const verbose = args.includes("--verbose");
const errorsOnly = args.includes("--errors-only");
const filterIdx = args.indexOf("--filter");
const filterPattern = filterIdx !== -1 ? args[filterIdx + 1] : null;

// ── Repo discovery ──────────────────────────────────────────────

const DEV_ROOT = "D:/dev";

/** Directories to scan for git repos. */
const SCAN_DIRS = ["personal", "github", "github/verter-test-repos"];

/** Directories to skip inside repos. */
const SKIP_DIRS = new Set([
  "node_modules",
  "dist",
  ".git",
  ".integration-tests",
  ".nuxt",
  ".output",
  ".vitepress",
]);

/**
 * Discover git repos under DEV_ROOT and collect .vue files via `git ls-files`.
 * Falls back to fs walk for non-git directories.
 */
function discoverVueFiles() {
  const files = [];

  for (const scanDir of SCAN_DIRS) {
    const base = path.join(DEV_ROOT, scanDir);
    if (!fs.existsSync(base)) continue;

    const entries = fs.readdirSync(base, { withFileTypes: true });
    for (const entry of entries) {
      if (!entry.isDirectory()) continue;
      if (SKIP_DIRS.has(entry.name)) continue;

      const repoPath = path.join(base, entry.name);
      const gitDir = path.join(repoPath, ".git");

      if (fs.existsSync(gitDir)) {
        // Use git ls-files for speed
        try {
          const output = execSync('git ls-files -z "*.vue"', {
            cwd: repoPath,
            encoding: "utf-8",
            timeout: 10000,
          }).trim();
          if (output) {
            for (let relFile of output.split("\0")) {
              if (!relFile) continue;
              // git ls-files -z with core.quotePath quotes non-ASCII — strip quotes
              if (relFile.startsWith('"') && relFile.endsWith('"')) {
                relFile = relFile.slice(1, -1);
              }
              const fullPath = path.join(repoPath, relFile);
              // Skip files inside node_modules etc. (git may track them)
              const parts = relFile.split("/");
              if (parts.some((p) => SKIP_DIRS.has(p))) continue;
              // Skip files that don't actually exist on disk (emoji/encoding issues)
              if (!fs.existsSync(fullPath)) continue;
              files.push(fullPath);
            }
          }
        } catch {
          // git ls-files failed, skip repo
        }
      }
    }
  }

  return files;
}

// ── Structural markers ─────────────────────────────────────────

const MARKERS = [
  { label: "TemplateBindingFN", pattern: "TemplateBindingFN" },
  { label: "TemplateBinding type", pattern: "TemplateBinding" },
  { label: "FullContextFN", pattern: "FullContextFN" },
  { label: "getRootComponent", pattern: "getRootComponent" },
  { label: "default_Component", pattern: "default_Component" },
  { label: "Instance", pattern: "Instance" },
  { label: "export default", pattern: "export default" },
  { label: "import @verter/types", pattern: 'from "@verter/types"' },
  { label: "import $verter/types", pattern: 'from "$verter/types"' },
  { label: "import vue", pattern: 'from "vue"' },
  { label: "createMacroReturn", pattern: "createMacroReturn" },
  { label: "Comp function", pattern: "function ___VERTER___Comp" },
  { label: "shallowUnwrapRef", pattern: "shallowUnwrapRef" },
  { label: "defineComponent", pattern: "defineComponent" },
];

// ── Pipeline wrappers ──────────────────────────────────────────

function tsCorePipeline(source, filename) {
  try {
    const result = parser(source, filename);
    const built = buildSingle(result);
    return { code: built.s.toString(), error: null };
  } catch (err) {
    return { code: null, error: err.message };
  }
}

function rustPipeline(host, filePath, source, filename) {
  try {
    const upsertResult = host.upsert({
      inputId: filePath,
      source,
      fileKind: "vue",
    });
    const profile = {
      filename,
      enableTypes: true,
      sourceMap: false,
    };
    // Trigger lazy compilation
    host.getVirtualFile({
      canonicalId: upsertResult.canonicalId,
      nodeKind: { kind: "main" },
      compileProfile: profile,
    });
    const tsx = host.getTsx(upsertResult.canonicalId, profile);
    if (!tsx) return { code: null, error: "null response from getTsx" };
    return { code: tsx.code, error: null };
  } catch (err) {
    return { code: null, error: err.message };
  }
}

function checkMarkers(code) {
  const result = {};
  for (const { label, pattern } of MARKERS) {
    result[label] = code.includes(pattern);
  }
  return result;
}

function relPath(filePath) {
  return path.relative(DEV_ROOT, filePath).replace(/\\/g, "/");
}

// ── Main ────────────────────────────────────────────────────────

const outputDir = path.join(__dirname, "compare-tsx-output");
if (writeOutput) {
  fs.mkdirSync(outputDir, { recursive: true });
}

console.log("Discovering .vue files under D:\\dev ...");
let allFiles = discoverVueFiles();
console.log(`Found ${allFiles.length} .vue files`);

if (filterPattern) {
  allFiles = allFiles.filter((f) => f.includes(filterPattern));
  console.log(`Filtered to ${allFiles.length} files matching "${filterPattern}"`);
}

const host = new VerterHost({ devMode: true, analysisLevel: "none" });

// ── Counters ────────────────────────────────────────────────────

let totalFiles = 0;
let totalMatch = 0;
let totalByDesignOnly = 0;
let totalDiffFiles = 0;
let totalMarkerDiffs = 0;
let totalRealDiffs = 0;
let tsErrors = 0;
let rustErrors = 0;
let bothErrors = 0;

/** Per-marker: how many files have a diff on this marker */
const markerDiffCounts = {};
for (const { label } of MARKERS) markerDiffCounts[label] = 0;

/** Collect TS error messages for grouping */
const tsErrorGroups = {};
/** Collect Rust error messages for grouping */
const rustErrorGroups = {};

/** Files where only Rust errored (TS succeeded) */
const rustOnlyErrors = [];
/** Files where only TS errored (Rust succeeded) */
const tsOnlyErrors = [];

// ── Process files ───────────────────────────────────────────────

const startTime = Date.now();

for (const filePath of allFiles) {
  totalFiles++;
  const source = fs.readFileSync(filePath, "utf-8");
  const filename = path.basename(filePath);
  const rel = relPath(filePath);

  const tsResult = tsCorePipeline(source, filename);
  const rustResult = rustPipeline(host, filePath, source, filename);

  // Track errors
  const tsErr = tsResult.error !== null;
  const rustErr = rustResult.error !== null;

  if (tsErr && rustErr) {
    bothErrors++;
    if (verbose) console.log(`  [BOTH ERR] ${rel}`);
    continue;
  }
  if (tsErr) {
    tsErrors++;
    const key = tsResult.error.slice(0, 80);
    tsErrorGroups[key] = (tsErrorGroups[key] || 0) + 1;
    tsOnlyErrors.push({ file: rel, error: tsResult.error });
    if (errorsOnly) console.log(`  [TS ERR] ${rel}: ${tsResult.error.slice(0, 100)}`);
    continue;
  }
  if (rustErr) {
    rustErrors++;
    const key = rustResult.error.slice(0, 80);
    rustErrorGroups[key] = (rustErrorGroups[key] || 0) + 1;
    rustOnlyErrors.push({ file: rel, error: rustResult.error });
    if (errorsOnly) console.log(`  [RUST ERR] ${rel}: ${rustResult.error.slice(0, 100)}`);
    continue;
  }

  if (errorsOnly) continue;

  // Both succeeded — compare markers
  const tsMarkers = checkMarkers(tsResult.code);
  const rustMarkers = checkMarkers(rustResult.code);

  const diffs = [];
  const realDiffs = [];
  for (const { label } of MARKERS) {
    if (tsMarkers[label] !== rustMarkers[label]) {
      const diff = { label, ts: tsMarkers[label], rust: rustMarkers[label] };
      diffs.push(diff);
      markerDiffCounts[label]++;

      // Classify: by-design vs real gap
      // By-design: TS uses $verter/types, Rust uses @verter/types
      if (label === "import @verter/types" && !tsMarkers[label] && rustMarkers[label]) continue;
      if (label === "import $verter/types" && tsMarkers[label] && !rustMarkers[label]) continue;
      // By-design: TS always imports createMacroReturn, Rust only when macros present
      if (label === "createMacroReturn" && tsMarkers[label] && !rustMarkers[label]) continue;
      // By-design: Rust has export default, TS may not (template-only TS core gap)
      if (label === "export default" && !tsMarkers[label] && rustMarkers[label]) continue;

      realDiffs.push(diff);
    }
  }

  if (diffs.length === 0) {
    totalMatch++;
    if (verbose) console.log(`  [MATCH] ${rel}`);
  } else if (realDiffs.length === 0) {
    totalByDesignOnly++;
    if (verbose) console.log(`  [BY-DESIGN] ${rel}`);
  } else {
    totalDiffFiles++;
    totalMarkerDiffs += diffs.length;
    totalRealDiffs += realDiffs.length;
    if (!errorsOnly) {
      console.log(`  [DIFF] ${rel}`);
      for (const d of realDiffs) {
        const tsStr = d.ts ? "YES" : " - ";
        const rustStr = d.rust ? "YES" : " - ";
        console.log(`         ${d.label.padEnd(25)} TS: ${tsStr}  Rust: ${rustStr}`);
      }
    }
  }

  // Write output files
  if (writeOutput) {
    const safeName = rel.replace(/[/\\:]/g, "_");
    const dir = path.join(outputDir, safeName);
    fs.mkdirSync(dir, { recursive: true });
    fs.writeFileSync(path.join(dir, "source.vue"), source);
    fs.writeFileSync(path.join(dir, "ts-core.tsx"), tsResult.code);
    fs.writeFileSync(path.join(dir, "rust.tsx"), rustResult.code);
  }
}

const elapsed = ((Date.now() - startTime) / 1000).toFixed(1);

// ── Summary ─────────────────────────────────────────────────────

console.log(`\n${"═".repeat(60)}`);
console.log(`COMPARISON SUMMARY (${elapsed}s)`);
console.log(`${"═".repeat(60)}`);
console.log(`Total .vue files:  ${totalFiles}`);
console.log(`Both succeeded:    ${totalMatch + totalByDesignOnly + totalDiffFiles}`);
console.log(`  Markers match:   ${totalMatch}`);
console.log(`  By-design only:  ${totalByDesignOnly}`);
console.log(`  Real diffs:      ${totalDiffFiles} (${totalRealDiffs} marker diffs)`);
console.log(`TS Core errors:    ${tsErrors} (Rust succeeded)`);
console.log(`Rust errors:       ${rustErrors} (TS succeeded)`);
console.log(`Both errored:      ${bothErrors}`);

if (totalMarkerDiffs > 0) {
  console.log(`\n── Marker diff breakdown ──`);
  const sorted = Object.entries(markerDiffCounts)
    .filter(([, v]) => v > 0)
    .sort((a, b) => b[1] - a[1]);
  for (const [label, count] of sorted) {
    console.log(`  ${label.padEnd(25)} ${count} files`);
  }
}

if (Object.keys(tsErrorGroups).length > 0) {
  console.log(`\n── TS Core error groups (${tsErrors} files) ──`);
  const sorted = Object.entries(tsErrorGroups).sort((a, b) => b[1] - a[1]);
  for (const [msg, count] of sorted.slice(0, 15)) {
    console.log(`  (${count}x) ${msg}`);
  }
  if (sorted.length > 15) console.log(`  ... and ${sorted.length - 15} more groups`);
}

if (Object.keys(rustErrorGroups).length > 0) {
  console.log(`\n── Rust error groups (${rustErrors} files) ──`);
  const sorted = Object.entries(rustErrorGroups).sort((a, b) => b[1] - a[1]);
  for (const [msg, count] of sorted.slice(0, 15)) {
    console.log(`  (${count}x) ${msg}`);
  }
  if (sorted.length > 15) console.log(`  ... and ${sorted.length - 15} more groups`);
}

// Write JSON report
if (writeOutput) {
  const report = {
    timestamp: new Date().toISOString(),
    elapsed,
    totalFiles,
    totalMatch,
    totalByDesignOnly,
    totalDiffFiles,
    totalMarkerDiffs,
    totalRealDiffs,
    tsErrors,
    rustErrors,
    bothErrors,
    markerDiffCounts,
    tsErrorGroups,
    rustErrorGroups,
    rustOnlyErrors: rustOnlyErrors.slice(0, 50),
    tsOnlyErrors: tsOnlyErrors.slice(0, 50),
  };
  fs.writeFileSync(path.join(outputDir, "report.json"), JSON.stringify(report, null, 2));
  console.log(`\nOutput written to: ${outputDir}`);
}

if (!writeOutput) {
  console.log(`\nRe-run with --write to save output files and JSON report`);
}
