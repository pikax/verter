/**
 * Repo-owned parent runner for corpus trace sweeps.
 *
 * Runs each Vue component in an isolated child process with a hard timeout
 * enforced by SIGKILL from the parent. Results are written as structured JSON.
 *
 * Usage:
 *   node scripts/benchmark/trace-component-corpus.mjs \
 *     --ui-root=.integration-tests/repos/nuxt-ui \
 *     --output-dir=tmp/corpus-trace \
 *     --timeout-ms=30000
 *
 * Each component is run via _trace-component.ts in a child process.
 * The parent owns the timeout — the child does NOT use Promise.race.
 */

import { existsSync, mkdirSync, readdirSync, readFileSync, writeFileSync } from "node:fs";
import { dirname, join, relative, resolve } from "node:path";
import { spawn, spawnSync } from "node:child_process";
import { createWriteStream } from "node:fs";
import { performance } from "node:perf_hooks";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { pathToFileURL } from "node:url";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, "../..");

// ---------------------------------------------------------------------------
// Configuration
// ---------------------------------------------------------------------------

const DEFAULT_TIMEOUT_MS = 30_000;
const DEFAULT_UI_ROOT = resolve(repoRoot, ".integration-tests", "repos", "nuxt-ui");
const DEFAULT_OUTPUT_DIR = resolve(repoRoot, "tmp", "corpus-trace");

function parseArgs(argv) {
  const config = {
    uiRoot: DEFAULT_UI_ROOT,
    outputDir: DEFAULT_OUTPUT_DIR,
    timeoutMs: DEFAULT_TIMEOUT_MS,
    filter: null,
    traceEnabled: true,
    jsAudit: false,
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
    } else if (arg === "--js-audit") {
      config.jsAudit = true;
    } else if (arg === "--no-trace") {
      config.traceEnabled = false;
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
// Process tree killing (same as run-hard-timeout.mjs)
// ---------------------------------------------------------------------------

function killProcessTree(pid) {
  if (!pid) return;
  if (process.platform === "win32") {
    spawnSync("taskkill", ["/PID", String(pid), "/T", "/F"], {
      stdio: "ignore",
      windowsHide: true,
    });
    return;
  }
  try {
    process.kill(-pid, "SIGKILL");
  } catch {
    try {
      process.kill(pid, "SIGKILL");
    } catch {}
  }
}

// ---------------------------------------------------------------------------
// Stdout parsing
// ---------------------------------------------------------------------------

function parseStdoutFields(stdout) {
  const doneMatch = stdout.match(/^Done in (\d+)ms/m);
  const sawDoneLine = doneMatch !== null;
  const sawClosedLine = /^Closed /m.test(stdout);
  return {
    queryMsFromStdout: doneMatch ? Number.parseInt(doneMatch[1], 10) : null,
    sawDoneLine,
    sawClosedLine,
  };
}

function classifyExitStatus({ exitCode, signal, timedOut, sawDoneLine, sawClosedLine }) {
  if (timedOut) {
    return sawDoneLine ? "close_timeout" : "query_timeout";
  }
  if (signal) return "external_signal";
  if (exitCode === 0 && sawDoneLine) return "ok";
  return "crash";
}

// ---------------------------------------------------------------------------
// Parse trace log for resolve_component_meta duration
// ---------------------------------------------------------------------------

function parseTraceResolveMs(tracePath) {
  if (!existsSync(tracePath)) return null;
  try {
    const content = readFileSync(tracePath, "utf8");
    const match = content.match(/name="resolve_component_meta".*dur_ms=([0-9.]+)/);
    return match ? Number.parseFloat(match[1]) : null;
  } catch {
    return null;
  }
}

// ---------------------------------------------------------------------------
// Run one component in isolation
// ---------------------------------------------------------------------------

function sanitizePathComponent(component) {
  return component.replace(/[/\\]/g, "__").replace(/\.vue$/, "__vue");
}

async function runComponent(componentRelPath, componentToken, config) {
  const sanitized = sanitizePathComponent(componentRelPath);
  const stdoutPath = resolve(config.outputDir, "stdout", `${sanitized}.stdout.txt`);
  const stderrPath = resolve(config.outputDir, "stderr", `${sanitized}.stderr.txt`);
  const tracePath = resolve(config.outputDir, "traces", `${sanitized}.trace.log`);

  mkdirSync(dirname(stdoutPath), { recursive: true });
  mkdirSync(dirname(stderrPath), { recursive: true });
  mkdirSync(dirname(tracePath), { recursive: true });

  const traceComponentPath = resolve(
    repoRoot,
    "packages",
    "benchmark",
    "src",
    "_trace-component.ts",
  );
  const tsxLoaderPath = pathToFileURL(createRequire(import.meta.url).resolve("tsx")).href;

  const env = {
    ...process.env,
    FORCE_COLOR: "0",
    ...(config.jsAudit ? { VERTER_JS_AUDIT: "1" } : {}),
  };
  if (config.traceEnabled) {
    env.VERTER_COMPONENT_META_TRACE = "1";
    env.VERTER_COMPONENT_META_TRACE_PATH = tracePath;
  }

  const stdoutStream = createWriteStream(stdoutPath);
  const stderrStream = createWriteStream(stderrPath);
  const stdoutChunks = [];

  const startMs = performance.now();

  const nodeExe = process.platform === "win32" ? `"${process.execPath}"` : process.execPath;

  const child = spawn(
    nodeExe,
    ["--expose-gc", "--import", tsxLoaderPath, traceComponentPath, componentToken],
    {
      cwd: repoRoot,
      shell: process.platform === "win32",
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
  const timer = setTimeout(() => {
    timedOut = true;
    killProcessTree(child.pid);
  }, config.timeoutMs);
  timer.unref();

  const { exitCode, signal } = await new Promise((resolvePromise) => {
    child.once("error", () => resolvePromise({ exitCode: 1, signal: null }));
    child.once("close", (code, sig) => resolvePromise({ exitCode: code, signal: sig }));
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

  const traceResolveMs = parseTraceResolveMs(tracePath);

  return {
    component: componentRelPath,
    status,
    wall_ms: Math.round(wallMs),
    query_ms_from_stdout: stdoutFields.queryMsFromStdout,
    trace_resolve_ms: traceResolveMs,
    exit_code: exitCode,
    signal,
    stdout_path: stdoutPath,
    stderr_path: stderrPath,
    trace_path: tracePath,
    saw_done_line: stdoutFields.sawDoneLine,
    saw_closed_line: stdoutFields.sawClosedLine,
    js_audit_path: null,
  };
}

// ---------------------------------------------------------------------------
// Main
// ---------------------------------------------------------------------------

async function main() {
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
  console.error(`Trace: ${config.traceEnabled ? "enabled" : "disabled"}`);

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
        `    ${result.status} (${result.wall_ms}ms wall, ${result.query_ms_from_stdout ?? "?"}ms query)`,
      );
    } else {
      failCount++;
      console.error(
        `    ${result.status} (${result.wall_ms}ms wall, exit=${result.exit_code}, signal=${result.signal})`,
      );
    }
  }

  // Write structured summary
  const summary = {
    generated_at: Date.now(),
    config: {
      ui_root: config.uiRoot,
      timeout_ms: config.timeoutMs,
      trace_enabled: config.traceEnabled,
    },
    totals: {
      discovered: vueFiles.length,
      ok: okCount,
      failed: failCount,
    },
    results,
  };

  const summaryPath = resolve(config.outputDir, "summary.json");
  writeFileSync(summaryPath, JSON.stringify(summary, null, 2));

  console.error(`\nDone: ${okCount}/${vueFiles.length} ok, ${failCount} failed`);
  console.error(`Summary: ${summaryPath}`);

  // Also write to stdout for piping
  console.log(JSON.stringify(summary, null, 2));

  process.exit(failCount > 0 ? 1 : 0);
}

main().catch((err) => {
  console.error("FATAL:", err);
  process.exit(2);
});
