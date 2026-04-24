/**
 * Parent-owned hard-timeout corpus runner.
 *
 * Runs each component in an isolated child process with a hard timeout
 * enforced by SIGKILL from the parent. The runner is consumed by the
 * audit-only corpus driver in `scripts/benchmark/trace-component-corpus.mjs`
 * (plan §3 Commit 10): each spawned worker emits BOTH the
 * `RustAuditRecord` JSON (`audit_path`) AND the
 * `ComponentMetaAnalysis` JSON (`analysis_path`) via the NAPI
 * `getComponentMetaWithAudit` binding. The runner exposes these paths
 * on the returned `CorpusTraceResult` so downstream consumers
 * (`audit-validator.ts`) can read both artifacts without re-running.
 */

import { createWriteStream, existsSync, mkdirSync } from "node:fs";
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
  /**
   * Path the worker MAY write the `RustAuditRecord` JSON to (env
   * `VERTER_COMPONENT_META_AUDIT_PATH`). The runner pre-allocates the
   * path; consumers should check `existsSync(audit_path)` before
   * reading. Populated when the worker emits the bundle through the
   * NAPI `getComponentMetaWithAudit` binding (plan §3 Commit 10).
   */
  audit_path: string;
  /**
   * Path the worker MAY write the `ComponentMetaAnalysis` JSON to
   * (env `VERTER_COMPONENT_META_ANALYSIS_PATH`). The runner
   * pre-allocates the path; consumers should check `existsSync` before
   * reading. Mirrors `audit_path` — both come from the same
   * `getComponentMetaWithAudit` call in the worker (plan §3 Commit 10).
   */
  analysis_path: string;
  /**
   * `true` when the worker actually wrote a file at `audit_path` —
   * lets the parent decide whether to invoke `audit-validator` on
   * this component. False on crash / timeout / legacy worker paths
   * that don't emit audit data.
   */
  audit_emitted: boolean;
  /**
   * Mirror of `audit_emitted` for the analysis side-car. Both are
   * required for `audit-validator` to run; `audit_emitted &&
   * analysis_emitted` is the precondition.
   */
  analysis_emitted: boolean;
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
  const auditPath = resolve(options.outputDir, `${sanitized}.audit.json`);
  const analysisPath = resolve(options.outputDir, `${sanitized}.analysis.json`);

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
      VERTER_COMPONENT_META_AUDIT_PATH: auditPath,
      VERTER_COMPONENT_META_ANALYSIS_PATH: analysisPath,
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
    audit_path: auditPath,
    analysis_path: analysisPath,
    audit_emitted: existsSync(auditPath),
    analysis_emitted: existsSync(analysisPath),
  };
}
