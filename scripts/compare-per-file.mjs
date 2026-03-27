#!/usr/bin/env node

/**
 * Per-File VDOM Codegen Comparison: Vue vs Verter
 *
 * Compiles each .vue file from a project with both Vue's official compiler
 * and Verter, across a 3-mode matrix (dev, prod, SSR), and diffs the outputs
 * to find logic differences that could cause runtime bugs.
 *
 * Two comparison layers:
 *   Layer 1 (direct): @vue/compiler-sfc vs @verter/native host API
 *   Layer 2 (vite):   Vite + @vitejs/plugin-vue vs Vite + @verter/unplugin
 *
 * Usage:
 *   node scripts/compare-per-file.mjs --project <path> [options]
 */

import { existsSync, mkdirSync, readFileSync, writeFileSync, readdirSync } from "fs";
import { resolve, relative, basename, join, dirname } from "path";
import { createRequire } from "module";
import { spawnSync } from "child_process";
import { createHash } from "crypto";
import { fileURLToPath } from "url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const VERTER_ROOT = resolve(__dirname, "..");

// ─── CLI ─────────────────────────────────────────────────────────────────────

function parseArgs() {
  const args = process.argv.slice(2);
  const opts = {
    project: null,
    modes: ["dev", "prod", "ssr"],
    layers: ["direct", "vite"],
    filter: null,
    out: null,
    concurrency: 1,
  };

  for (let i = 0; i < args.length; i++) {
    switch (args[i]) {
      case "--project":
        opts.project = resolve(args[++i]);
        break;
      case "--modes":
        opts.modes = args[++i].split(",").map((m) => m.trim().toLowerCase());
        break;
      case "--layer":
        opts.layers = args[++i].split(",").map((l) => l.trim().toLowerCase());
        break;
      case "--filter":
        opts.filter = args[++i];
        break;
      case "--out":
        opts.out = resolve(args[++i]);
        break;
      case "--concurrency":
        opts.concurrency = parseInt(args[++i], 10);
        break;
      case "--help":
        printUsage();
        process.exit(0);
      default:
        if (!args[i].startsWith("--") && !opts.project) {
          opts.project = resolve(args[i]);
        } else {
          console.error(`Unknown argument: ${args[i]}`);
          printUsage();
          process.exit(1);
        }
    }
  }

  if (!opts.project) {
    console.error("Error: --project <path> is required");
    printUsage();
    process.exit(1);
  }
  if (!opts.out) {
    opts.out = join(opts.project, ".verter-compare-files");
  }
  return opts;
}

function printUsage() {
  console.log(`
Usage: node scripts/compare-per-file.mjs --project <path> [options]

Options:
  --project <path>        Target project root (required)
  --modes dev,prod,ssr    Modes to test (default: all three)
  --layer direct,vite     Which layers to run (default: both)
  --filter <glob>         Only process matching .vue files
  --out <path>            Output directory (default: <project>/.verter-compare-files)
  --concurrency <n>       Parallel file processing (default: 1)
  --help                  Show this help
`);
}

// ─── Utilities ───────────────────────────────────────────────────────────────

const MODE_CONFIG = {
  dev: { isProd: false, ssr: false, label: "DEV" },
  prod: { isProd: true, ssr: false, label: "PROD" },
  ssr: { isProd: false, ssr: true, label: "SSR" },
};

function getHash(text) {
  return createHash("sha256").update(text).digest("hex").substring(0, 8);
}

function ensureDir(dir) {
  mkdirSync(dir, { recursive: true });
}

function findVueFiles(dir, filter) {
  const results = [];
  const excludeDirs = new Set([
    "node_modules",
    "dist",
    "dist_vue",
    "dist_verter",
    ".git",
    ".verter-compare",
    ".verter-compare-files",
    ".nuxt",
    ".output",
    "coverage",
    "__snapshots__",
  ]);

  function walk(d) {
    let entries;
    try {
      entries = readdirSync(d, { withFileTypes: true });
    } catch {
      return;
    }
    for (const entry of entries) {
      if (entry.isDirectory()) {
        if (!excludeDirs.has(entry.name) && !entry.name.startsWith(".")) {
          walk(join(d, entry.name));
        }
      } else if (entry.isFile() && entry.name.endsWith(".vue")) {
        const rel = relative(dir, join(d, entry.name)).replace(/\\/g, "/");
        if (!filter || simpleGlob(rel, filter)) {
          results.push(rel);
        }
      }
    }
  }

  walk(dir);
  results.sort();
  return results;
}

function simpleGlob(str, pattern) {
  const regex = pattern
    .replace(/\*\*/g, "{{GLOBSTAR}}")
    .replace(/\*/g, "[^/]*")
    .replace(/{{GLOBSTAR}}/g, ".*")
    .replace(/\?/g, ".");
  return new RegExp(`^${regex}$`).test(str);
}

// ─── JS Validation & Normalization ──────────────────────────────────────────

function tryParseJs(code) {
  try {
    // Check for obvious structural issues that indicate real codegen bugs
    if (code.includes("_ctx.{")) return { valid: false, error: "_ctx.{ on object literal" };

    // Look for the render function and do a structural brace check
    const renderIdx = code.indexOf("function render(");
    if (renderIdx >= 0) {
      const renderCode = code.slice(renderIdx);
      // Count braces, properly handling strings and template literals
      let braces = 0;
      let inString = false,
        stringChar = "",
        escaped = false;
      let templateDepth = 0; // track ${} nesting inside template literals

      for (let i = 0; i < renderCode.length; i++) {
        const ch = renderCode[i];

        if (escaped) {
          escaped = false;
          continue;
        }
        if (ch === "\\") {
          escaped = true;
          continue;
        }

        if (inString) {
          if (stringChar === "`") {
            // Template literal: handle ${} expressions
            if (ch === "$" && i + 1 < renderCode.length && renderCode[i + 1] === "{") {
              templateDepth++;
              i++; // skip the {
              braces++; // count the { from ${}
              continue;
            }
            if (templateDepth > 0) {
              if (ch === "{") braces++;
              else if (ch === "}") {
                braces--;
                templateDepth--;
                if (templateDepth === 0) continue; // closing the ${}, still in template
              }
              continue;
            }
            if (ch === "`") {
              inString = false;
              continue;
            }
          } else {
            if (ch === stringChar) inString = false;
          }
          continue;
        }

        if (ch === '"' || ch === "'" || ch === "`") {
          inString = true;
          stringChar = ch;
          continue;
        }
        if (ch === "{") braces++;
        else if (ch === "}") braces--;
        if (braces < 0) return { valid: false, error: "Unbalanced '}' in render function" };
      }
      if (braces !== 0)
        return { valid: false, error: `Unbalanced braces in render function: ${braces}` };
    }

    return { valid: true };
  } catch (err) {
    return { valid: false, error: String(err) };
  }
}

function normalizeJs(code) {
  return code
    .replace(/\r\n/g, "\n")
    .replace(/[ \t]+/g, " ")
    .replace(/\n\s*\n/g, "\n")
    .replace(/,\s*([)\]}])/g, "$1")
    .trim();
}

function extractImports(code) {
  const imports = [];
  const re =
    /import\s+(?:\{[^}]+\}|\*\s+as\s+\w+|\w+)?\s*(?:,\s*(?:\{[^}]+\}|\*\s+as\s+\w+))?\s*from\s+['"]([^'"]+)['"]/g;
  let m;
  while ((m = re.exec(code)) !== null) imports.push(m[1]);
  return imports;
}

function extractHelperImports(code) {
  // Extract Vue runtime helper names from import statements
  const helpers = new Set();
  const re = /import\s+\{([^}]+)\}\s+from\s+['"]vue['"]/g;
  let m;
  while ((m = re.exec(code)) !== null) {
    for (const name of m[1].split(",")) {
      helpers.add(
        name
          .trim()
          .split(/\s+as\s+/)[0]
          .trim(),
      );
    }
  }
  return helpers;
}

function extractPatchFlags(code) {
  // Find patch flag constants like /* TEXT */ or numeric flags
  const flags = [];
  const re = /(\d+)\s*\/\*\s*([A-Z_]+)\s*\*\//g;
  let m;
  while ((m = re.exec(code)) !== null) {
    flags.push({ value: parseInt(m[1], 10), name: m[2] });
  }
  return flags;
}

function extractHoistedNodes(code) {
  // Count _hoisted_N declarations
  const re = /const _hoisted_(\d+)/g;
  let count = 0,
    m;
  while ((m = re.exec(code)) !== null) count = Math.max(count, parseInt(m[1], 10));
  return count;
}

// ─── Diff Classification ────────────────────────────────────────────────────

function classifyDiff(vueCode, verterCode) {
  // A: Verter output is invalid JS
  const verterParse = tryParseJs(verterCode);
  if (!verterParse.valid) {
    return { category: "A", severity: "P0", reason: `Invalid JS: ${verterParse.error}` };
  }

  // Check Vue validity too (skip if Vue output is also invalid)
  if (!tryParseJs(vueCode).valid) {
    return { category: "E", severity: "TRACKED", reason: "Vue output also invalid — skip" };
  }

  // Exact match after normalization
  if (normalizeJs(vueCode) === normalizeJs(verterCode)) {
    return null; // identical
  }

  // B: Import/export differences
  const vueImps = new Set(extractImports(vueCode));
  const verterImps = new Set(extractImports(verterCode));
  const missingImps = [...vueImps].filter((i) => !verterImps.has(i));
  const extraImps = [...verterImps].filter((i) => !vueImps.has(i));
  if (missingImps.length > 0 || extraImps.length > 0) {
    const parts = [];
    if (missingImps.length) parts.push(`missing: ${missingImps.join(", ")}`);
    if (extraImps.length) parts.push(`extra: ${extraImps.join(", ")}`);
    return { category: "B", severity: "P1", reason: `Import diff: ${parts.join("; ")}` };
  }

  // C: Structural — different helper calls
  const vueHelpers = extractHelperImports(vueCode);
  const verterHelpers = extractHelperImports(verterCode);
  const missingHelpers = [...vueHelpers].filter((h) => !verterHelpers.has(h));
  const extraHelpers = [...verterHelpers].filter((h) => !vueHelpers.has(h));
  if (missingHelpers.length > 0 || extraHelpers.length > 0) {
    const parts = [];
    if (missingHelpers.length) parts.push(`missing: ${missingHelpers.join(", ")}`);
    if (extraHelpers.length) parts.push(`extra: ${extraHelpers.join(", ")}`);
    return { category: "C", severity: "P2", reason: `Helper diff: ${parts.join("; ")}` };
  }

  // C: Structural — hoisting differences
  const vueHoisted = extractHoistedNodes(vueCode);
  const verterHoisted = extractHoistedNodes(verterCode);
  if (Math.abs(vueHoisted - verterHoisted) > 0) {
    return {
      category: "C",
      severity: "P2",
      reason: `Hoisted nodes: vue=${vueHoisted} verter=${verterHoisted}`,
    };
  }

  // D: Patch flag differences
  const vueFlags = extractPatchFlags(vueCode);
  const verterFlags = extractPatchFlags(verterCode);
  if (vueFlags.length !== verterFlags.length) {
    return {
      category: "D",
      severity: "P2",
      reason: `Patch flag count: vue=${vueFlags.length} verter=${verterFlags.length}`,
    };
  }
  for (let i = 0; i < vueFlags.length; i++) {
    if (vueFlags[i].value !== verterFlags[i]?.value) {
      return {
        category: "D",
        severity: "P2",
        reason: `Patch flag mismatch at #${i}: vue=${vueFlags[i].value}(${vueFlags[i].name}) verter=${verterFlags[i]?.value}(${verterFlags[i]?.name})`,
      };
    }
  }

  // D: Function count difference (structural complexity)
  const vueFnCount = (vueCode.match(/function\s/g) || []).length;
  const verterFnCount = (verterCode.match(/function\s/g) || []).length;
  if (Math.abs(vueFnCount - verterFnCount) > 2) {
    return {
      category: "C",
      severity: "P2",
      reason: `Function count: vue=${vueFnCount} verter=${verterFnCount}`,
    };
  }

  // E: Cosmetic / minor
  return { category: "E", severity: "TRACKED", reason: "Output differs (cosmetic/minor)" };
}

// ─── Layer 1: Direct Compiler API ────────────────────────────────────────────

function runDirectLayer(opts, vueFiles, outDir) {
  console.log("\n=== Layer 1: Direct Compiler API ===\n");

  // We spawn a child process that has access to both Vue and Verter APIs
  const runnerPath = join(outDir, "_direct_runner.mjs");
  const resultsPath = join(outDir, "_direct_results.json");

  const verterNativePath = join(VERTER_ROOT, "packages", "native").replace(/\\/g, "/");
  const projectDir = opts.project.replace(/\\/g, "/");
  const vueFilesJson = JSON.stringify(vueFiles);
  const modesJson = JSON.stringify(opts.modes);

  const runnerCode = `
import { readFileSync, writeFileSync } from 'fs';
import { resolve, join, dirname } from 'path';
import { createRequire } from 'module';

const require = createRequire(import.meta.url);

// Load Vue compiler from project's node_modules (if available) or from verter's
let vueSfc;
try {
  vueSfc = require('${projectDir}/node_modules/@vue/compiler-sfc');
} catch {
  try {
    vueSfc = require('${join(VERTER_ROOT, "node_modules", "@vue", "compiler-sfc").replace(/\\/g, "/")}');
  } catch {
    vueSfc = null;
  }
}

// Load Verter native
let native;
try {
  native = require('${verterNativePath}');
} catch (err) {
  console.error('Failed to load @verter/native:', err.message);
  process.exit(1);
}

const projectDir = '${projectDir}';
const vueFiles = ${vueFilesJson};
const modes = ${modesJson};

function compileWithVue(source, filename, mode) {
  if (!vueSfc) return { code: '', error: '@vue/compiler-sfc not available' };

  const isProd = mode === 'prod';
  const isSsr = mode === 'ssr';

  try {
    const { descriptor, errors: parseErrors } = vueSfc.parse(source, { filename });
    if (parseErrors?.length > 0) {
      return { code: '', error: parseErrors.map(e => e.message).join('; ') };
    }

    let scriptCode = '';
    let bindingMetadata;

    if (descriptor.script || descriptor.scriptSetup) {
      try {
        const scriptResult = vueSfc.compileScript(descriptor, {
          id: filename,
          inlineTemplate: false,
          isProd,
        });
        scriptCode = scriptResult.content;
        bindingMetadata = scriptResult.bindings;
      } catch (err) {
        return { code: '', error: 'compileScript: ' + (err.message || String(err)) };
      }
    }

    let templateCode = '';
    if (descriptor.template) {
      try {
        const templateResult = vueSfc.compileTemplate({
          source: descriptor.template.content,
          filename,
          id: filename,
          scoped: descriptor.styles.some(s => s.scoped),
          isProd,
          ssr: isSsr,
          ssrCssVars: isSsr ? descriptor.cssVars : undefined,
          compilerOptions: {
            mode: 'module',
            bindingMetadata,
          },
        });
        if (templateResult.errors?.length > 0) {
          const errs = templateResult.errors.map(e => typeof e === 'string' ? e : e.message);
          return { code: '', error: 'compileTemplate: ' + errs.join('; ') };
        }
        templateCode = templateResult.code;
      } catch (err) {
        return { code: '', error: 'compileTemplate: ' + (err.message || String(err)) };
      }
    }

    const code = [scriptCode, templateCode].filter(Boolean).join('\\n\\n');
    return { code, error: null };
  } catch (err) {
    return { code: '', error: String(err.message || err) };
  }
}

function compileWithVerter(host, source, filename, mode) {
  const isProd = mode === 'prod';
  const isSsr = mode === 'ssr';

  try {
    host.remove(filename);
    const upsertResult = host.upsert({
      inputId: filename,
      source,
    });

    // Resolve type deps (relative .ts imports)
    if (upsertResult.importSpecifiers?.length > 0) {
      const fs = await_fs;
      const path = await_path;
      const exts = ['', '.ts', '.tsx', '.js', '.jsx'];
      for (const imp of upsertResult.importSpecifiers) {
        if (!imp.source.startsWith('.')) continue;
        const absBase = path.resolve(path.dirname(filename), imp.source);
        for (const ext of exts) {
          const fullPath = absBase + ext;
          if (fullPath.endsWith('.vue')) continue;
          try {
            const depSource = fs.readFileSync(fullPath, 'utf-8');
            host.upsert({ inputId: fullPath, source: depSource, fileKind: 'non_sfc' });
            break;
          } catch { continue; }
        }
      }
    }

    // Get the main virtual file (script + template combined)
    const profile = {
      filename,
      isProduction: isProd,
      ssr: isSsr,
      hmrStrategy: 'none',
      sourceMap: false,
      forceJs: false,
    };

    // Try getting separate script + template blocks
    let code = '';
    const scriptFile = host.getVirtualFile({
      canonicalId: upsertResult.canonicalId,
      nodeKind: { kind: 'script' },
      compileProfile: profile,
    });
    if (scriptFile) code += scriptFile.code;

    const templateFile = host.getVirtualFile({
      canonicalId: upsertResult.canonicalId,
      nodeKind: { kind: 'template' },
      compileProfile: profile,
    });
    if (templateFile) code += (code ? '\\n\\n' : '') + templateFile.code;

    // If no script or template, try 'main'
    if (!code) {
      const mainFile = host.getVirtualFile({
        canonicalId: upsertResult.canonicalId,
        nodeKind: { kind: 'main' },
        compileProfile: profile,
      });
      if (mainFile) code = mainFile.code;
    }

    const errors = upsertResult.diagnostics?.diagnostics
      ?.filter(d => d.severity === 'error')
      .map(d => d.message) || [];

    return { code, error: errors.length > 0 ? errors.join('; ') : null };
  } catch (err) {
    return { code: '', error: String(err.message || err) };
  }
}

// We need fs/path synchronously in compileWithVerter
import fs from 'fs';
import path from 'path';
const await_fs = fs;
const await_path = path;

async function main() {
  const host = new native.VerterHost({ devMode: true, analysisLevel: 'essential' });

  const results = [];

  for (const relPath of vueFiles) {
    const absPath = resolve(projectDir, relPath).replace(/\\\\/g, '/');
    let source;
    try {
      source = readFileSync(absPath, 'utf-8');
    } catch (err) {
      results.push({ file: relPath, error: 'read failed: ' + err.message });
      continue;
    }

    const fileResult = { file: relPath, modes: {} };

    for (const mode of modes) {
      const vue = compileWithVue(source, absPath, mode);
      const verter = compileWithVerter(host, source, absPath, mode);

      fileResult.modes[mode] = {
        vue: { code: vue.code, error: vue.error, hasCode: !!vue.code },
        verter: { code: verter.code, error: verter.error, hasCode: !!verter.code },
      };
    }

    results.push(fileResult);
  }

  writeFileSync('${resultsPath.replace(/\\/g, "/")}', JSON.stringify(results, null, 2));
}

main().catch(err => {
  console.error('Direct runner fatal:', err);
  writeFileSync('${resultsPath.replace(/\\/g, "/")}', JSON.stringify([{ error: 'fatal: ' + String(err.message || err) }]));
  process.exit(1);
});
`;

  writeFileSync(runnerPath, runnerCode);

  console.log(`Compiling ${vueFiles.length} files across ${opts.modes.length} modes...`);

  const result = spawnSync("node", [runnerPath], {
    cwd: opts.project,
    stdio: ["pipe", "pipe", "pipe"],
    timeout: 600_000,
    encoding: "utf-8",
    env: { ...process.env, NODE_OPTIONS: "" },
  });

  if (result.stderr) {
    const stderrLines = result.stderr.trim().split("\n").filter(Boolean);
    if (stderrLines.length > 0) {
      console.log(`  Runner stderr (${stderrLines.length} lines):`);
      for (const line of stderrLines.slice(0, 10)) {
        console.log(`    ${line.slice(0, 200)}`);
      }
      if (stderrLines.length > 10) console.log(`    ... and ${stderrLines.length - 10} more`);
    }
  }

  if (!existsSync(resultsPath)) {
    console.error("Direct runner produced no output file.");
    console.error("stdout:", result.stdout?.slice(0, 500));
    return [];
  }

  const rawResults = JSON.parse(readFileSync(resultsPath, "utf-8"));

  // Classify diffs
  const diffs = [];
  let identical = 0;
  let vueErrors = 0;
  let verterErrors = 0;

  for (const fileResult of rawResults) {
    if (fileResult.error) {
      diffs.push({
        layer: "direct",
        file: fileResult.file || "?",
        mode: "*",
        category: "A",
        severity: "P0",
        reason: fileResult.error,
      });
      continue;
    }

    for (const mode of opts.modes) {
      const modeResult = fileResult.modes?.[mode];
      if (!modeResult) continue;

      const { vue, verter } = modeResult;

      if (vue.error && !verter.error) {
        vueErrors++;
        continue; // Vue failed, skip comparison
      }
      if (verter.error) {
        verterErrors++;
        diffs.push({
          layer: "direct",
          file: fileResult.file,
          mode,
          category: "A",
          severity: "P0",
          reason: `Verter error: ${verter.error}`,
        });
        continue;
      }
      if (!vue.hasCode && !verter.hasCode) continue; // both empty (no template)
      if (!vue.hasCode || !verter.hasCode) {
        diffs.push({
          layer: "direct",
          file: fileResult.file,
          mode,
          category: "B",
          severity: "P1",
          reason: `One side empty: vue=${vue.hasCode} verter=${verter.hasCode}`,
        });
        continue;
      }

      const diff = classifyDiff(vue.code, verter.code);
      if (diff) {
        diffs.push({
          layer: "direct",
          file: fileResult.file,
          mode,
          ...diff,
        });

        // Save diff files for inspection
        if (diff.category <= "D") {
          const hash = getHash(fileResult.file + mode);
          const vueCapture = join(outDir, "captures", `${hash}_vue_${mode}.js`);
          const verterCapture = join(outDir, "captures", `${hash}_verter_${mode}.js`);
          ensureDir(join(outDir, "captures"));
          writeFileSync(vueCapture, vue.code);
          writeFileSync(verterCapture, verter.code);
        }
      } else {
        identical++;
      }
    }
  }

  console.log(`\nDirect layer results:`);
  console.log(`  Identical: ${identical}`);
  console.log(`  Diffs: ${diffs.length}`);
  console.log(`  Vue errors (skipped): ${vueErrors}`);
  console.log(`  Verter errors: ${verterErrors}`);

  return diffs;
}

// ─── Layer 2: Vite Pipeline ─────────────────────────────────────────────────

function resolveViteConfig(projectDir) {
  for (const c of ["vite.config.ts", "vite.config.js", "vite.config.mjs", "vite.config.mts"]) {
    const abs = join(projectDir, c);
    if (existsSync(abs)) return abs;
  }
  return null;
}

function runViteLayer(opts, vueFiles, outDir) {
  const viteConfig = resolveViteConfig(opts.project);
  if (!viteConfig) {
    console.log("\n=== Layer 2: Vite Pipeline — SKIPPED (no vite config) ===\n");
    return [];
  }

  const nm = join(opts.project, "node_modules");
  if (!existsSync(nm)) {
    console.log("\n=== Layer 2: Vite Pipeline — SKIPPED (no node_modules) ===\n");
    return [];
  }

  console.log("\n=== Layer 2: Vite Pipeline ===\n");
  console.log(`  Config: ${relative(opts.project, viteConfig)}`);

  const resultsPath = join(outDir, "_vite_results.json");
  const configPathFwd = viteConfig.replace(/\\/g, "/");
  const projectDirFwd = opts.project.replace(/\\/g, "/");
  const verterPluginPath = join(VERTER_ROOT, "packages", "unplugin", "dist", "vite.mjs").replace(
    /\\/g,
    "/",
  );
  const modesJson = JSON.stringify(opts.modes);
  const vueFilesJson = JSON.stringify(vueFiles);

  // Generate a runner that uses Vite's dev server to transformRequest each file
  const runnerPath = join(outDir, "_vite_runner.mjs");
  const runnerCode = `
import { createServer, loadConfigFromFile } from 'vite';
import { writeFileSync } from 'fs';

const projectDir = '${projectDirFwd}';
const configFile = '${configPathFwd}';
const verterPluginPath = 'file:///${verterPluginPath}';
const modes = ${modesJson};
const vueFiles = ${vueFilesJson};

async function createVueServer(ssr) {
  return await createServer({
    configFile,
    root: projectDir,
    server: { middlewareMode: true },
    appType: 'custom',
    logLevel: 'silent',
    optimizeDeps: { noDiscovery: true },
  });
}

async function createVerterServer(ssr) {
  const _verterMod = await import(verterPluginPath);
  const verter = _verterMod.verter || _verterMod.default;

  const loaded = await loadConfigFromFile(
    { command: 'serve', mode: 'development' },
    configFile,
    projectDir
  );
  const userConfig = loaded?.config || {};
  const { plugins: origPlugins, ...rest } = userConfig;

  const nonVuePlugins = (origPlugins || []).flat().filter(p => {
    const name = p?.name || '';
    return name !== 'vite:vue' && name !== 'vite-plugin-verter';
  });

  return await createServer({
    ...rest,
    configFile: false,
    plugins: [verter(), ...nonVuePlugins],
    root: projectDir,
    server: { middlewareMode: true },
    appType: 'custom',
    logLevel: 'silent',
    optimizeDeps: { noDiscovery: true },
  });
}

async function main() {
  const results = [];

  for (const mode of modes) {
    const ssr = mode === 'ssr';
    let vueServer, verterServer;

    try {
      vueServer = await createVueServer(ssr);
    } catch (err) {
      results.push({ mode, error: 'Vue server failed: ' + (err.message || String(err)).slice(0, 300) });
      continue;
    }

    try {
      verterServer = await createVerterServer(ssr);
    } catch (err) {
      await vueServer.close();
      results.push({ mode, error: 'Verter server failed: ' + (err.message || String(err)).slice(0, 300) });
      continue;
    }

    for (const vuePath of vueFiles) {
      const moduleId = '/' + vuePath;
      const entry = { file: vuePath, mode, vue: {}, verter: {} };

      try {
        const vueResult = await vueServer.transformRequest(moduleId, { ssr });
        entry.vue = { code: vueResult?.code || '', error: null, hasCode: !!vueResult?.code };
      } catch (err) {
        entry.vue = { code: '', error: (err.message || String(err)).slice(0, 300), hasCode: false };
      }

      try {
        const verterResult = await verterServer.transformRequest(moduleId, { ssr });
        entry.verter = { code: verterResult?.code || '', error: null, hasCode: !!verterResult?.code };
      } catch (err) {
        entry.verter = { code: '', error: (err.message || String(err)).slice(0, 300), hasCode: false };
      }

      results.push(entry);
    }

    await vueServer.close();
    await verterServer.close();
  }

  writeFileSync('${resultsPath.replace(/\\/g, "/")}', JSON.stringify(results, null, 2));
}

main().catch(err => {
  console.error('Vite runner fatal:', err);
  writeFileSync('${resultsPath.replace(/\\/g, "/")}', JSON.stringify([{ error: 'fatal: ' + String(err.message || err) }]));
  process.exit(1);
});
`;

  writeFileSync(runnerPath, runnerCode);

  console.log(`Compiling ${vueFiles.length} files across ${opts.modes.length} modes via Vite...`);

  const result = spawnSync("node", [runnerPath], {
    cwd: opts.project,
    stdio: ["pipe", "pipe", "pipe"],
    timeout: 600_000,
    encoding: "utf-8",
    env: { ...process.env, NODE_OPTIONS: "" },
  });

  if (result.stderr) {
    const stderrLines = result.stderr.trim().split("\n").filter(Boolean);
    if (stderrLines.length > 0) {
      console.log(`  Runner stderr (${stderrLines.length} lines):`);
      for (const line of stderrLines.slice(0, 10)) {
        console.log(`    ${line.slice(0, 200)}`);
      }
      if (stderrLines.length > 10) console.log(`    ... and ${stderrLines.length - 10} more`);
    }
  }

  if (!existsSync(resultsPath)) {
    console.error("Vite runner produced no output file.");
    return [];
  }

  const rawResults = JSON.parse(readFileSync(resultsPath, "utf-8"));

  const diffs = [];
  let identical = 0;
  let vueErrors = 0;
  let verterErrors = 0;

  for (const entry of rawResults) {
    if (entry.error) {
      diffs.push({
        layer: "vite",
        file: entry.file || "?",
        mode: entry.mode || "*",
        category: "A",
        severity: "P0",
        reason: entry.error,
      });
      continue;
    }

    const { vue, verter } = entry;
    if (!vue || !verter) continue;

    if (vue.error && !verter.error) {
      vueErrors++;
      continue;
    }
    if (verter.error) {
      verterErrors++;
      diffs.push({
        layer: "vite",
        file: entry.file,
        mode: entry.mode,
        category: "A",
        severity: "P0",
        reason: `Verter error: ${verter.error}`,
      });
      continue;
    }
    if (!vue.hasCode && !verter.hasCode) continue;
    if (!vue.hasCode || !verter.hasCode) {
      diffs.push({
        layer: "vite",
        file: entry.file,
        mode: entry.mode,
        category: "B",
        severity: "P1",
        reason: `One side empty: vue=${vue.hasCode} verter=${verter.hasCode}`,
      });
      continue;
    }

    const diff = classifyDiff(vue.code, verter.code);
    if (diff) {
      diffs.push({
        layer: "vite",
        file: entry.file,
        mode: entry.mode,
        ...diff,
      });

      if (diff.category <= "D") {
        const hash = getHash(entry.file + entry.mode + "vite");
        ensureDir(join(outDir, "captures-vite"));
        writeFileSync(join(outDir, "captures-vite", `${hash}_vue_${entry.mode}.js`), vue.code);
        writeFileSync(
          join(outDir, "captures-vite", `${hash}_verter_${entry.mode}.js`),
          verter.code,
        );
      }
    } else {
      identical++;
    }
  }

  console.log(`\nVite layer results:`);
  console.log(`  Identical: ${identical}`);
  console.log(`  Diffs: ${diffs.length}`);
  console.log(`  Vue errors (skipped): ${vueErrors}`);
  console.log(`  Verter errors: ${verterErrors}`);

  return diffs;
}

// ─── Report Generation ──────────────────────────────────────────────────────

function generateReport(allDiffs, outDir, opts) {
  const categoryNames = {
    A: "Invalid JS (P0)",
    B: "Missing/Extra Output (P1)",
    C: "Structural Differences (P2)",
    D: "Wrong Values (P2)",
    E: "Cosmetic/Known (TRACKED)",
  };
  const categoryOrder = ["A", "B", "C", "D", "E"];

  // Group by category
  const byCategory = {};
  for (const d of allDiffs) {
    if (!byCategory[d.category]) byCategory[d.category] = [];
    byCategory[d.category].push(d);
  }

  // Group by mode
  const byMode = {};
  for (const d of allDiffs) {
    if (!byMode[d.mode]) byMode[d.mode] = [];
    byMode[d.mode].push(d);
  }

  let report = `# Per-File Comparison Report\n\n`;
  report += `**Project:** ${opts.project}\n`;
  report += `**Date:** ${new Date().toISOString()}\n`;
  report += `**Modes:** ${opts.modes.join(", ")}\n`;
  report += `**Layers:** ${opts.layers.join(", ")}\n\n`;

  // Summary table
  report += `## Summary\n\n`;
  report += `| Category | Count | Severity |\n|----------|-------|----------|\n`;
  for (const cat of categoryOrder) {
    const items = byCategory[cat] || [];
    if (items.length === 0) continue;
    report += `| ${cat}: ${categoryNames[cat]} | ${items.length} | ${items[0]?.severity} |\n`;
  }
  report += `\n**Total differences:** ${allDiffs.length}\n\n`;

  // By-mode breakdown
  report += `## By Mode\n\n`;
  report += `| Mode | A (P0) | B (P1) | C (P2) | D (P2) | E (Tracked) | Total |\n`;
  report += `|------|--------|--------|--------|--------|-------------|-------|\n`;
  for (const mode of opts.modes) {
    const modeDiffs = byMode[mode] || [];
    const counts = {};
    for (const d of modeDiffs) counts[d.category] = (counts[d.category] || 0) + 1;
    report += `| ${mode} | ${counts.A || 0} | ${counts.B || 0} | ${counts.C || 0} | ${counts.D || 0} | ${counts.E || 0} | ${modeDiffs.length} |\n`;
  }
  report += "\n";

  // Details per category
  for (const cat of categoryOrder) {
    const items = byCategory[cat] || [];
    if (items.length === 0) continue;
    report += `## Category ${cat}: ${categoryNames[cat]}\n\n`;
    report += `| Layer | Mode | File | Reason |\n|-------|------|------|--------|\n`;
    for (const item of items.slice(0, 200)) {
      report += `| ${item.layer} | ${item.mode} | ${item.file} | ${item.reason?.slice(0, 120)} |\n`;
    }
    if (items.length > 200) report += `\n*... and ${items.length - 200} more*\n`;
    report += "\n";
  }

  writeFileSync(join(outDir, "report.md"), report);
  writeFileSync(join(outDir, "diffs.json"), JSON.stringify(allDiffs, null, 2));

  return report;
}

// ─── Main ────────────────────────────────────────────────────────────────────

async function main() {
  const opts = parseArgs();

  console.log("Per-File VDOM Codegen Comparison");
  console.log("================================");
  console.log(`Project:  ${opts.project}`);
  console.log(`Modes:    ${opts.modes.join(", ")}`);
  console.log(`Layers:   ${opts.layers.join(", ")}`);
  console.log(`Output:   ${opts.out}`);

  if (!existsSync(opts.project)) {
    console.error(`Project does not exist: ${opts.project}`);
    process.exit(1);
  }

  ensureDir(opts.out);

  const vueFiles = findVueFiles(opts.project, opts.filter);
  console.log(`\nFound ${vueFiles.length} .vue files`);

  if (vueFiles.length === 0) {
    console.log("No .vue files. Exiting.");
    return;
  }

  const allDiffs = [];

  // Layer 1: Direct compiler API
  if (opts.layers.includes("direct")) {
    const directDiffs = runDirectLayer(opts, vueFiles, opts.out);
    allDiffs.push(...directDiffs);
  }

  // Layer 2: Vite pipeline
  if (opts.layers.includes("vite")) {
    const viteDiffs = runViteLayer(opts, vueFiles, opts.out);
    allDiffs.push(...viteDiffs);
  }

  // Generate report
  const report = generateReport(allDiffs, opts.out, opts);

  console.log("\n================================");
  console.log("REPORT SUMMARY");
  console.log("================================");

  const byCategory = {};
  for (const d of allDiffs) byCategory[d.category] = (byCategory[d.category] || 0) + 1;
  for (const cat of ["A", "B", "C", "D", "E"]) {
    if (byCategory[cat]) console.log(`  Category ${cat}: ${byCategory[cat]}`);
  }
  console.log(`  Total: ${allDiffs.length}`);
  console.log(`\nReport: ${join(opts.out, "report.md")}`);
  console.log(`Diffs:  ${join(opts.out, "diffs.json")}`);
}

main().catch((err) => {
  console.error("Fatal:", err);
  process.exit(1);
});
