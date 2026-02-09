#!/usr/bin/env node

/**
 * Verter Matrix Compare Orchestrator
 *
 * Runs a strict 4-mode Vue vs Verter matrix on any Vite Vue project:
 *   DEV, PROD, SSR, PROD_SSR
 *
 * For each mode, captures per-component module output from both compilers,
 * invokes the Rust comparator, and generates reports.
 *
 * Usage:
 *   node scripts/verter-compare-matrix.mjs --project <path> [options]
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

// ─── CLI Argument Parsing ────────────────────────────────────────────────────

function parseArgs() {
  const args = process.argv.slice(2);
  const opts = {
    project: null,
    config: null,
    modes: ["dev", "prod", "ssr", "prod_ssr"],
    out: null,
    fixInvalidJs: true,
    maxFixes: Infinity,
    componentFilter: null,
  };

  for (let i = 0; i < args.length; i++) {
    const arg = args[i];
    switch (arg) {
      case "--project":
        opts.project = resolve(args[++i]);
        break;
      case "--config":
        opts.config = args[++i];
        break;
      case "--modes":
        opts.modes = args[++i].split(",").map((m) => m.trim().toLowerCase());
        break;
      case "--out":
        opts.out = resolve(args[++i]);
        break;
      case "--fix-invalid-js":
        opts.fixInvalidJs = args[++i] !== "false";
        break;
      case "--max-fixes":
        opts.maxFixes = parseInt(args[++i], 10);
        break;
      case "--component-filter":
        opts.componentFilter = args[++i];
        break;
      case "--help":
        printUsage();
        process.exit(0);
      default:
        if (!arg.startsWith("--") && !opts.project) {
          opts.project = resolve(arg);
        } else {
          console.error(`Unknown argument: ${arg}`);
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
    opts.out = join(opts.project, ".verter-compare");
  }

  return opts;
}

function printUsage() {
  console.log(`
Usage: node scripts/verter-compare-matrix.mjs --project <path> [options]

Options:
  --project <path>          Target Vite Vue project (required)
  --config <path>           Vite config file (auto-detected if omitted)
  --modes DEV,PROD,SSR,PROD_SSR  Comma-separated modes (default: all four)
  --out <path>              Output directory (default: <project>/.verter-compare)
  --fix-invalid-js <bool>   Auto-fix Category A issues (default: true)
  --max-fixes <n>           Max auto-fixes per run
  --component-filter <glob> Only process matching .vue files
  --help                    Show this help
`);
}

// ─── Mode Configuration ──────────────────────────────────────────────────────

const MODE_CONFIG = {
  dev: { isProduction: false, ssr: false, label: "DEV" },
  prod: { isProduction: true, ssr: false, label: "PROD" },
  ssr: { isProduction: false, ssr: true, label: "SSR" },
  prod_ssr: { isProduction: true, ssr: true, label: "PROD_SSR" },
};

// ─── Utilities ───────────────────────────────────────────────────────────────

function getHash(text) {
  return createHash("sha256").update(text).digest("hex").substring(0, 8);
}

function timestamp() {
  return new Date().toISOString().replace(/[:.]/g, "-").slice(0, 19) + "Z";
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

function ensureDir(dir) {
  mkdirSync(dir, { recursive: true });
}

// ─── Vite Config Resolution ─────────────────────────────────────────────────

function resolveViteConfig(projectDir, configPath) {
  if (configPath) {
    const abs = resolve(projectDir, configPath);
    if (!existsSync(abs)) throw new Error(`Specified Vite config not found: ${abs}`);
    return abs;
  }

  for (const c of ["vite.config.ts", "vite.config.js", "vite.config.mjs", "vite.config.mts"]) {
    const abs = join(projectDir, c);
    if (existsSync(abs)) return abs;
  }

  throw new Error(`No Vite config found in ${projectDir}`);
}

// ─── Preflight Checks ───────────────────────────────────────────────────────

function preflight(opts) {
  if (!existsSync(opts.project)) throw new Error(`Project does not exist: ${opts.project}`);

  const nm = join(opts.project, "node_modules");
  if (!existsSync(nm)) throw new Error(`No node_modules. Run npm install in ${opts.project}`);

  const vuePlugin = join(nm, "@vitejs", "plugin-vue");
  if (!existsSync(vuePlugin)) {
    console.warn("Warning: @vitejs/plugin-vue not found. Vue capture may fail.");
  }

  // Check verter vite-plugin is built
  const verterDist = join(VERTER_ROOT, "packages", "vite-plugin", "dist", "index.js");
  if (!existsSync(verterDist)) {
    throw new Error(`Verter vite-plugin not built. Run 'pnpm run build:ts' in ${VERTER_ROOT}`);
  }
}

// ─── Capture via child process runner ───────────────────────────────────────
// We spawn a node process with cwd=project so vite resolves from project's node_modules.

function captureMode(opts, mode, vueFiles, runDir, compiler) {
  const modeConf = MODE_CONFIG[mode];
  const captureDir = join(runDir, "captures", compiler, mode);
  ensureDir(captureDir);

  const configPath = resolveViteConfig(opts.project, opts.config);
  const isDevLike = mode === "dev" || mode === "ssr";

  // Verter plugin path (absolute, forward slashes for JS)
  const verterPluginPath = join(
    VERTER_ROOT,
    "packages",
    "vite-plugin",
    "dist",
    "index.mjs",
  ).replace(/\\/g, "/");

  // Write a runner script that will execute inside the project's node context
  const runnerPath = join(runDir, `_runner_${compiler}_${mode}.mjs`);

  const vueFilesJson = JSON.stringify(vueFiles);
  const captureDirFwd = captureDir.replace(/\\/g, "/");
  const runDirFwd = runDir.replace(/\\/g, "/");
  const configPathFwd = configPath.replace(/\\/g, "/");

  let runnerCode;
  if (isDevLike) {
    runnerCode = generateDevRunner({
      compiler,
      mode,
      modeConf,
      vueFilesJson,
      captureDirFwd,
      runDirFwd,
      configPathFwd,
      verterPluginPath,
      projectDir: opts.project.replace(/\\/g, "/"),
    });
  } else {
    runnerCode = generateProdRunner({
      compiler,
      mode,
      modeConf,
      vueFilesJson,
      captureDirFwd,
      runDirFwd,
      configPathFwd,
      verterPluginPath,
      projectDir: opts.project.replace(/\\/g, "/"),
    });
  }

  writeFileSync(runnerPath, runnerCode);

  // Execute the runner in the project's directory context
  const result = spawnSync("node", [runnerPath], {
    cwd: opts.project,
    stdio: ["pipe", "pipe", "pipe"],
    timeout: 300_000,
    encoding: "utf-8",
    env: { ...process.env, NODE_OPTIONS: "" },
  });

  // Read the output manifest written by the runner
  const outputPath = join(runDir, `_output_${compiler}_${mode}.json`);
  if (existsSync(outputPath)) {
    try {
      return JSON.parse(readFileSync(outputPath, "utf-8"));
    } catch {
      return {
        entries: [],
        errors: [
          {
            compiler,
            mode,
            source_vue_path: "*",
            block_kind: "runner",
            error: `Failed to parse runner output: ${result.stderr?.slice(0, 500)}`,
          },
        ],
      };
    }
  }

  return {
    entries: [],
    errors: [
      {
        compiler,
        mode,
        source_vue_path: "*",
        block_kind: "runner",
        error: `Runner failed (exit ${result.status}): ${(result.stderr || result.stdout || "").slice(0, 500)}`,
      },
    ],
  };
}

function generateDevRunner({
  compiler,
  mode,
  modeConf,
  vueFilesJson,
  captureDirFwd,
  runDirFwd,
  configPathFwd,
  verterPluginPath,
  projectDir,
}) {
  // Vue runs: use configFile directly (original config already has vue()).
  // Verter runs: use loadConfigFromFile() to get config, strip vite:vue, add verter(), pass inline.
  // This is necessary because Vite's config hook cannot remove already-registered plugins.

  if (compiler === "vue") {
    return `
import { createServer } from 'vite';
import { writeFileSync } from 'fs';
import { createHash } from 'crypto';

const vueFiles = ${vueFilesJson};
const captureDir = '${captureDirFwd}';
const runDir = '${runDirFwd}';
const mode = '${mode}';
const compiler = '${compiler}';
const ssr = ${modeConf.ssr};

function getHash(text) {
  return createHash('sha256').update(text).digest('hex').substring(0, 8);
}

async function main() {
  const entries = [];
  const errors = [];

  try {
    const server = await createServer({
      configFile: '${configPathFwd}',
      root: '${projectDir}',
      server: { middlewareMode: true },
      appType: 'custom',
      logLevel: 'silent',
      optimizeDeps: { noDiscovery: true },
    });

    for (const vuePath of vueFiles) {
      const moduleId = '/' + vuePath;
      try {
        const result = await server.transformRequest(moduleId, { ssr });
        if (result?.code) {
          const hash = getHash(vuePath + mode + 'main');
          const captureFile = captureDir + '/' + hash + '.js';
          writeFileSync(captureFile, result.code);
          entries.push({
            compiler, mode,
            module_key: vuePath + '?vue&type=main',
            source_vue_path: vuePath,
            block_kind: 'main',
            captured_file: captureFile.split(runDir + '/').pop().split('\\\\').join('/'),
          });
        }
      } catch (err) {
        errors.push({
          compiler, mode, source_vue_path: vuePath, block_kind: 'main',
          error: String(err.message || err).slice(0, 500),
        });
      }
    }

    await server.close();
  } catch (err) {
    errors.push({
      compiler, mode, source_vue_path: '*', block_kind: 'server',
      error: 'Server error: ' + String(err.message || err).slice(0, 500),
    });
  }

  writeFileSync('${runDirFwd}/_output_${compiler}_${mode}.json',
    JSON.stringify({ entries, errors }, null, 2));
}

main().catch(err => {
  writeFileSync('${runDirFwd}/_output_${compiler}_${mode}.json',
    JSON.stringify({ entries: [], errors: [{ compiler: '${compiler}', mode: '${mode}', source_vue_path: '*', block_kind: 'fatal', error: String(err.message || err).slice(0, 500) }] }, null, 2));
  process.exit(1);
});
`;
  }

  // Verter runner: load config programmatically, strip vue plugin, add verter
  return `
import { createServer, loadConfigFromFile } from 'vite';
import { writeFileSync } from 'fs';
import { createHash } from 'crypto';

const _verterMod = await import('file:///${verterPluginPath}');
const verter = _verterMod.verter || _verterMod.default;

const vueFiles = ${vueFilesJson};
const captureDir = '${captureDirFwd}';
const runDir = '${runDirFwd}';
const mode = '${mode}';
const compiler = '${compiler}';
const ssr = ${modeConf.ssr};

function getHash(text) {
  return createHash('sha256').update(text).digest('hex').substring(0, 8);
}

async function main() {
  const entries = [];
  const errors = [];

  try {
    // Load the project's vite config programmatically
    const loaded = await loadConfigFromFile(
      { command: 'serve', mode: 'development' },
      '${configPathFwd}',
      '${projectDir}'
    );
    const userConfig = loaded?.config || {};
    const { plugins: origPlugins, ...rest } = userConfig;

    // Strip vite:vue and any verter plugin, add our verter plugin
    const nonVuePlugins = (origPlugins || []).flat().filter(p => {
      const name = p?.name || '';
      return name !== 'vite:vue' && name !== 'vite-plugin-verter';
    });

    const server = await createServer({
      ...rest,
      configFile: false,
      plugins: [verter(), ...nonVuePlugins],
      root: '${projectDir}',
      server: { middlewareMode: true },
      appType: 'custom',
      logLevel: 'silent',
      optimizeDeps: { noDiscovery: true },
    });

    for (const vuePath of vueFiles) {
      const moduleId = '/' + vuePath;
      try {
        const result = await server.transformRequest(moduleId, { ssr });
        if (result?.code) {
          const hash = getHash(vuePath + mode + 'main');
          const captureFile = captureDir + '/' + hash + '.js';
          writeFileSync(captureFile, result.code);
          entries.push({
            compiler, mode,
            module_key: vuePath + '?vue&type=main',
            source_vue_path: vuePath,
            block_kind: 'main',
            captured_file: captureFile.split(runDir + '/').pop().split('\\\\').join('/'),
          });
        }
      } catch (err) {
        errors.push({
          compiler, mode, source_vue_path: vuePath, block_kind: 'main',
          error: String(err.message || err).slice(0, 500),
        });
      }
    }

    await server.close();
  } catch (err) {
    errors.push({
      compiler, mode, source_vue_path: '*', block_kind: 'server',
      error: 'Server error: ' + String(err.message || err).slice(0, 500),
    });
  }

  writeFileSync('${runDirFwd}/_output_${compiler}_${mode}.json',
    JSON.stringify({ entries, errors }, null, 2));
}

main().catch(err => {
  writeFileSync('${runDirFwd}/_output_${compiler}_${mode}.json',
    JSON.stringify({ entries: [], errors: [{ compiler: '${compiler}', mode: '${mode}', source_vue_path: '*', block_kind: 'fatal', error: String(err.message || err).slice(0, 500) }] }, null, 2));
  process.exit(1);
});
`;
}

function generateProdRunner({
  compiler,
  mode,
  modeConf,
  vueFilesJson,
  captureDirFwd,
  runDirFwd,
  configPathFwd,
  verterPluginPath,
  projectDir,
}) {
  const outDir = `${runDirFwd}/build_${compiler}_${mode}`;

  const commonCode = `
import { writeFileSync, readFileSync, readdirSync, existsSync } from 'fs';
import { createHash } from 'crypto';
import { relative, join } from 'path';

const captureDir = '${captureDirFwd}';
const runDir = '${runDirFwd}';
const mode = '${mode}';
const compiler = '${compiler}';
const outDir = '${outDir}';

function getHash(text) {
  return createHash('sha256').update(text).digest('hex').substring(0, 8);
}

function readBuiltFiles(dir) {
  const files = [];
  try {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const fullPath = join(dir, entry.name);
      if (entry.isDirectory()) files.push(...readBuiltFiles(fullPath));
      else if (entry.name.endsWith('.js')) files.push(fullPath);
    }
  } catch {}
  return files;
}
`;

  const buildOpts = `{
      minify: false,
      ${modeConf.ssr ? "ssr: true," : ""}
      outDir,
      rollupOptions: {
        output: {
          format: 'es',
          entryFileNames: '[name].js',
          chunkFileNames: '[name].js',
        },
      },
    }`;

  const collectCode = `
    if (existsSync(outDir)) {
      const builtFiles = readBuiltFiles(outDir);
      for (const builtFile of builtFiles) {
        const code = readFileSync(builtFile, 'utf-8');
        const relName = relative(outDir, builtFile).split('\\\\').join('/');
        const hash = getHash(relName + mode + compiler);
        const captureFile = captureDir + '/' + hash + '.js';
        writeFileSync(captureFile, code);
        entries.push({
          compiler, mode,
          module_key: relName,
          source_vue_path: relName,
          block_kind: 'bundle',
          captured_file: captureFile.split(runDir + '/').pop().split('\\\\').join('/'),
        });
      }
    }`;

  const writeOutput = `
  writeFileSync('${runDirFwd}/_output_${compiler}_${mode}.json',
    JSON.stringify({ entries, errors }, null, 2));`;

  const catchBlock = `
main().catch(err => {
  writeFileSync('${runDirFwd}/_output_${compiler}_${mode}.json',
    JSON.stringify({ entries: [], errors: [{ compiler: '${compiler}', mode: '${mode}', source_vue_path: '*', block_kind: 'fatal', error: String(err.message || err).slice(0, 500) }] }, null, 2));
  process.exit(1);
});`;

  if (compiler === "vue") {
    return `
import { build } from 'vite';
${commonCode}

async function main() {
  const entries = [];
  const errors = [];

  try {
    await build({
      configFile: '${configPathFwd}',
      root: '${projectDir}',
      logLevel: 'silent',
      build: ${buildOpts},
    });
${collectCode}
  } catch (err) {
    errors.push({
      compiler, mode, source_vue_path: '*', block_kind: 'build',
      error: 'Build error: ' + String(err.message || err).slice(0, 500),
    });
  }
${writeOutput}
}
${catchBlock}
`;
  }

  // Verter: load config, strip vue plugin, add verter
  return `
import { build, loadConfigFromFile } from 'vite';
${commonCode}

const _verterMod = await import('file:///${verterPluginPath}');
const verter = _verterMod.verter || _verterMod.default;

async function main() {
  const entries = [];
  const errors = [];

  try {
    const loaded = await loadConfigFromFile(
      { command: 'build', mode: 'production' },
      '${configPathFwd}',
      '${projectDir}'
    );
    const userConfig = loaded?.config || {};
    const { plugins: origPlugins, ...rest } = userConfig;

    const nonVuePlugins = (origPlugins || []).flat().filter(p => {
      const name = p?.name || '';
      return name !== 'vite:vue' && name !== 'vite-plugin-verter';
    });

    await build({
      ...rest,
      configFile: false,
      plugins: [verter(), ...nonVuePlugins],
      root: '${projectDir}',
      logLevel: 'silent',
      build: { ...(rest.build || {}), ...${buildOpts} },
    });
${collectCode}
  } catch (err) {
    errors.push({
      compiler, mode, source_vue_path: '*', block_kind: 'build',
      error: 'Build error: ' + String(err.message || err).slice(0, 500),
    });
  }
${writeOutput}
}
${catchBlock}
`;
}

// ─── Rust Comparator Invocation ─────────────────────────────────────────────

function runRustComparator(manifestPath, runDir) {
  console.log(`  Running Rust comparator...`);
  try {
    const result = spawnSync(
      "cargo",
      ["run", "-p", "verter_core", "--example", "check_matrix", "--", "--manifest", manifestPath],
      {
        cwd: VERTER_ROOT,
        stdio: ["pipe", "pipe", "pipe"],
        timeout: 300_000,
        encoding: "utf-8",
      },
    );

    if (result.status !== 0) {
      console.error(`  Comparator failed (exit ${result.status}): ${result.stderr?.slice(0, 500)}`);
      return null;
    }

    const diffsPath = join(runDir, "diffs.jsonl");
    if (existsSync(diffsPath)) {
      return readFileSync(diffsPath, "utf-8")
        .split("\n")
        .filter(Boolean)
        .map((l) => JSON.parse(l));
    }
    return [];
  } catch (err) {
    console.error(`  Comparator error: ${err.message}`);
    return null;
  }
}

// ─── Report Generation ──────────────────────────────────────────────────────

function generateReports(diffs, runDir, outRoot) {
  const diffsByCategory = {};
  for (const d of diffs) {
    const cat = d.category || "unknown";
    if (!diffsByCategory[cat]) diffsByCategory[cat] = [];
    diffsByCategory[cat].push(d);
  }

  const categoryNames = {
    A: "Invalid JS (P0)",
    B: "Missing Module (P1)",
    C: "AST Structure (P2)",
    D: "Wrong Values/Imports (P2)",
    E: "Cosmetic/Known Limitation (TRACKED)",
  };

  const categoryOrder = ["A", "B", "C", "D", "E"];

  // Differences report
  let differencesReport = `# Verter Differences Report\n\n`;
  differencesReport += `**Run:** ${basename(runDir)}\n`;
  differencesReport += `**Total Differences:** ${diffs.length}\n\n`;

  for (const cat of categoryOrder) {
    const items = diffsByCategory[cat] || [];
    if (items.length === 0) continue;
    differencesReport += `## Category ${cat}: ${categoryNames[cat] || cat}\n\n`;
    differencesReport += `| Mode | Source | Reason |\n|------|--------|--------|\n`;
    for (const item of items) {
      differencesReport += `| ${item.mode} | ${item.source_vue_path} | ${item.reason?.slice(0, 100) || "n/a"} |\n`;
    }
    differencesReport += "\n";
  }

  // Summary report
  let summaryReport = `# Verter Comparison Summary\n\n`;
  summaryReport += `**Run:** ${basename(runDir)}\n\n`;
  summaryReport += `| Category | Count | Severity |\n|----------|-------|----------|\n`;
  for (const cat of categoryOrder) {
    const items = diffsByCategory[cat] || [];
    const sev = cat === "A" ? "P0" : cat === "B" ? "P1" : cat <= "D" ? "P2" : "TRACKED";
    summaryReport += `| ${cat}: ${categoryNames[cat] || cat} | ${items.length} | ${sev} |\n`;
  }
  summaryReport += `\n**Total:** ${diffs.length} differences\n`;

  writeFileSync(join(runDir, "verter_differences.md"), differencesReport);
  writeFileSync(join(runDir, "verter_summary.md"), summaryReport);
  writeFileSync(join(outRoot, "verter_differences.md"), differencesReport);
  writeFileSync(join(outRoot, "verter_summary.md"), summaryReport);

  const diffsPath = join(runDir, "diffs.jsonl");
  if (existsSync(diffsPath)) {
    writeFileSync(join(outRoot, "diffs.jsonl"), readFileSync(diffsPath, "utf-8"));
  }

  writeFileSync(
    join(outRoot, "latest_run.json"),
    JSON.stringify(
      {
        run_id: basename(runDir),
        run_dir: runDir,
        timestamp: new Date().toISOString(),
      },
      null,
      2,
    ),
  );

  return { differencesReport, summaryReport };
}

// ─── Category A Fix Queue ───────────────────────────────────────────────────

function generateFixQueue(diffs, runDir, maxFixes) {
  const catA = diffs.filter((d) => d.category === "A");
  const queue = catA.slice(0, maxFixes).map((d, i) => ({
    index: i,
    mode: d.mode,
    module_key: d.module_key,
    source_vue_path: d.source_vue_path,
    reason: d.reason,
    vue_file: d.vue_file,
    verter_file: d.verter_file,
    recommended_test: d.recommended_test || `e2e parity test for ${d.source_vue_path} (${d.mode})`,
    suspected_files: d.suspected_files || [],
    status: "pending",
  }));
  writeFileSync(join(runDir, "invalid_js_queue.json"), JSON.stringify(queue, null, 2));
  return queue;
}

// ─── JS-based Comparator (fallback) ─────────────────────────────────────────

function runJsComparator(manifest, runDir) {
  const diffs = [];
  const byKey = new Map();
  for (const entry of manifest.entries) {
    const key = `${entry.mode}:${entry.source_vue_path}:${entry.block_kind}`;
    if (!byKey.has(key)) byKey.set(key, {});
    byKey.get(key)[entry.compiler] = entry;
  }

  for (const [key, compilers] of byKey) {
    const vueEntry = compilers.vue;
    const verterEntry = compilers.verter;

    if (!vueEntry || !verterEntry) {
      const missing = !verterEntry ? "verter" : "vue";
      const present = compilers.vue || compilers.verter;
      diffs.push({
        id: key,
        mode: present.mode,
        category: "B",
        severity: "P1",
        module_key: present.module_key || present.source_vue_path,
        source_vue_path: present.source_vue_path,
        vue_file: vueEntry?.captured_file || null,
        verter_file: verterEntry?.captured_file || null,
        reason: `Missing ${missing} output for ${key}`,
        recommended_test: `Add e2e parity test for ${present.source_vue_path}`,
        suspected_files: [],
      });
      continue;
    }

    const vuePath = join(runDir, vueEntry.captured_file);
    const verterPath = join(runDir, verterEntry.captured_file);
    if (!existsSync(vuePath) || !existsSync(verterPath)) continue;

    const vueCode = readFileSync(vuePath, "utf-8");
    const verterCode = readFileSync(verterPath, "utf-8");

    const verterParse = tryParseJs(verterCode);
    if (!verterParse.valid) {
      diffs.push({
        id: key,
        mode: verterEntry.mode,
        category: "A",
        severity: "P0",
        module_key: verterEntry.module_key,
        source_vue_path: verterEntry.source_vue_path,
        vue_file: vueEntry.captured_file,
        verter_file: verterEntry.captured_file,
        reason: `Verter JS parse failure: ${verterParse.error?.slice(0, 200)}`,
        recommended_test: `Add e2e parity test in codegen.rs`,
        suspected_files: ["crates/verter_core/src/codegen/vue/template/element.rs"],
      });
      continue;
    }

    if (!tryParseJs(vueCode).valid) continue;

    if (normalizeJs(vueCode) === normalizeJs(verterCode)) continue;

    const category = classifyDifference(vueCode, verterCode);
    diffs.push({
      id: key,
      mode: verterEntry.mode,
      category,
      severity:
        category <= "B" ? (category === "A" ? "P0" : "P1") : category <= "D" ? "P2" : "TRACKED",
      module_key: verterEntry.module_key,
      source_vue_path: verterEntry.source_vue_path,
      vue_file: vueEntry.captured_file,
      verter_file: verterEntry.captured_file,
      reason: `Output differs (${category})`,
      recommended_test: `Add e2e parity test for ${verterEntry.source_vue_path}`,
      suspected_files: [],
    });
  }
  return diffs;
}

function tryParseJs(code) {
  try {
    let braces = 0,
      parens = 0,
      brackets = 0;
    let inString = false,
      stringChar = "",
      escaped = false;
    for (const ch of code) {
      if (escaped) {
        escaped = false;
        continue;
      }
      if (ch === "\\") {
        escaped = true;
        continue;
      }
      if (inString) {
        if (ch === stringChar) inString = false;
        continue;
      }
      if (ch === '"' || ch === "'" || ch === "`") {
        inString = true;
        stringChar = ch;
        continue;
      }
      if (ch === "{") braces++;
      else if (ch === "}") braces--;
      else if (ch === "(") parens++;
      else if (ch === ")") parens--;
      else if (ch === "[") brackets++;
      else if (ch === "]") brackets--;
      if (braces < 0 || parens < 0 || brackets < 0)
        return { valid: false, error: `Unbalanced '${ch}'` };
    }
    if (braces !== 0) return { valid: false, error: `Unbalanced braces: ${braces}` };
    if (parens !== 0) return { valid: false, error: `Unbalanced parens: ${parens}` };
    if (brackets !== 0) return { valid: false, error: `Unbalanced brackets: ${brackets}` };
    if (code.includes("_ctx.{")) return { valid: false, error: "_ctx.{ on object literal" };
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

function classifyDifference(vueCode, verterCode) {
  const vueImports = extractImports(vueCode);
  const verterImports = extractImports(verterCode);
  const vueSet = new Set(vueImports.map((i) => i.source));
  const verterSet = new Set(verterImports.map((i) => i.source));
  if ([...vueSet].some((s) => !verterSet.has(s)) || [...verterSet].some((s) => !vueSet.has(s)))
    return "D";
  const vueFn = (vueCode.match(/function\s/g) || []).length;
  const verterFn = (verterCode.match(/function\s/g) || []).length;
  if (Math.abs(vueFn - verterFn) > 2) return "C";
  return "E";
}

function extractImports(code) {
  const imports = [];
  const re =
    /import\s+(?:{[^}]+}|\*\s+as\s+\w+|\w+)?\s*(?:,\s*(?:{[^}]+}|\*\s+as\s+\w+))?\s*from\s+['"]([^'"]+)['"]/g;
  let m;
  while ((m = re.exec(code)) !== null) imports.push({ source: m[1] });
  return imports;
}

// ─── Main Orchestrator ──────────────────────────────────────────────────────

async function main() {
  const opts = parseArgs();

  console.log("Verter Matrix Compare");
  console.log("=====================");
  console.log(`Project:  ${opts.project}`);
  console.log(`Modes:    ${opts.modes.join(", ")}`);
  console.log(`Output:   ${opts.out}`);
  console.log("");

  preflight(opts);

  const runId = timestamp();
  const runDir = join(opts.out, "runs", runId);
  ensureDir(runDir);
  ensureDir(join(runDir, "logs"));

  const vueFiles = findVueFiles(opts.project, opts.componentFilter);
  console.log(`Found ${vueFiles.length} .vue files`);
  if (vueFiles.length === 0) {
    console.log("No .vue files. Exiting.");
    return;
  }

  const runState = {
    schema: "verter.run_state.v1",
    run_id: runId,
    project_root: opts.project,
    modes: opts.modes,
    status: "in_progress",
    stages_completed: [],
    vue_file_count: vueFiles.length,
    started_at: new Date().toISOString(),
  };
  writeFileSync(join(runDir, "run_state.json"), JSON.stringify(runState, null, 2));

  const manifest = {
    schema: "verter.capture_manifest.v1",
    run_id: runId,
    project_root: opts.project,
    entries: [],
    errors: [],
  };

  // ─── Capture Phase ─────────────────────────────────────────────────
  for (const mode of opts.modes) {
    for (const compiler of ["vue", "verter"]) {
      console.log(`\nCapturing: ${compiler} / ${mode}...`);
      const start = Date.now();

      const result = captureMode(opts, mode, vueFiles, runDir, compiler);
      manifest.entries.push(...result.entries);
      manifest.errors.push(...result.errors);

      const elapsed = ((Date.now() - start) / 1000).toFixed(1);
      console.log(
        `  ${result.entries.length} modules, ${result.errors.length} errors (${elapsed}s)`,
      );
    }
    runState.stages_completed.push(`capture_${mode}`);
    writeFileSync(join(runDir, "run_state.json"), JSON.stringify(runState, null, 2));
  }

  const manifestPath = join(runDir, "capture_manifest.json");
  writeFileSync(manifestPath, JSON.stringify(manifest, null, 2));
  console.log(`\nManifest: ${manifest.entries.length} entries, ${manifest.errors.length} errors`);

  // ─── Comparison Phase ──────────────────────────────────────────────
  console.log("\nRunning comparator...");
  let diffs = runRustComparator(manifestPath, runDir);

  if (diffs === null) {
    console.log("  Rust comparator unavailable. Using JS fallback...");
    diffs = runJsComparator(manifest, runDir);
    writeFileSync(join(runDir, "diffs.jsonl"), diffs.map((d) => JSON.stringify(d)).join("\n"));
  }

  runState.stages_completed.push("compare");

  // ─── Report Phase ─────────────────────────────────────────────────
  generateReports(diffs, runDir, opts.out);
  console.log("\nReports:");
  console.log(`  ${join(opts.out, "verter_differences.md")}`);
  console.log(`  ${join(opts.out, "verter_summary.md")}`);

  if (opts.fixInvalidJs) {
    const queue = generateFixQueue(diffs, runDir, opts.maxFixes);
    if (queue.length > 0) {
      console.log(`\nCategory A fix queue: ${queue.length} items`);
    }
  }

  runState.status = "completed";
  runState.completed_at = new Date().toISOString();
  writeFileSync(join(runDir, "run_state.json"), JSON.stringify(runState, null, 2));
  console.log("\nDone!");
}

main().catch((err) => {
  console.error("Fatal:", err);
  process.exit(1);
});
