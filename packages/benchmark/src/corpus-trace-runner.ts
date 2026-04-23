/**
 * Parent-owned hard-timeout corpus trace runner.
 *
 * Runs each component in an isolated child process with a hard timeout
 * enforced by SIGKILL from the parent. This replaces the soft Promise.race
 * timeout model in _trace-component.ts for corpus sweeps.
 */

import { createWriteStream, mkdirSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { spawn } from "node:child_process";
import { performance } from "node:perf_hooks";

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

export type CorpusTraceStatus =
  | "ok"
  | "query_timeout"
  | "setup_timeout"
  | "close_timeout"
  | "crash"
  | "external_signal";

export interface CorpusTraceResult {
  component: string;
  status: CorpusTraceStatus;
  wall_ms: number;
  query_ms_from_stdout: number | null;
  trace_resolve_ms: number | null;
  trace_compute_ms: number | null;
  trace_materialize_ms: number | null;
  trace_query_ms: number | null;
  exit_code: number | null;
  signal: string | null;
  stdout_path: string;
  stderr_path: string;
  trace_path: string;
  result_path: string;
  saw_done_line: boolean;
  saw_closed_line: boolean;
  js_audit_path: string | null;
}

// ---------------------------------------------------------------------------
// classifyExitStatus — pure status classification
// ---------------------------------------------------------------------------

export interface ExitStatusInput {
  exitCode: number | null;
  signal: string | null;
  timedOut: boolean;
  sawDoneLine: boolean;
  sawClosedLine: boolean;
}

export function classifyExitStatus(input: ExitStatusInput): CorpusTraceStatus {
  if (input.timedOut) {
    // Parent timer fired and killed the child
    if (input.sawDoneLine) {
      // Query completed but process did not exit in time (close hang)
      return "close_timeout";
    }
    return "query_timeout";
  }

  if (input.signal) {
    // Killed by a signal but not by our parent timeout
    return "external_signal";
  }

  if (input.exitCode === 0 && input.sawDoneLine) {
    return "ok";
  }

  if (input.exitCode !== null && input.exitCode !== 0) {
    return "crash";
  }

  // Fallback: exited with 0 but no Done line — treat as crash
  return "crash";
}

// ---------------------------------------------------------------------------
// parseStdoutFields — extract structured fields from child stdout
// ---------------------------------------------------------------------------

export interface StdoutFields {
  queryMsFromStdout: number | null;
  sawDoneLine: boolean;
  sawClosedLine: boolean;
}

const DONE_LINE_RE = /^Done in (\d+)ms/m;
const CLOSED_LINE_RE = /^Closed\b/m;

export function parseStdoutFields(stdout: string): StdoutFields {
  const doneMatch = stdout.match(DONE_LINE_RE);
  const sawDoneLine = doneMatch !== null;
  const sawClosedLine = CLOSED_LINE_RE.test(stdout);

  return {
    queryMsFromStdout: doneMatch ? Number.parseInt(doneMatch[1], 10) : null,
    sawDoneLine,
    sawClosedLine,
  };
}

// ---------------------------------------------------------------------------
// killProcessTree — hard kill for child process group
// ---------------------------------------------------------------------------

function killWindowsProcessTree(pid: number | undefined): void {
  if (!pid) {
    return;
  }
  const killer = spawn("taskkill", ["/PID", String(pid), "/T", "/F"], {
    stdio: "ignore",
    windowsHide: true,
    detached: true,
  });
  killer.unref();
}

function killProcessTree(pid: number | undefined): void {
  if (!pid) {
    return;
  }
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
      // Process already gone
    }
  }
}

// ---------------------------------------------------------------------------
// sanitizePathComponent — make a file path safe for filesystem use
// ---------------------------------------------------------------------------

function sanitizePathComponent(component: string): string {
  return component.replace(/[/\\]/g, "__").replace(/\.vue$/, "__vue");
}

// ---------------------------------------------------------------------------
// runComponentInIsolation — run a single component in an isolated child
// ---------------------------------------------------------------------------

export interface RunComponentOptions {
  component: string;
  command: string;
  args: string[];
  timeoutMs: number;
  outputDir: string;
  env?: Record<string, string>;
}

export async function runComponentInIsolation(
  options: RunComponentOptions,
): Promise<CorpusTraceResult> {
  const sanitized = sanitizePathComponent(options.component);
  const stdoutPath = resolve(options.outputDir, `${sanitized}.stdout.txt`);
  const stderrPath = resolve(options.outputDir, `${sanitized}.stderr.txt`);
  const tracePath = resolve(options.outputDir, `${sanitized}.trace.log`);
  const resultPath = resolve(options.outputDir, `${sanitized}.result.json`);

  mkdirSync(dirname(stdoutPath), { recursive: true });

  const stdoutStream = createWriteStream(stdoutPath);
  const stderrStream = createWriteStream(stderrPath);
  const stdoutChunks: string[] = [];

  const startMs = performance.now();

  const child = spawn(options.command, options.args, {
    cwd: process.cwd(),
    shell: false,
    detached: process.platform !== "win32",
    windowsHide: true,
    stdio: ["ignore", "pipe", "pipe"],
    env: {
      ...process.env,
      FORCE_COLOR: "0",
      ...(options.env ?? {}),
      VERTER_COMPONENT_META_TRACE_PATH: tracePath,
      VERTER_COMPONENT_META_RESULT_PATH: resultPath,
    },
  });

  child.stdout?.setEncoding("utf8");
  child.stdout?.on("data", (chunk: string) => {
    stdoutChunks.push(chunk);
    stdoutStream.write(chunk);
  });

  child.stderr?.on("data", (chunk: Buffer) => {
    stderrStream.write(chunk);
  });

  let timedOut = false;
  let childClosed = false;
  let windowsTreeKillFallback: NodeJS.Timeout | null = null;
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
  }, options.timeoutMs);
  timer.unref();

  const { exitCode, signal } = await new Promise<{
    exitCode: number | null;
    signal: string | null;
  }>((resolveResult) => {
    child.once("error", () => resolveResult({ exitCode: 1, signal: null }));
    child.once("close", (code, sig) => {
      childClosed = true;
      if (windowsTreeKillFallback) {
        clearTimeout(windowsTreeKillFallback);
      }
      resolveResult({ exitCode: code, signal: sig });
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
  const normalizedExitCode = timedOut ? null : exitCode;

  return {
    component: options.component,
    status,
    wall_ms: Math.round(wallMs),
    query_ms_from_stdout: stdoutFields.queryMsFromStdout,
    trace_resolve_ms: null, // Populated by trace log parsing if needed
    trace_compute_ms: null, // Populated by trace log parsing if needed
    trace_materialize_ms: null, // Populated by trace log parsing if needed
    trace_query_ms: null, // Populated by trace log parsing if needed
    exit_code: normalizedExitCode,
    signal,
    stdout_path: stdoutPath,
    stderr_path: stderrPath,
    trace_path: tracePath,
    result_path: resultPath,
    saw_done_line: stdoutFields.sawDoneLine,
    saw_closed_line: stdoutFields.sawClosedLine,
    js_audit_path: null,
  };
}
