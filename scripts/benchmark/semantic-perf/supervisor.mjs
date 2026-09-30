// The process-supervision adapter: every benchmark invocation runs under
// `verter-supervise` (crates/verter_supervise), which establishes OS-backed
// process-tree memory containment and a deadline before the child runs,
// tears the tree down on every exit path and reports high-water telemetry.
//
// There is no fallback: when no supervisor binary is available the harness
// refuses to run (fail closed). A sampled backend (macOS) runs only with the
// caller's explicit consent (`--allow-sampled`), and every record says which
// containment it ran under.

import { spawn, spawnSync } from "node:child_process";
import { existsSync, readFileSync } from "node:fs";
import { join } from "node:path";

export const SUPERVISOR_SCHEMA = 1;
export const EXIT_TIMEOUT = 124;
export const EXIT_SUPERVISOR_ERROR = 125;
export const EXIT_CANCELLED = 130;
export const EXIT_MEMORY = 137;

const REQUIRED_FIELDS = [
  "schema",
  "launched",
  "wallMs",
  "exitCode",
  "signal",
  "killedBy",
  "peakBytes",
  "peakMetric",
  "containment",
  "backend",
  "stdoutPath",
  "stderrPath",
  "errors",
];

/**
 * Locate the supervisor: an explicit path, else the workspace's own
 * `verter_supervise` crate built in release. Throws when neither exists.
 */
export function resolveSupervisor(root, explicitPath) {
  if (explicitPath) {
    if (!existsSync(explicitPath)) throw new Error(`--supervisor ${explicitPath} does not exist`);
    return { path: explicitPath, origin: "explicit" };
  }
  if (!existsSync(join(root, "crates", "verter_supervise", "Cargo.toml"))) {
    throw new Error(
      "no process supervisor: this checkout has no crates/verter_supervise and no --supervisor <path> was given. " +
        "The benchmark never runs a probe without hard (or explicitly consented sampled) containment.",
    );
  }
  const env = { ...process.env };
  if (!env.CARGO_INCREMENTAL) env.CARGO_INCREMENTAL = "0";
  const r = spawnSync("cargo", ["build", "--release", "-p", "verter_supervise", "--bin", "verter-supervise"], {
    cwd: root,
    env,
    stdio: ["ignore", "inherit", "inherit"],
  });
  if (r.status !== 0) throw new Error(`building verter-supervise failed (exit ${r.status})`);
  const path = join(root, "target", "release", process.platform === "win32" ? "verter-supervise.exe" : "verter-supervise");
  if (!existsSync(path)) throw new Error(`verter-supervise was not built at ${path}`);
  return { path, origin: "workspace" };
}

/** Problems with a supervisor result document (empty when it is well formed). */
export function supervisorRecordProblems(record) {
  if (!record || typeof record !== "object") return ["no supervisor result document"];
  const problems = [];
  for (const field of REQUIRED_FIELDS) if (!(field in record)) problems.push(`supervisor result lacks ${field}`);
  if (record.schema !== SUPERVISOR_SCHEMA) problems.push(`supervisor result schema ${record.schema} is not ${SUPERVISOR_SCHEMA}`);
  return problems;
}

/**
 * Run `argv` under the supervisor; resolves with the supervisor's exit code
 * and its parsed result document (or the reason it could not be read).
 */
export function runSupervised(supervisor, { memMb, timeoutMs, out, cwd, env = {}, argv, allowSampled }) {
  const args = ["run", "--mem-mb", String(memMb), "--timeout-ms", String(timeoutMs), "--out", out];
  if (cwd) args.push("--cwd", cwd);
  for (const [key, value] of Object.entries(env)) args.push("--env", `${key}=${value}`);
  if (allowSampled) args.push("--allow-sampled");
  args.push("--", ...argv);
  return new Promise((resolve) => {
    const child = spawn(supervisor, args, { stdio: ["ignore", "ignore", "pipe"] });
    let stderr = "";
    child.stderr.on("data", (chunk) => (stderr += chunk));
    child.on("error", (err) => resolve({ supervisorExit: null, spawnError: String(err), record: null, stderr }));
    child.on("close", (code, signal) => {
      let record = null;
      let readError = null;
      try {
        record = JSON.parse(readFileSync(out, "utf8"));
      } catch (err) {
        readError = String(err);
      }
      resolve({ supervisorExit: code, supervisorSignal: signal, record, readError, stderr: stderr.slice(0, 4000) });
    });
  });
}
