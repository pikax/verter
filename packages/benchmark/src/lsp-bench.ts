/**
 * LSP benchmark runner for Verter vs Volar.
 *
 * Measures initialization, workspace scan, didOpen-to-hover latency, and warm
 * hover latency for both Verter LSP and Volar.
 */

import { spawn, type ChildProcess } from "node:child_process";
import { readFileSync, readdirSync } from "node:fs";
import { resolve, join, dirname } from "node:path";
import { pathToFileURL, fileURLToPath } from "node:url";

import { parseLspBenchConfig } from "./lsp-bench.config";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
// Repo root: packages/benchmark/src/ → ../../..
const REPO_ROOT = resolve(__dirname, "../../..");
const BENCH_CONFIG = parseLspBenchConfig({
  argv: process.argv,
  cwd: process.cwd(),
  env: process.env,
  platform: process.platform,
  repoRoot: REPO_ROOT,
});
const JSON_MODE = BENCH_CONFIG.jsonMode;
const SKIP_VOLAR = BENCH_CONFIG.skipVolar;

// In JSON mode, redirect console.log to stderr so only JSON goes to stdout
if (JSON_MODE) {
  console.log = (...args: unknown[]) => {
    process.stderr.write(args.join(" ") + "\n");
  };
}

// ─── Configuration ───────────────────────────────────────────────────

const WORKSPACE_ROOT = BENCH_CONFIG.workspaceRoot;
const VERTER_BIN = BENCH_CONFIG.verterBin;
const TEST_FILE_REL = BENCH_CONFIG.testFileRel;
const TEST_FILE = BENCH_CONFIG.testFile;
const HOVER_LINE = BENCH_CONFIG.hoverLine;
const HOVER_CHAR = BENCH_CONFIG.hoverChar;
const PROJECT_NAME = BENCH_CONFIG.projectName;

const PHASE_TIMEOUT = 120_000; // 120s for workspace scan
const SHORT_TIMEOUT = 30_000; // 30s for other phases
const WARM_HOVER_ITERATIONS = 5;

// ─── JSON-RPC over stdio ────────────────────────────────────────────

class LspClient {
  private process: ChildProcess;
  private buffer = "";
  private nextId = 1;
  private pendingRequests = new Map<
    number,
    {
      resolve: (v: any) => void;
      reject: (e: Error) => void;
      timer?: ReturnType<typeof setTimeout>;
    }
  >();
  private notificationHandlers = new Map<string, ((params: any) => void)[]>();
  private requestHandlers = new Map<string, (params: any) => unknown>();
  private name: string;

  constructor(name: string, command: string, args: string[], cwd?: string) {
    this.name = name;
    this.process = spawn(command, args, {
      stdio: ["pipe", "pipe", "pipe"],
      cwd,
      env: { ...process.env },
      detached: process.platform !== "win32",
    });

    this.process.stdout!.on("data", (chunk: Buffer) => {
      this.buffer += chunk.toString("utf-8");
      this.drainMessages();
    });

    this.process.stderr!.on("data", (chunk: Buffer) => {
      // Suppress stderr — some servers log here
    });

    this.process.on("error", (err) => {
      console.error(`[${this.name}] Process error:`, err.message);
    });

    this.process.on("exit", (code) => {
      // Clear timers and reject all pending requests
      for (const [, pending] of this.pendingRequests) {
        if (pending.timer) clearTimeout(pending.timer);
        pending.reject(new Error(`${this.name} process exited with code ${code}`));
      }
      this.pendingRequests.clear();
    });
  }

  private drainMessages() {
    while (true) {
      const headerEnd = this.buffer.indexOf("\r\n\r\n");
      if (headerEnd === -1) break;

      const header = this.buffer.slice(0, headerEnd);
      const match = header.match(/Content-Length:\s*(\d+)/i);
      if (!match) {
        // Skip malformed header
        this.buffer = this.buffer.slice(headerEnd + 4);
        continue;
      }

      const contentLength = parseInt(match[1], 10);
      const bodyStart = headerEnd + 4;
      const bodyEnd = bodyStart + contentLength;

      if (this.buffer.length < bodyEnd) break; // Incomplete body

      const body = this.buffer.slice(bodyStart, bodyEnd);
      this.buffer = this.buffer.slice(bodyEnd);

      try {
        const msg = JSON.parse(body);
        this.handleMessage(msg);
      } catch {
        // Skip malformed JSON
      }
    }
  }

  private handleMessage(msg: any) {
    if ("id" in msg && !("method" in msg)) {
      // Response
      const pending = this.pendingRequests.get(msg.id);
      if (pending) {
        this.pendingRequests.delete(msg.id);
        if (pending.timer) clearTimeout(pending.timer);
        if (msg.error) {
          pending.reject(new Error(`${this.name} LSP error: ${JSON.stringify(msg.error)}`));
        } else {
          pending.resolve(msg.result);
        }
      }
    } else if ("method" in msg && !("id" in msg)) {
      // Notification
      if (process.env.LSP_BENCH_DEBUG) {
        const summary =
          msg.method === "textDocument/publishDiagnostics"
            ? ` uri=${msg.params?.uri} diags=${msg.params?.diagnostics?.length ?? 0}`
            : "";
        console.log(`  [${this.name}] notification: ${msg.method}${summary}`);
      }
      const handlers = this.notificationHandlers.get(msg.method);
      if (handlers) {
        for (const handler of handlers) {
          handler(msg.params);
        }
      }
    } else if ("method" in msg && "id" in msg) {
      // Server-to-client request — check registered handlers first
      const handler = this.requestHandlers.get(msg.method);
      if (handler) {
        try {
          const result = handler(msg.params);
          const body = JSON.stringify({ jsonrpc: "2.0", id: msg.id, result });
          const header = `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n`;
          this.process.stdin!.write(header + body);
        } catch (err: any) {
          const body = JSON.stringify({
            jsonrpc: "2.0",
            id: msg.id,
            error: { code: -32603, message: err.message },
          });
          const header = `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n`;
          this.process.stdin!.write(header + body);
        }
      } else if (
        msg.method === "window/workDoneProgress/create" ||
        msg.method === "client/registerCapability"
      ) {
        const body = JSON.stringify({ jsonrpc: "2.0", id: msg.id, result: null });
        const header = `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n`;
        this.process.stdin!.write(header + body);
      }
    }
  }

  sendRequest<T = any>(method: string, params?: any, timeout = SHORT_TIMEOUT): Promise<T> {
    return new Promise<T>((resolve, reject) => {
      const id = this.nextId++;

      const timer = setTimeout(() => {
        if (this.pendingRequests.has(id)) {
          this.pendingRequests.delete(id);
          reject(new Error(`${this.name} request '${method}' timed out after ${timeout}ms`));
        }
      }, timeout);
      timer.unref();

      this.pendingRequests.set(id, { resolve, reject, timer });

      const body = JSON.stringify({ jsonrpc: "2.0", id, method, params });
      const header = `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n`;
      this.process.stdin!.write(header + body);
    });
  }

  sendNotification(method: string, params?: any) {
    const body = JSON.stringify({ jsonrpc: "2.0", method, params });
    const header = `Content-Length: ${Buffer.byteLength(body)}\r\n\r\n`;
    this.process.stdin!.write(header + body);
  }

  onNotification(method: string, handler: (params: any) => void) {
    const handlers = this.notificationHandlers.get(method) || [];
    handlers.push(handler);
    this.notificationHandlers.set(method, handlers);
  }

  /** Register a handler for server-to-client requests. */
  onRequest(method: string, handler: (params: any) => unknown) {
    this.requestHandlers.set(method, handler);
  }

  waitForNotification(
    method: string,
    timeout = SHORT_TIMEOUT,
    predicate?: (params: any) => boolean,
  ): Promise<any> {
    return new Promise((resolve, reject) => {
      const timer = setTimeout(() => {
        reject(new Error(`${this.name} notification '${method}' timed out after ${timeout}ms`));
      }, timeout);

      const handler = (params: any) => {
        if (predicate && !predicate(params)) return;
        clearTimeout(timer);
        // Remove this handler
        const handlers = this.notificationHandlers.get(method);
        if (handlers) {
          const idx = handlers.indexOf(handler);
          if (idx >= 0) handlers.splice(idx, 1);
        }
        resolve(params);
      };
      this.onNotification(method, handler);
    });
  }

  async kill() {
    return new Promise<void>((resolve) => {
      // Already exited
      if (this.process.exitCode !== null) {
        resolve();
        return;
      }

      const pid = this.process.pid;
      const forceTimer = setTimeout(() => {
        // Grace period expired — hard kill the process tree
        if (process.platform === "win32") {
          const killer = spawn("taskkill", ["/PID", String(pid), "/T", "/F"], {
            stdio: "ignore",
            windowsHide: true,
          });
          killer.once("close", () => {});
        } else {
          // Negative PID kills the process group (requires detached spawn)
          try {
            process.kill(-pid!, "SIGKILL");
          } catch {
            try {
              process.kill(pid!, "SIGKILL");
            } catch {}
          }
        }
      }, 2000);
      forceTimer.unref();

      this.process.once("exit", () => {
        clearTimeout(forceTimer);
        resolve();
      });

      try {
        this.process.kill("SIGTERM");
      } catch {}
    });
  }
}

// ─── Timing helpers ──────────────────────────────────────────────────

function hrMs(): number {
  return performance.now();
}

function median(values: number[]): number {
  const sorted = [...values].sort((a, b) => a - b);
  const mid = Math.floor(sorted.length / 2);
  return sorted.length % 2 !== 0 ? sorted[mid] : (sorted[mid - 1] + sorted[mid]) / 2;
}

function formatMs(ms: number): string {
  if (ms >= 1000) return `${(ms / 1000).toFixed(1)}s`;
  if (ms < 1) return `${ms.toFixed(2)}ms`;
  return `${Math.round(ms)}ms`;
}

// ─── Initialize params ───────────────────────────────────────────────

function makeInitializeParams(rootUri: string, workspaceName: string, initializationOptions?: any) {
  return {
    processId: process.pid,
    capabilities: {
      textDocument: {
        publishDiagnostics: {
          relatedInformation: true,
        },
        hover: {
          contentFormat: ["markdown", "plaintext"],
        },
        completion: {
          completionItem: {
            snippetSupport: false,
          },
        },
      },
      workspace: {
        workspaceFolders: true,
      },
    },
    rootUri,
    workspaceFolders: [
      {
        uri: rootUri,
        name: workspaceName,
      },
    ],
    ...(initializationOptions ? { initializationOptions } : {}),
  };
}

// ─── Benchmark runner ────────────────────────────────────────────────

interface BenchmarkResult {
  initialize: number;
  workspaceScan: number;
  didOpenToHover: number;
  hoverCold: number;
  hoverWarmMedian: number;
}

async function benchmarkVerter(label: string, typeProvider: string): Promise<BenchmarkResult> {
  const args = [WORKSPACE_ROOT, `--type-provider=${typeProvider}`];
  const client = new LspClient(label, VERTER_BIN, args);

  // For extension type provider, the benchmark runner acts as the extension host.
  // Register a $/verter/tsQuery handler backed by an in-process TS language service.
  if (typeProvider === "extension") {
    const { ExtensionTsService } = await import("../../vue-vscode/src/extensionTsService");
    const tsService = new ExtensionTsService(resolve(WORKSPACE_ROOT));
    client.onRequest(
      "$/verter/tsQuery",
      (params: { command: string; arguments: Record<string, unknown> }) => {
        return tsService.handleQuery(params.command, params.arguments);
      },
    );
  }

  const rootUri = pathToFileURL(resolve(WORKSPACE_ROOT)).toString();
  const fileUri = pathToFileURL(resolve(TEST_FILE)).toString();
  const fileContent = readFileSync(TEST_FILE, "utf-8");

  try {
    // Phase 1: Initialize
    const t0 = hrMs();
    await client.sendRequest(
      "initialize",
      makeInitializeParams(rootUri, PROJECT_NAME),
      SHORT_TIMEOUT,
    );
    client.sendNotification("initialized", {});
    const initTime = hrMs() - t0;

    // Phase 2: Workspace Scan — wait for $/verter/ready which fires after
    // project registry, workspace scanner, and type provider are all ready.
    const t1 = hrMs();
    await client.waitForNotification("$/verter/ready", PHASE_TIMEOUT);
    const scanTime = hrMs() - t1;

    // Phase 3: didOpen → first hover (measures parse + compile + sync time)
    const t2 = hrMs();
    client.sendNotification("textDocument/didOpen", {
      textDocument: {
        uri: fileUri,
        languageId: "vue",
        version: 1,
        text: fileContent,
      },
    });
    await client.sendRequest(
      "textDocument/hover",
      {
        textDocument: { uri: fileUri },
        position: { line: HOVER_LINE, character: HOVER_CHAR },
      },
      SHORT_TIMEOUT,
    );
    const didOpenToHoverTime = hrMs() - t2;

    // Phase 4: Hover (cold — file already open, but fresh request)
    const t3 = hrMs();
    await client.sendRequest(
      "textDocument/hover",
      {
        textDocument: { uri: fileUri },
        position: { line: HOVER_LINE, character: HOVER_CHAR },
      },
      SHORT_TIMEOUT,
    );
    const hoverColdTime = hrMs() - t3;

    // Phase 5: Hover (warm) — repeat N times, take median
    const hoverTimes: number[] = [];
    for (let i = 0; i < WARM_HOVER_ITERATIONS; i++) {
      const t = hrMs();
      await client.sendRequest(
        "textDocument/hover",
        {
          textDocument: { uri: fileUri },
          position: { line: HOVER_LINE, character: HOVER_CHAR },
        },
        SHORT_TIMEOUT,
      );
      hoverTimes.push(hrMs() - t);
    }

    return {
      initialize: initTime,
      workspaceScan: scanTime,
      didOpenToHover: didOpenToHoverTime,
      hoverCold: hoverColdTime,
      hoverWarmMedian: median(hoverTimes),
    };
  } finally {
    // Send shutdown/exit before killing
    try {
      await client.sendRequest("shutdown", null, 5000);
      client.sendNotification("exit");
    } catch {}
    await client.kill();
  }
}

async function benchmarkVolar(): Promise<BenchmarkResult> {
  const volarScript = BENCH_CONFIG.volarScript;
  const tsdkPath = BENCH_CONFIG.tsdkPath;
  if (!volarScript || !tsdkPath) {
    throw new Error("Volar benchmark requires a resolved Volar script and TypeScript SDK.");
  }
  const client = new LspClient("Volar", process.execPath, [volarScript, "--stdio"]);

  const rootUri = pathToFileURL(resolve(WORKSPACE_ROOT)).toString();
  const fileUri = pathToFileURL(resolve(TEST_FILE)).toString();
  const fileContent = readFileSync(TEST_FILE, "utf-8");

  try {
    const volarInitOptions = {
      typescript: {
        tsdk: tsdkPath,
      },
    };

    // Phase 1: Initialize
    const t0 = hrMs();
    await client.sendRequest(
      "initialize",
      makeInitializeParams(rootUri, PROJECT_NAME, volarInitOptions),
      SHORT_TIMEOUT,
    );
    client.sendNotification("initialized", {});
    const initTime = hrMs() - t0;

    // Phase 2: Workspace Scan — Volar doesn't have a scan-complete signal.
    // We skip this phase for Volar (set to 0) since it's bundled into init.
    const scanTime = 0;

    // Phase 3: didOpen → first hover
    const t2 = hrMs();
    client.sendNotification("textDocument/didOpen", {
      textDocument: {
        uri: fileUri,
        languageId: "vue",
        version: 1,
        text: fileContent,
      },
    });
    await client.sendRequest(
      "textDocument/hover",
      {
        textDocument: { uri: fileUri },
        position: { line: HOVER_LINE, character: HOVER_CHAR },
      },
      SHORT_TIMEOUT,
    );
    const didOpenToHoverTime = hrMs() - t2;

    // Phase 4: Hover (cold — file already open)
    const t3 = hrMs();
    await client.sendRequest(
      "textDocument/hover",
      {
        textDocument: { uri: fileUri },
        position: { line: HOVER_LINE, character: HOVER_CHAR },
      },
      SHORT_TIMEOUT,
    );
    const hoverColdTime = hrMs() - t3;

    // Phase 5: Hover (warm)
    const hoverTimes: number[] = [];
    for (let i = 0; i < WARM_HOVER_ITERATIONS; i++) {
      const t = hrMs();
      await client.sendRequest(
        "textDocument/hover",
        {
          textDocument: { uri: fileUri },
          position: { line: HOVER_LINE, character: HOVER_CHAR },
        },
        SHORT_TIMEOUT,
      );
      hoverTimes.push(hrMs() - t);
    }

    return {
      initialize: initTime,
      workspaceScan: scanTime,
      didOpenToHover: didOpenToHoverTime,
      hoverCold: hoverColdTime,
      hoverWarmMedian: median(hoverTimes),
    };
  } finally {
    try {
      await client.sendRequest("shutdown", null, 5000);
      client.sendNotification("exit");
    } catch {}
    await client.kill();
  }
}

// ─── Helpers ─────────────────────────────────────────────────────────

function countVueFiles(dir: string): number {
  let count = 0;
  try {
    for (const entry of readdirSync(dir, { withFileTypes: true })) {
      const full = join(dir, entry.name);
      if (entry.isDirectory() && entry.name !== "node_modules") {
        count += countVueFiles(full);
      } else if (entry.isFile() && entry.name.endsWith(".vue")) {
        count++;
      }
    }
  } catch {
    // Permission errors, etc.
  }
  return count;
}

// ─── Output formatting ───────────────────────────────────────────────

function printResults(
  results: Record<string, BenchmarkResult>,
  projectName: string,
  vueFileCount: number,
) {
  console.log();
  console.log(`LSP Benchmark: ${projectName} (${vueFileCount} .vue files)`);
  console.log("═".repeat(70));
  console.log();

  const configs = Object.keys(results);
  const colWidth = 20;

  // Header
  const headerLabel = "Phase".padEnd(25);
  const headerCols = configs.map((c) => c.padStart(colWidth)).join("");
  console.log(`  ${headerLabel}${headerCols}`);
  console.log(`  ${"─".repeat(25)}${configs.map(() => "─".repeat(colWidth)).join("")}`);

  // Rows
  const phases: { key: keyof BenchmarkResult; label: string }[] = [
    { key: "initialize", label: "Initialize" },
    { key: "workspaceScan", label: "Workspace Scan" },
    { key: "didOpenToHover", label: "didOpen → Hover" },
    { key: "hoverCold", label: "Hover (cold)" },
    { key: "hoverWarmMedian", label: "Hover (median of 5)" },
  ];

  for (const phase of phases) {
    const label = phase.label.padEnd(25);
    const cols = configs
      .map((c) => {
        const val = results[c][phase.key];
        const text = phase.key === "workspaceScan" && val === 0 ? "N/A" : formatMs(val);
        return text.padStart(colWidth);
      })
      .join("");
    console.log(`  ${label}${cols}`);
  }

  console.log();
}

function outputJson(
  results: Record<string, BenchmarkResult>,
  projectName: string,
  vueFileCount: number,
  testFileLines: number,
) {
  const configs: Record<string, any> = {};
  for (const [name, r] of Object.entries(results)) {
    configs[name] = {
      initialize: roundTo2(r.initialize),
      workspaceScan: roundTo2(r.workspaceScan),
      didOpenToHover: roundTo2(r.didOpenToHover),
      hoverCold: roundTo2(r.hoverCold),
      hoverWarmMedian: roundTo2(r.hoverWarmMedian),
    };
  }

  const json = {
    project: projectName,
    vueFileCount,
    testFile: TEST_FILE_REL,
    testFileLines,
    platform: process.platform,
    arch: process.arch,
    nodeVersion: process.version,
    configs,
    timestamp: new Date().toISOString(),
  };

  // Write to stdout (console.log is redirected to stderr in JSON mode)
  process.stdout.write(JSON.stringify(json, null, 2) + "\n");
}

function roundTo2(n: number): number {
  return Math.round(n * 100) / 100;
}

// ─── Main ────────────────────────────────────────────────────────────

async function main() {
  const fileContent = readFileSync(TEST_FILE, "utf-8");
  const testFileLines = fileContent.split("\n").length;
  const vueFileCount = countVueFiles(WORKSPACE_ROOT);
  const totalSteps = SKIP_VOLAR ? 4 : 5;

  console.log(`LSP Benchmark: Verter vs Volar on ${PROJECT_NAME}`);
  console.log("Workspace:", WORKSPACE_ROOT, `(${vueFileCount} .vue files)`);
  console.log("Verter binary:", VERTER_BIN);
  console.log("Test file:", TEST_FILE_REL, `(${testFileLines} lines)`);
  console.log("Hover target:", `line ${HOVER_LINE + 1}, char ${HOVER_CHAR + 1}`);
  if (SKIP_VOLAR) console.log("Skipping Volar benchmark.");
  console.log();

  const results: Record<string, BenchmarkResult> = {};

  // Config 1: Verter without type provider
  console.log(`[1/${totalSteps}] Benchmarking Verter (type-provider=off)...`);
  try {
    results["Verter (no TP)"] = await benchmarkVerter("Verter (no TP)", "off");
    console.log("  Done.");
  } catch (err: any) {
    console.error("  FAILED:", err.message);
  }

  // Config 2: Verter with type provider
  console.log(`[2/${totalSteps}] Benchmarking Verter (type-provider=auto)...`);
  try {
    results["Verter (auto)"] = await benchmarkVerter("Verter (auto)", "auto");
    console.log("  Done.");
  } catch (err: any) {
    console.error("  FAILED:", err.message);
  }

  // Config 3: Verter with extension type provider (Experiment E)
  console.log(`[3/${totalSteps}] Benchmarking Verter (type-provider=extension)...`);
  try {
    results["Verter (extension)"] = await benchmarkVerter("Verter (extension)", "extension");
    console.log("  Done.");
  } catch (err: any) {
    console.error("  FAILED:", err.message);
  }

  // Config 4: Verter with TSGO
  console.log(`[4/${totalSteps}] Benchmarking Verter (type-provider=tsgo)...`);
  try {
    results["Verter (tsgo)"] = await benchmarkVerter("Verter (tsgo)", "tsgo");
    console.log("  Done.");
  } catch (err: any) {
    console.error("  FAILED:", err.message);
  }

  // Config 5: Volar (optional)
  if (!SKIP_VOLAR) {
    console.log(`[${totalSteps}/${totalSteps}] Benchmarking Volar...`);
    try {
      results["Volar"] = await benchmarkVolar();
      console.log("  Done.");
    } catch (err: any) {
      console.error("  FAILED:", err.message);
    }
  }

  if (Object.keys(results).length === 0) {
    console.error("All benchmarks failed.");
    process.exit(1);
  }

  if (JSON_MODE) {
    outputJson(results, PROJECT_NAME, vueFileCount, testFileLines);
  } else {
    printResults(results, PROJECT_NAME, vueFileCount);
  }
}

main().catch((err) => {
  console.error("Fatal:", err);
  process.exit(1);
});
