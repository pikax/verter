/**
 * Repo-owned parent runner for corpus audit sweeps.
 *
 * Runs each Vue component in an isolated child process with a hard
 * timeout enforced by SIGKILL from the parent. Each child emits BOTH
 * a `RustAuditRecord` JSON (`<sanitized>.audit.json`) and a
 * `ComponentMetaAnalysis` JSON (`<sanitized>.analysis.json`) via the
 * NAPI `getComponentMetaWithAudit` binding. Results are written as a
 * structured JSON summary.
 *
 * Plan §3 Commit 10 (F8). Replaces the legacy trace+regex-validator
 * flow: the emitted audit bundles are the sole correctness authority,
 * and the per-component analyzer is
 * [`audit-validator.ts`](../../packages/benchmark/src/audit-validator.ts)
 * — the legacy `trace-validator.ts`, `trace-check.ts`, and
 * `trace-specs/component-meta/*.json` files are deleted.
 *
 * Usage:
 *   node scripts/benchmark/trace-component-corpus.mjs \
 *     --ui-root=.integration-tests/repos/nuxt-ui \
 *     --output-dir=tmp/corpus-audit \
 *     --timeout-ms=30000
 *
 * Each component is run via `_audit-component.ts` in a child process.
 * The parent owns the timeout — the child does NOT use Promise.race.
 */

import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, relative, resolve } from "node:path";
import { spawn } from "node:child_process";
import { createWriteStream } from "node:fs";
import { performance } from "node:perf_hooks";
import { fileURLToPath, pathToFileURL } from "node:url";
import { createRequire } from "node:module";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, "../..");
const benchmarkRequire = createRequire(
  pathToFileURL(resolve(repoRoot, "packages", "benchmark", "package.json")).href,
);

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const DEFAULT_TIMEOUT_MS = 30_000;
const DEFAULT_UI_ROOT = resolve(repoRoot, ".integration-tests", "repos", "nuxt-ui");
const DEFAULT_OUTPUT_DIR = resolve(repoRoot, "tmp", "corpus-audit");

function parseArgs(argv) {
  const config = {
    uiRoot: DEFAULT_UI_ROOT,
    outputDir: DEFAULT_OUTPUT_DIR,
    timeoutMs: DEFAULT_TIMEOUT_MS,
    filter: null,
    concurrency: 1,
  };

  for (const arg of argv) {
    if (arg.startsWith("--ui-root=")) {
      config.uiRoot = resolve(arg.slice("--ui-root=".length));
    } else if (arg.startsWith("--output-dir=")) {
      config.outputDir = resolve(arg.slice("--output-dir=".length));
    } else if (arg.startsWith("--timeout-ms=")) {
      config.timeoutMs = Number.parseInt(arg.slice("--timeout-ms=".length), 10);
    } else if (arg.startsWith("--filter=")) {
      config.filter = arg.slice("--filter=".length);
    }
  }

  return config;
}

// ---------------------------------------------------------------------------
// Component discovery
// ---------------------------------------------------------------------------

function discoverVueFiles(rootDir) {
  const files = [];
  for (const entry of readdirSync(rootDir, { withFileTypes: true })) {
    const absolutePath = resolve(rootDir, entry.name);
    if (entry.isDirectory()) {
      files.push(...discoverVueFiles(absolutePath));
    } else if (entry.isFile() && entry.name.endsWith(".vue")) {
      files.push(absolutePath);
    }
  }
  return files;
}

// ---------------------------------------------------------------------------
// Process tree killing
// ---------------------------------------------------------------------------

function killWindowsProcessTree(pid) {
  if (!pid) return;
  const killer = spawn("taskkill", ["/PID", String(pid), "/T", "/F"], {
    stdio: "ignore",
    windowsHide: true,
    detached: true,
  });
  killer.unref();
}

function killProcessTree(pid) {
  if (!pid) return;
  if (process.platform === "win32") {
    killWindowsProcessTree(pid);
    return;
  }
  try {
    process.kill(-pid, "SIGKILL");
  } catch {
    try {
      process.kill(pid, "SIGKILL");
    } catch {
      // Process already gone.
    }
  }
}

// ---------------------------------------------------------------------------
// Stdout parsing
// ---------------------------------------------------------------------------

function parseStdoutFields(stdout) {
  const doneMatch = stdout.match(/^Done in (\d+)ms/m);
  const sawDoneLine = doneMatch !== null;
  const sawClosedLine = /^Closed\b/m.test(stdout);
  return {
    queryMsFromStdout: doneMatch ? Number.parseInt(doneMatch[1], 10) : null,
    sawDoneLine,
    sawClosedLine,
  };
}

function classifyExitStatus({ exitCode, signal, timedOut, sawDoneLine }) {
  if (timedOut) {
    return sawDoneLine ? "close_timeout" : "query_timeout";
  }
  if (signal) return "external_signal";
  if (exitCode === 0 && sawDoneLine) return "ok";
  return "crash";
}

// ---------------------------------------------------------------------------
// Per-component filename sanitization
// ---------------------------------------------------------------------------

function sanitizePathComponent(component) {
  return component.replace(/[/\\]/g, "__").replace(/\.vue$/, "__vue");
}

// ---------------------------------------------------------------------------
// Run one component in isolation
// ---------------------------------------------------------------------------

async function runComponent(componentRelPath, componentToken, config) {
  const sanitized = sanitizePathComponent(componentRelPath);
  const stdoutPath = resolve(config.outputDir, "stdout", `${sanitized}.stdout.txt`);
  const stderrPath = resolve(config.outputDir, "stderr", `${sanitized}.stderr.txt`);
  const auditPath = resolve(config.outputDir, "audit", `${sanitized}.audit.json`);
  const analysisPath = resolve(config.outputDir, "analysis", `${sanitized}.analysis.json`);
  const resultPath = resolve(config.outputDir, "results", `${componentRelPath}.json`);

  mkdirSync(dirname(stdoutPath), { recursive: true });
  mkdirSync(dirname(stderrPath), { recursive: true });
  mkdirSync(dirname(auditPath), { recursive: true });
  mkdirSync(dirname(analysisPath), { recursive: true });
  mkdirSync(dirname(resultPath), { recursive: true });

  // Plan §3 Commit 10: audit-only worker. The legacy
  // `_trace-component.ts` worker used the compat checker (no audit
  // data) — the new `_audit-component.ts` worker drives the NAPI
  // `getComponentMetaWithAudit` binding directly.
  const auditWorkerPath = resolve(repoRoot, "packages", "benchmark", "src", "_audit-component.ts");
  const tsxLoaderPath = pathToFileURL(benchmarkRequire.resolve("tsx")).href;

  const env = {
    ...process.env,
    FORCE_COLOR: "0",
    VERTER_COMPONENT_META_AUDIT_PATH: auditPath,
    VERTER_COMPONENT_META_ANALYSIS_PATH: analysisPath,
    VERTER_COMPONENT_META_RESULT_PATH: resultPath,
  };

  const stdoutStream = createWriteStream(stdoutPath);
  const stderrStream = createWriteStream(stderrPath);
  const stdoutChunks = [];

  const startMs = performance.now();

  const child = spawn(
    process.execPath,
    ["--expose-gc", "--import", tsxLoaderPath, auditWorkerPath, componentToken],
    {
      cwd: repoRoot,
      shell: false,
      detached: process.platform !== "win32",
      windowsHide: true,
      stdio: ["ignore", "pipe", "pipe"],
      env,
    },
  );

  child.stdout?.setEncoding("utf8");
  child.stdout?.on("data", (chunk) => {
    stdoutChunks.push(chunk);
    stdoutStream.write(chunk);
  });
  child.stderr?.on("data", (chunk) => {
    stderrStream.write(chunk);
  });

  let timedOut = false;
  let childClosed = false;
  let windowsTreeKillFallback = null;
  const timer = setTimeout(() => {
    timedOut = true;
    if (process.platform === "win32") {
      try {
        if (child.pid) {
          process.kill(child.pid, "SIGKILL");
        }
      } catch {
        killWindowsProcessTree(child.pid);
        return;
      }
      windowsTreeKillFallback = setTimeout(() => {
        if (!childClosed) {
          killWindowsProcessTree(child.pid);
        }
      }, 250);
      windowsTreeKillFallback.unref();
      return;
    }
    killProcessTree(child.pid);
  }, config.timeoutMs);
  timer.unref();

  const { exitCode, signal } = await new Promise((resolvePromise) => {
    child.once("error", () => resolvePromise({ exitCode: 1, signal: null }));
    child.once("close", (code, sig) => {
      childClosed = true;
      if (windowsTreeKillFallback) {
        clearTimeout(windowsTreeKillFallback);
      }
      resolvePromise({ exitCode: code, signal: sig });
    });
  });

  clearTimeout(timer);
  stdoutStream.end();
  stderrStream.end();

  const wallMs = performance.now() - startMs;
  const fullStdout = stdoutChunks.join("");
  const stdoutFields = parseStdoutFields(fullStdout);

  const status = classifyExitStatus({
    exitCode,
    signal,
    timedOut,
    sawDoneLine: stdoutFields.sawDoneLine,
    sawClosedLine: stdoutFields.sawClosedLine,
  });

  return {
    component: componentRelPath,
    status,
    wall_ms: Math.round(wallMs),
    query_ms_from_stdout: stdoutFields.queryMsFromStdout,
    exit_code: exitCode,
    signal,
    stdout_path: stdoutPath,
    stderr_path: stderrPath,
    audit_path: auditPath,
    analysis_path: analysisPath,
    result_path: resultPath,
    audit_emitted: existsSync(auditPath),
    analysis_emitted: existsSync(analysisPath),
    saw_done_line: stdoutFields.sawDoneLine,
    saw_closed_line: stdoutFields.sawClosedLine,
  };
}

// ---------------------------------------------------------------------------
// Audit validation
// ---------------------------------------------------------------------------

/**
 * Invoke the TS-side `audit-validator.ts` on each emitted audit +
 * analysis pair. Plan §3 Commit 10 — the audit-validator is the sole
 * correctness authority after the regex validator's deletion.
 *
 * The validator is loaded via `tsx` from the benchmark package; specs
 * live under `packages/benchmark/audit-specs/component-meta/` (a
 * curated subset matching the Commit 7 authored correctness
 * fixtures). Components without a matching spec are skipped.
 */
async function validateEmissions(results, config) {
  const specDir = resolve(repoRoot, "packages", "benchmark", "audit-specs", "component-meta");
  if (!existsSync(specDir)) {
    console.error(`No audit-specs directory at ${specDir}; skipping validation.`);
    return [];
  }

  const specFiles = new Set(
    readdirSync(specDir)
      .filter((n) => n.endsWith(".json"))
      .map((n) => n.replace(/\.json$/, "")),
  );

  // Load the TS validator via tsx-backed dynamic import. This keeps
  // the TS module compilation consistent with the worker side — the
  // .mjs never bypasses the repo's TS configuration.
  const validatorUrl = pathToFileURL(
    resolve(repoRoot, "packages", "benchmark", "src", "audit-validator.ts"),
  ).href;
  /** @type {typeof import("../../packages/benchmark/src/audit-validator.js")} */
  // eslint-disable-next-line no-unused-vars
  let validatorModule;
  try {
    validatorModule = await import(validatorUrl);
  } catch (err) {
    console.error(`FATAL: failed to load audit-validator.ts: ${err}`);
    return [];
  }
  const { validateAuditBundle } = validatorModule;

  const outputs = [];
  for (const r of results) {
    if (!r.audit_emitted || !r.analysis_emitted) continue;
    const baseName = r.component.replace(/^.*\//, "").replace(/\.vue$/, "");
    if (!specFiles.has(baseName)) continue;
    const specPath = resolve(specDir, `${baseName}.json`);
    const spec = JSON.parse(readFileSync(specPath, "utf-8"));
    const bundle = {
      analysis: JSON.parse(readFileSync(r.analysis_path, "utf-8")),
      resolution: null,
      record: JSON.parse(readFileSync(r.audit_path, "utf-8")),
    };
    const validation = validateAuditBundle(bundle, spec);
    outputs.push({
      component: r.component,
      spec: baseName,
      passed: validation.passed,
      violations: validation.violations,
    });
    if (!validation.passed) {
      console.error(`  [audit-validator] ${r.component} FAILED:`);
      for (const v of validation.violations) console.error(`    - ${v}`);
    }
  }
  void config;
  return outputs;
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

export async function main() {
  const config = parseArgs(process.argv.slice(2));

  const componentsDir = resolve(config.uiRoot, "src", "runtime", "components");
  if (!existsSync(componentsDir)) {
    console.error(`FATAL: components directory not found at ${componentsDir}`);
    console.error("Run: pnpm bench:meta:ui:setup");
    process.exit(1);
  }

  let vueFiles = discoverVueFiles(componentsDir).sort();

  if (config.filter) {
    vueFiles = vueFiles.filter((f) => f.includes(config.filter));
  }

  console.error(`Discovered ${vueFiles.length} Vue components`);
  console.error(`Timeout: ${config.timeoutMs}ms per component`);
  console.error(`Output: ${config.outputDir}`);
  console.error("Mode: audit-only (plan §3 Commit 10)");

  mkdirSync(config.outputDir, { recursive: true });

  const results = [];
  let okCount = 0;
  let failCount = 0;

  for (let i = 0; i < vueFiles.length; i++) {
    const absolutePath = vueFiles[i];
    const relPath = relative(config.uiRoot, absolutePath).replace(/\\/g, "/");
    console.error(`  [${i + 1}/${vueFiles.length}] ${relPath}...`);

    const result = await runComponent(relPath, absolutePath, config);
    results.push(result);

    if (result.status === "ok") {
      okCount++;
      console.error(
        `    ${result.status} (${result.wall_ms}ms wall, ${result.query_ms_from_stdout ?? "?"}ms query, audit=${result.audit_emitted})`,
      );
    } else {
      failCount++;
      console.error(
        `    ${result.status} (${result.wall_ms}ms wall, exit=${result.exit_code}, signal=${result.signal})`,
      );
    }
  }

  // Run audit-validator over the emissions.
  const validations = await validateEmissions(results, config);
  const validationFailures = validations.filter((v) => !v.passed).length;

  const summary = {
    generated_at: Date.now(),
    plan_reference: "§3 Commit 10 (F8) — audit-only corpus sweep",
    config: {
      ui_root: config.uiRoot,
      timeout_ms: config.timeoutMs,
    },
    totals: {
      discovered: vueFiles.length,
      ok: okCount,
      failed: failCount,
      audit_emitted: results.filter((r) => r.audit_emitted).length,
      analysis_emitted: results.filter((r) => r.analysis_emitted).length,
      validated: validations.length,
      validation_failures: validationFailures,
    },
    results,
    validations,
    analyzer: "audit-validator",
  };

  const summaryPath = resolve(config.outputDir, "summary.json");
  writeFileSync(summaryPath, JSON.stringify(summary, null, 2));

  console.error(
    `\nDone: ${okCount}/${vueFiles.length} ok, ${failCount} failed, ${validationFailures} validation failures`,
  );
  console.error(`Summary: ${summaryPath}`);

  // Also write to stdout for piping.
  console.log(JSON.stringify(summary, null, 2));

  process.exit(failCount > 0 || validationFailures > 0 ? 1 : 0);
}

if (process.argv[1] && resolve(process.argv[1]) === fileURLToPath(import.meta.url)) {
  main().catch((err) => {
    console.error("FATAL:", err);
    process.exit(2);
  });
}
