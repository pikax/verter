/**
 * The §2.7 perf workloads, factored as reusable sample producers.
 *
 * Two axes:
 *  - AXIS A (compiler): Verter's OWN native compiler (verter_compiler via NAPI).
 *    Each side runs in a CHILD process loading THAT side's `@verter/native`
 *    build (two native builds cannot coexist in one process), so the
 *    self-referential comparison is genuine. Gated on SFC→carrier codegen
 *    throughput (a SERIAL per-file compile, not a batch call), the
 *    codegen(+source-map) emit-time split, output/source-map bytes, and a
 *    carrier+source-map content hash; the non-checker aggregate + per-PID peak
 *    RSS are recorded-or-null and DEFERRED from the gated set.
 *  - AXIS B (typecheck): Verter+tsgo carrier typecheck/LSP. The cold + the
 *    incremental re-typecheck run through the `verter-tsc` subprocess (asserting
 *    exit-code + diagnostic-SET equality — the warm edit is TYPE-CHANGING and must
 *    transition the dependent diagnostics); the genuinely-warm + the interactive
 *    workloads drive the persistent `verter-lsp` binary, collecting per-operation
 *    latency DISTRIBUTIONS + IDE-query hover/completion content equality. `rssBytes`
 *    is recorded raw (wrapper-PID) but DEFERRED from the gated set — verter-tsc
 *    spawns tsgo as a SEPARATE child, so the wrapper-PID RSS misses the engine.
 *
 * Each workload exposes a `runOnce()` returning one sample. The gate (`gate.ts`)
 * interleaves a candidate build against a pinned baseline build over these
 * producers and evaluates real ratios + correctness + behavioral invariants.
 */
import { spawn, spawnSync, execFileSync, type SpawnSyncReturns } from "node:child_process";
import {
  existsSync,
  statSync,
  readdirSync,
  readFileSync,
  writeFileSync,
  rmSync,
  mkdirSync,
  cpSync,
} from "node:fs";
import { join, resolve, relative, dirname, basename } from "node:path";
import { fileURLToPath } from "node:url";
import type { EnsuredCorpus } from "./corpus.js";
import { type OverheadAttribution } from "./audit-attribution.js";
import { nativeEntry, type AxisAChildSample } from "./axis-a-child.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const VERTER_ROOT = resolve(__dirname, "..", "..", "..", "..");
const IS_WIN = process.platform === "win32";
const EXE = IS_WIN ? ".exe" : "";
const AXIS_A_CHILD = join(__dirname, "axis-a-child.ts");

export interface WorkloadCorrectness {
  readonly exitCode: number;
  /** Normalized, sorted diagnostic SET (corpus-relative). */
  readonly diagnostics: string[];
}

export interface WorkloadBehavioral {
  /**
   * Distinct document URIs that re-PUBLISHED diagnostics after a single-file edit
   * (max observed) — a diagnostic-publication-locality proxy, NOT a count of
   * truly invalidated/rechecked URIs (that needs server instrumentation).
   */
  readonly affectedUris: number;
  /** Project size (the locality-fraction denominator). */
  readonly totalUris: number;
}

/** A single workload sample. */
export interface WorkloadSample {
  /** Total wall-clock for this run (ms). */
  readonly totalMs: number;
  /** Non-checker (codegen-side) attribution, when an audited path produced it. */
  readonly attribution: OverheadAttribution | null;
  /**
   * Measured TARGET-process RSS (bytes) — the child compiler / typechecker
   * process, NOT this Node harness. `0` ⇒ sampled but unavailable on this
   * platform/path; `null` ⇒ NOT sampled for this workload (e.g. the LSP
   * workloads drive an in-process client and do not sample the server's RSS, so
   * they report `null` rather than a misleading `0`).
   */
  readonly rssBytes: number | null;
  /** Workload-specific scalar metrics (carrier count, diagnostics, etc.). */
  readonly metrics: Readonly<Record<string, number>>;
  /**
   * Expected per-operation count for THIS sample's distributions — the number of
   * operations the run requested (`ops`). The gate fails any per-operation
   * distribution whose length ≠ `expectedOps` (a sample that returned 49 latencies
   * for 50 requested ops is partial, not pooled). Set by the interactive workloads;
   * absent for non-distribution samples.
   */
  readonly expectedOps?: number;
  /** Per-operation latency distributions (interactive / warm workloads). */
  readonly distributions?: Readonly<Record<string, number[]>>;
  /** Exit code + diagnostic SET for correctness-gated workloads. */
  readonly correctness?: WorkloadCorrectness;
  /** Single-file-edit locality for behavioral-gated workloads. */
  readonly behavioral?: WorkloadBehavioral;
  /**
   * Observable-output content SETs compared candidate-vs-baseline for equality
   * (the gate's `contentEqualityGated` rail) — NOT counts. Keyed by a stable label:
   * the IDE-query workload publishes `hoverContents` (normalized hover text per
   * probed position) + `completionLabels` (normalized completion label set), and
   * axis-A publishes `carrierContent` (a content hash over each compiled carrier +
   * its source-map). A content divergence at a probed position / per-compile output
   * is a correctness regression even when the COUNT is unchanged.
   */
  readonly contentSets?: Readonly<Record<string, string[]>>;
}

export interface WorkloadContext {
  readonly corpus: EnsuredCorpus;
  /** A built-binary directory override (the gate points this at each build). */
  readonly binDir?: string;
  /** The side's `@verter/native` package root (axis A loads THAT side's build). */
  readonly nativeRoot?: string;
  /** Thread count to pin (recorded; axis-A's in-process host honors it). */
  readonly threads: number;
  /** Operations per interactive/warm run (the per-run distribution size). */
  readonly ops?: number;
  readonly quiet?: boolean;
  /**
   * Per-side isolated working directory. When set, the on-disk-cache workloads
   * (verter-tsc cold + incremental, and the LSP workloads) operate on a private
   * COPY of the corpus tree here — their OWN build/output/cache state — so a
   * candidate run cannot warm or perturb the baseline run, and vice-versa. When
   * unset (single-side runs, smoke, unit tests) they use the shared corpus dir.
   * The copy is content-identical; only the materialization location differs.
   */
  readonly workDir?: string;
  /**
   * Injectable child-spawn seam for the `verter-tsc` subprocess workloads
   * (defaults to the real `spawn`). Tests inject a scripted child to exercise
   * the measured exit/RSS/settle handling without a real binary.
   */
  readonly spawnChild?: SpawnChild;
  /**
   * Injectable axis-A child runner (the per-side native spawn seam; defaults to
   * spawning the real per-side `axis-a-child` process). Tests inject a fake to
   * assert each side runs with its OWN `--native` root and that a synthetic
   * axis-A regression flows through `runOnce` → the gate.
   */
  readonly axisAChildRunner?: AxisAChildRunner;
}

export interface Workload {
  readonly id: string;
  readonly axis: "A" | "B";
  readonly title: string;
  readonly interactive: boolean;
  readonly available: (ctx: WorkloadContext) => { ok: boolean; reason?: string };
  readonly runOnce: (ctx: WorkloadContext) => Promise<WorkloadSample>;
}

const DEFAULT_OPS = 50;
const opsOf = (ctx: WorkloadContext): number => ctx.ops ?? DEFAULT_OPS;

// ── Binary + native discovery ────────────────────────────────────────────────
function findBinary(name: string, binDir?: string): string | null {
  const candidates = binDir
    ? [join(binDir, `${name}${EXE}`)]
    : [
        join(VERTER_ROOT, "target", "release", `${name}${EXE}`),
        join(VERTER_ROOT, "target", "debug", `${name}${EXE}`),
      ];
  let best: { path: string; mtime: number } | null = null;
  for (const c of candidates) {
    if (!existsSync(c)) continue;
    const m = statSync(c).mtimeMs;
    if (best === null || m > best.mtime) best = { path: c, mtime: m };
  }
  return best?.path ?? null;
}

function nativeRootOf(ctx: WorkloadContext): string {
  return ctx.nativeRoot ?? join(VERTER_ROOT, "packages", "native");
}

// ── Child-process RSS sampling (the canonical runner is Linux) ───────────────
/**
 * Sample a process's RSS (peak where the OS exposes it), in bytes, or `null` when
 * it cannot be read (no pid, process gone, unsupported platform). Unavailable is
 * `null` — NEVER `0` — so a missing reading is counted as missing by the gate's
 * presence rail rather than coerced to a "present" zero that averages into a
 * nonzero vector and slips through.
 */
function sampleProcessRss(pid: number | undefined): number | null {
  if (!pid) return null;
  if (process.platform === "linux") {
    try {
      const status = readFileSync(`/proc/${pid}/status`, "utf-8");
      const hwm = status.match(/VmHWM:\s+(\d+)\s+kB/); // peak resident set
      if (hwm) return Number(hwm[1]) * 1024;
      const rss = status.match(/VmRSS:\s+(\d+)\s+kB/);
      if (rss) return Number(rss[1]) * 1024;
    } catch {
      /* process gone */
    }
    return null;
  }
  if (process.platform === "darwin") {
    try {
      const out = execFileSync("ps", ["-o", "rss=", "-p", String(pid)], { encoding: "utf-8" });
      const kb = Number(out.trim());
      return Number.isFinite(kb) && kb > 0 ? kb * 1024 : null;
    } catch {
      return null;
    }
  }
  // Windows: best-effort unavailable (a full run on a non-Linux box treats the
  // RSS metric as missing instrumentation; the canonical runner is Linux).
  return null;
}

export interface SampledRun {
  /** The child's exit code, or null when it died via signal / failed to spawn. */
  readonly status: number | null;
  /** The terminating signal, if any (SIGKILL on a harness timeout). */
  readonly signal: NodeJS.Signals | null;
  /** A spawn failure (e.g. ENOENT), if the child never started. */
  readonly spawnError: Error | null;
  /** Whether the harness timeout fired and force-killed the child. */
  readonly timedOut: boolean;
  readonly stdout: string;
  readonly stderr: string;
  /** Peak child RSS (bytes), or `null` when RSS could not be sampled (never `0`). */
  readonly peakRssBytes: number | null;
}

/**
 * The minimal spawned-child surface `spawnWithRssSampling` drives. Node's
 * `ChildProcess` satisfies it structurally; a test injects a scripted fake to
 * exercise the settle/exit/error handling without a real subprocess.
 */
export interface SampledChild {
  readonly pid?: number;
  readonly stdout: NodeJS.ReadableStream | null;
  readonly stderr: NodeJS.ReadableStream | null;
  on(event: "exit", listener: (code: number | null, signal: NodeJS.Signals | null) => void): void;
  // `close` fires AFTER the process ends AND all stdio streams are flushed/closed,
  // so the captured stdout/stderr is complete — the settle trigger (not `exit`).
  on(event: "close", listener: (code: number | null, signal: NodeJS.Signals | null) => void): void;
  on(event: "error", listener: (err: Error) => void): void;
  kill(signal?: NodeJS.Signals | number): void;
}

/** The child-spawn seam (defaults to the real `spawn`). */
export type SpawnChild = (
  bin: string,
  args: readonly string[],
  opts: { cwd: string; windowsHide: boolean },
) => SampledChild;

const defaultSpawnChild: SpawnChild = (bin, args, opts) => spawn(bin, [...args], opts);

/**
 * How long to wait for a `close` after a spawn `error` before settling via a
 * fallback. ENOENT and most spawn failures emit `close` shortly after `error` (so
 * the fallback is cleared instantly); this only bounds the rare error-without-close
 * case so a failed run never hangs.
 */
const ERROR_SETTLE_FALLBACK_MS = 50;

/**
 * Spawn a child and sample its peak RSS while it runs.
 *
 * Settles EXACTLY once, on `close` — which fires AFTER the process ends AND all
 * stdio streams are flushed/closed, so the captured stdout/stderr is COMPLETE (an
 * `exit`- or `error`-triggered IMMEDIATE settle could truncate a final diagnostic
 * chunk that flushes after exit). A spawn `error` is AUTHORITATIVE: it records
 * `spawnError` (forcing `status: null`) but DEFERS the settle to `close` so the
 * failure artifact's stdout/stderr is captured complete; if no `close` arrives
 * (some spawn failures emit only `error`), a short fallback timer settles. The
 * recorded `spawnError` can never be masked by a stray `close`, regardless of
 * arrival order — so two equally-failing sides can never collapse to a matching
 * "exit code" and pass. A `close` settles on a microtask so an `error` emitted in
 * the same tick records `spawnError` before the settle reads it.
 */
export function spawnWithRssSampling(
  bin: string,
  args: readonly string[],
  cwd: string,
  timeoutMs: number,
  spawnChild: SpawnChild = defaultSpawnChild,
): Promise<SampledRun> {
  return new Promise((resolveRun) => {
    let spawnError: Error | null = null;
    let timedOut = false;
    let settled = false;
    let errorSettleFallback: ReturnType<typeof setTimeout> | null = null;
    const child = spawnChild(bin, args, { cwd, windowsHide: true });
    let stdout = "";
    let stderr = "";
    // `null` until a real reading lands — an unsampleable child stays `null`
    // (unavailable), never the number `0`.
    let peak: number | null = null;
    child.stdout?.on("data", (d: Buffer) => (stdout += d.toString("utf-8")));
    child.stderr?.on("data", (d: Buffer) => (stderr += d.toString("utf-8")));
    const sample = (): void => {
      const r = sampleProcessRss(child.pid);
      if (r != null && (peak == null || r > peak)) peak = r;
    };
    const timer = setInterval(sample, 50);
    const killer = setTimeout(() => {
      timedOut = true;
      child.kill("SIGKILL");
    }, timeoutMs);
    const settle = (status: number | null, signal: NodeJS.Signals | null): void => {
      if (settled) return;
      settled = true;
      clearInterval(timer);
      clearTimeout(killer);
      if (errorSettleFallback) clearTimeout(errorSettleFallback);
      sample();
      // A captured spawn error is authoritative: never report a success status
      // for a child that failed to spawn.
      resolveRun({
        status: spawnError ? null : status,
        signal,
        spawnError,
        timedOut,
        stdout,
        stderr,
        peakRssBytes: peak,
      });
    };
    child.on("error", (err) => {
      // Record the spawn error AUTHORITATIVELY, but defer the settle to `close` so a
      // stdout/stderr chunk flushed around a late close is captured complete (not
      // truncated by an immediate settle). If no `close` arrives, the fallback timer
      // settles. `status` stays null whenever spawnError is set, so the error is
      // never masked regardless of arrival order.
      spawnError = err;
      errorSettleFallback = setTimeout(() => settle(null, null), ERROR_SETTLE_FALLBACK_MS);
      errorSettleFallback.unref();
    });
    // Settle on `close` (after all stdio is flushed), NOT `exit` — so a final stdout
    // chunk that flushes after the process exits is captured, not truncated.
    child.on("close", (code, signal) => queueMicrotask(() => settle(code, signal)));
  });
}

/**
 * Reduce a sampled run to a real exit code — or THROW a hard workload failure on
 * a crash / timeout / spawn-error / abnormal termination. A timeout, signal, or
 * spawn-error is NEVER an exit code: two equally-crashing sides must not collapse
 * to a "matching" status and pass. A clean nonzero exit (a tsc diagnostic exit)
 * IS a real code and is returned as-is (correctness-gated downstream).
 */
export function exitCodeOrThrow(run: SampledRun, label: string): number {
  if (run.spawnError) {
    const code = (run.spawnError as NodeJS.ErrnoException).code;
    throw new Error(
      `${label}: failed to spawn${code ? ` (${code})` : ""} — ${run.spawnError.message}`,
    );
  }
  if (run.timedOut) throw new Error(`${label}: timed out (hard failure, not an exit code)`);
  if (run.signal) {
    throw new Error(`${label}: killed by signal ${run.signal} (hard failure, not an exit code)`);
  }
  if (run.status === null) {
    throw new Error(`${label}: terminated abnormally with no exit code (hard failure)`);
  }
  return run.status;
}

/**
 * Reduce a typecheck-workload run (verter-tsc + tsgo) to a real exit code — or THROW
 * a hard workload failure. Beyond {@link exitCodeOrThrow}'s spawn-error / timeout /
 * signal / abnormal-exit throws, a clean NONZERO exit is a valid DIAGNOSTIC exit
 * ONLY when the output carries at least one parsed TypeScript diagnostic (`error
 * TS####:`, which includes tsc config/usage errors like TS5023 / TS18003). A nonzero
 * exit with ZERO parsed TypeScript diagnostics is a STARTUP/CRASH failure (the
 * engine failed before type-checking, e.g. exits 1/101 with a panic and no `error
 * TS` lines), NOT a diagnostic exit — a both-sides-identical crash must never compare
 * as a "matching" empty diagnostic set and pass the correctness gate. A clean ZERO
 * exit (no errors) stays a valid pass.
 */
export function typecheckExitCodeOrThrow(run: SampledRun, out: string, label: string): number {
  const code = exitCodeOrThrow(run, label);
  if (code !== 0 && countTsErrors(out) === 0) {
    throw new Error(
      `${label}: nonzero exit ${code} with no parsed TypeScript diagnostics ` +
        "(no `error TS####:` lines) and no recognizable tsc config/usage error — " +
        "a startup/crash exit, not a diagnostic exit",
    );
  }
  return code;
}

/** Throw a hard workload failure if a `spawnSync` crashed / timed out / errored. */
function assertSpawnSyncHealthy(r: SpawnSyncReturns<string>, label: string): void {
  if (r.error) {
    const code = (r.error as NodeJS.ErrnoException).code;
    throw new Error(`${label}: subprocess error${code ? ` (${code})` : ""} — ${r.error.message}`);
  }
  if (r.signal) throw new Error(`${label}: killed by signal ${r.signal} (hard failure)`);
}

// ── Diagnostic-set + build-state helpers ─────────────────────────────────────
function countTsErrors(out: string): number {
  return (out.match(/error TS\d+:/g) ?? []).length;
}

/**
 * Normalize a diagnostic file path to a run-STABLE form: collapse the per-run
 * `.tmp<random>/` carrier-materialization dir to a stable `.tmp/` token, and
 * relativize an absolute corpus path. Without this, two identical runs of the
 * same binary produce disjoint diagnostic sets (the random temp prefix differs),
 * which would make the correctness gate a perpetual false-fail.
 */
export function normalizeDiagPath(raw: string, rootDir: string): string {
  let p = raw.trim().replace(/\\/g, "/");
  p = p.replace(/(^|\/)\.tmp[A-Za-z0-9_]+\//g, "$1.tmp/");
  p = collapseCarrierHash(p);
  if (/^([a-zA-Z]:)?\//.test(p)) {
    const rel = relative(rootDir, p).split(/[\\/]/).join("/");
    if (!rel.startsWith("..")) p = rel;
  }
  return p;
}

/**
 * Collapse the content-addressed hash in a carrier filename (`<base>_<hash>.vue.ts`
 * → `<base>.vue.ts`). The carrier hash is path-dependent — it differs between the
 * per-side isolated working trees — but the diagnostic SET correctness compares
 * LOGICAL diagnostics (SFC + line + col + code), not the physical carrier hash,
 * so a side-independent logical name is the right key (the same spirit as the
 * per-run `.tmp<random>` collapse above).
 */
function collapseCarrierHash(p: string): string {
  return p.replace(/_[0-9a-fA-F]{8,}(\.vue\.[A-Za-z]+)/g, "$1");
}

/** Normalize a no-file diagnostic message to a run-stable form (collapse the
 * per-run temp dir + relativize any embedded absolute corpus path). */
export function normalizeDiagMessage(msg: string, rootDir: string): string {
  let m = msg.trim().replace(/\\/g, "/");
  m = m.replace(/(^|[\s'"(])\.tmp[A-Za-z0-9_]+\//g, "$1.tmp/");
  m = collapseCarrierHash(m);
  const root = rootDir.replace(/\\/g, "/").replace(/\/$/, "");
  if (root) m = m.split(`${root}/`).join("");
  return m;
}

/**
 * Parse verter-tsc/tsgo output into a normalized, sorted, run-stable diagnostic
 * SET. Captures BOTH file diagnostics (`path(line,col): error TSxxxx`) AND
 * no-file (global / compiler-options / config) diagnostics (`error TSxxxx:`) in
 * ONE pass — a dropped global diagnostic must not hide just because a file
 * diagnostic is also present.
 */
export function parseDiagnosticSet(out: string, rootDir: string): string[] {
  const set = new Set<string>();
  // The file-diagnostic key carries the SEVERITY and the normalized MESSAGE in
  // addition to path/range/code: two file diagnostics that agree on
  // path/range/code but differ in the diagnostic TEXT (or its severity) are
  // DISTINCT — a candidate that changes the actual error reported at the same
  // location+code must NOT pass diagnostic-SET equality. The message is
  // normalized run-stable (per-side carrier hashes + per-run temp dirs collapse)
  // so the equality stays logical, not byte-physical. Both `error` and `warning`
  // severities are captured (so a severity flip at the same site also registers).
  const fileRe = /^\s*(.+?)\((\d+),(\d+)\):\s*(error|warning) (TS\d+):\s*(.*)$/;
  const globalRe = /(?:^|\s)(error|warning) (TS\d+):\s*(.*)$/;
  for (const line of out.split(/\r?\n/)) {
    const f = fileRe.exec(line);
    if (f) {
      const [, path, ln, col, severity, code, message] = f;
      set.add(
        `${normalizeDiagPath(path, rootDir)}:${ln}:${col}:${code}:${severity}:${normalizeDiagMessage(message, rootDir)}`,
      );
      continue;
    }
    const g = globalRe.exec(line);
    if (g) {
      const [, severity, code, message] = g;
      set.add(`<global>:${code}:${severity}:${normalizeDiagMessage(message, rootDir)}`);
    }
  }
  return [...set].sort();
}

/**
 * Order-independent equality of two normalized diagnostic SETs (both already
 * sorted by `parseDiagnosticSet`). Used by the warm-incremental workload to assert
 * a type-changing dependency edit actually TRANSITIONED the dependent's diagnostics
 * (an unchanged set ⇒ the edit was ignored / the dependent was not rechecked).
 */
export function diagnosticSetsEqual(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false;
  const as = [...a].sort();
  const bs = [...b].sort();
  return as.every((v, i) => v === bs[i]);
}

/** Remove the on-disk build state so a cold run is genuinely cold + per-side isolated. */
function cleanBuildState(corpusDir: string): void {
  rmSync(join(corpusDir, ".out"), { recursive: true, force: true });
}

// ── Per-side working-tree isolation ──────────────────────────────────────────
/** A side's working corpus root + its solution tsconfig. */
export interface SideCorpus {
  readonly dir: string;
  readonly rootTsconfig: string;
}

// Track which work dirs this process has already materialized, so the per-side
// COPY happens once (the first on-disk-cache workload for a side), not per run.
const _materializedWorkDirs = new Set<string>();

/**
 * Materialize a side's private COPY of the corpus tree at `workDir` (once per
 * process). The copy is content-identical to the source — only the build /
 * output / cache state produced INTO it is per-side, so a candidate run cannot
 * warm or perturb the baseline run. Excludes a stale `.out` / `node_modules`.
 */
function materializeSideWorkTree(workDir: string, sourceDir: string): void {
  if (_materializedWorkDirs.has(workDir)) return;
  rmSync(workDir, { recursive: true, force: true });
  mkdirSync(workDir, { recursive: true });
  cpSync(sourceDir, workDir, {
    recursive: true,
    filter: (src) => {
      const b = basename(src);
      return b !== ".out" && b !== "node_modules";
    },
  });
  _materializedWorkDirs.add(workDir);
}

/** Reset the per-process work-tree materialization memo (test/teardown hook). */
export function resetSideWorkTrees(): void {
  _materializedWorkDirs.clear();
}

/**
 * The side's working corpus. With `ctx.workDir` set, returns the side's private
 * copy (materializing it on first use); otherwise the shared corpus dir.
 */
export function sideCorpus(ctx: WorkloadContext): SideCorpus {
  if (!ctx.workDir) {
    return { dir: ctx.corpus.dir, rootTsconfig: ctx.corpus.rootTsconfig };
  }
  materializeSideWorkTree(ctx.workDir, ctx.corpus.dir);
  return { dir: ctx.workDir, rootTsconfig: join(ctx.workDir, "tsconfig.json") };
}

/**
 * An `EnsuredCorpus` view rooted at the side's working tree (for the LSP
 * workloads, which take an `EnsuredCorpus`). Identical to `ctx.corpus` when no
 * `workDir` is set.
 */
export function sideCorpusView(ctx: WorkloadContext): EnsuredCorpus {
  if (!ctx.workDir) return ctx.corpus;
  const { dir } = sideCorpus(ctx);
  return {
    ...ctx.corpus,
    dir,
    rootTsconfig: join(dir, "tsconfig.json"),
    appTsconfig: join(dir, "app", "tsconfig.json"),
    kernelTsconfig: join(dir, "kernel", "tsconfig.json"),
  };
}

// ── AXIS A — native compiler codegen throughput (per-side child) ────────────
/** The axis-A child invocation (the per-side native spawn the seam runs). */
export interface AxisAChildInvocation {
  readonly execPath: string;
  /** The full child argv, including `--native <root> --corpus <dir> --threads <n>`. */
  readonly argv: readonly string[];
  readonly cwd: string;
}

/** The axis-A child-runner seam (defaults to spawning the real per-side child). */
export type AxisAChildRunner = (inv: AxisAChildInvocation) => AxisAChildSample;

/** Spawn the real per-side `axis-a-child` process and read its one JSON sample. */
const defaultAxisAChildRunner: AxisAChildRunner = (inv) => {
  const r = spawnSync(inv.execPath, [...inv.argv], {
    cwd: inv.cwd,
    encoding: "utf-8",
    timeout: 30 * 60 * 1000,
    windowsHide: true,
    maxBuffer: 64 * 1024 * 1024,
  });
  const nativeIdx = inv.argv.indexOf("--native");
  const nativeRoot = nativeIdx >= 0 ? inv.argv[nativeIdx + 1] : "(unknown)";
  assertSpawnSyncHealthy(r, `axis-a child (native ${nativeRoot})`);
  if (r.status !== 0) {
    throw new Error(
      `axis-a child (native ${nativeRoot}) exited ${r.status}: ${String(r.stderr ?? "").trim()}`,
    );
  }
  const line = String(r.stdout ?? "")
    .trim()
    .split("\n")
    .filter((l) => l.trim().startsWith("{"))
    .pop();
  if (!line) throw new Error(`axis-a child produced no JSON sample (native ${nativeRoot})`);
  return JSON.parse(line) as AxisAChildSample;
};

export const axisACodegen: Workload = {
  id: "axis-a-codegen",
  axis: "A",
  title: "AXIS A — native compiler carrier codegen (per-side child, audited)",
  interactive: false,
  available(ctx) {
    const entry = nativeEntry(nativeRootOf(ctx));
    if (!existsSync(entry)) {
      return { ok: false, reason: `@verter/native build not found at ${entry} (run build:native)` };
    }
    return { ok: true };
  },
  async runOnce(ctx) {
    const nativeRoot = nativeRootOf(ctx);
    // Run the child with the benchmark package as cwd so `--import tsx` resolves
    // regardless of how the parent gate/runner was launched (tsx is a benchmark
    // devDep, not a root dependency). Each side passes ITS OWN `--native` root,
    // so the self-comparison loads each build's own native — never one build's
    // native on both sides.
    const benchPkgRoot = join(__dirname, "..", "..");
    const argv = [
      "--import",
      "tsx",
      AXIS_A_CHILD,
      "--native",
      nativeRoot,
      "--corpus",
      ctx.corpus.dir,
      "--threads",
      String(ctx.threads),
    ];
    const runChild = ctx.axisAChildRunner ?? defaultAxisAChildRunner;
    const s = runChild({ execPath: process.execPath, argv, cwd: benchPkgRoot });

    // The child already represents every unavailable/missing audit field as
    // `null` (never `0`); pass it through verbatim so the gate's presence rail
    // sees a genuine missing datum rather than a coerced zero.
    const attribution: OverheadAttribution = {
      codegenMs: s.codegenMs,
      sourcemapMs: s.sourcemapMs,
      // The PRESENT codegen-side emit aggregate (codegen + source-map build). Both
      // phases are emitted by the compile audit record; their sum is the measurable
      // portion of the non-checker budget the axis-A `codegen_time_ratio` gates.
      // Nullness propagates: either phase missing ⇒ the aggregate is null.
      codegenSourcemapMs:
        s.codegenMs != null && s.sourcemapMs != null ? s.codegenMs + s.sourcemapMs : null,
      parseTransformTransportMs: s.parseTransformTransportMs,
      nonCheckerMs: s.nonCheckerMs,
      outputBytes: s.outputBytes,
      sourceMapBytes: s.sourceMapBytes,
      codeTransformOps: s.codeTransformOps,
      peakRssBytes: s.peakRssBytes,
    };
    return {
      totalMs: s.totalMs,
      attribution,
      rssBytes: s.peakRssBytes,
      // The scalar-sourced axis-A metrics; the non-checker time + source-map
      // bytes are read from `attribution` (the audited compile split), not
      // duplicated here.
      metrics: {
        // carrierCount = audited compiles emitting output (a real codegen signal),
        // gated as a two-sided invariant; sfcCount is the corpus SFC count it is
        // expected to equal (surfaced for the carrier-coverage cross-check).
        carrierCount: s.carrierCount,
        sfcCount: s.sfcCount,
        filesPerSec: s.filesPerSec,
      },
      // Candidate-vs-baseline carrier CONTENT equality (the gate's
      // contentEqualityGated rail): a content hash over every generated IDE carrier
      // + its source-map. A codegen change that preserves output_bytes + carrierCount
      // but alters the emitted CONTENT trips this even though the byte/count
      // invariants pass — the carrier-content correctness signal. A MISSING carrier (the child
      // reports `carrierContentHash: null` because a carrier had no IDE code/source-
      // map) maps to an EMPTY set — never a coerced empty-string hash — so the gate's
      // content-equality presence rail hard-fails a full run instead of comparing two
      // both-sides-missing runs as an equal hash.
      contentSets: { carrierContent: s.carrierContentHash != null ? [s.carrierContentHash] : [] },
    };
  },
};

// ── AXIS B/1 — cold full-project carrier typecheck ──────────────────────────
export const coldTypecheck: Workload = {
  id: "cold-typecheck",
  axis: "B",
  title: "AXIS B/1 — cold full-project carrier typecheck (verter-tsc + tsgo, fresh process)",
  interactive: false,
  available(ctx) {
    const bin = findBinary("verter-tsc", ctx.binDir);
    if (!bin) return { ok: false, reason: "verter-tsc binary not found (build crates/verter_tsc)" };
    return { ok: true };
  },
  async runOnce(ctx) {
    const bin = findBinary("verter-tsc", ctx.binDir)!;
    const { dir, rootTsconfig } = sideCorpus(ctx);
    cleanBuildState(dir); // genuinely cold + per-side isolated
    const t0 = performance.now();
    const r = await spawnWithRssSampling(
      bin,
      ["-b", rootTsconfig, "--noEmit"],
      dir,
      30 * 60 * 1000,
      ctx.spawnChild,
    );
    const totalMs = performance.now() - t0;
    const out = r.stdout + r.stderr;
    // A nonzero exit with NO parsed TS diagnostic is a startup/crash, not a
    // diagnostic exit — a both-sides-identical crash must not pass as a matching
    // empty diagnostic set.
    const exitCode = typecheckExitCodeOrThrow(r, out, "cold-typecheck (verter-tsc)");
    return {
      totalMs,
      attribution: null,
      rssBytes: r.peakRssBytes,
      metrics: { diagnostics: countTsErrors(out), exitCode },
      correctness: { exitCode, diagnostics: parseDiagnosticSet(out, dir) },
    };
  },
};

// ── AXIS B/2 — incremental re-typecheck (on-disk carrier cache) ─────────────
// NOT a retained Program — verter-tsc is always a fresh process; this measures
// the on-disk (tsbuildinfo) incremental re-typecheck after a small dependency
// edit. Its wall is reported-only; it is correctness-gated. The genuinely-warm
// signal is `warm-lsp-incremental` (the persistent LSP retains the Program).
export const incrementalRetypecheck: Workload = {
  id: "warm-incremental-retypecheck",
  axis: "B",
  title:
    "AXIS B/2 — incremental re-typecheck (on-disk carrier cache + type-changing dependency edit)",
  interactive: false,
  available(ctx) {
    const bin = findBinary("verter-tsc", ctx.binDir);
    if (!bin) return { ok: false, reason: "verter-tsc binary not found" };
    return { ok: true };
  },
  async runOnce(ctx) {
    const bin = findBinary("verter-tsc", ctx.binDir)!;
    const { dir, rootTsconfig } = sideCorpus(ctx);
    cleanBuildState(dir);
    // Populate the on-disk cache with THIS side's binary (own the warmth),
    // through the SAME hard-fail path the measured runs use: a spawn / timeout /
    // signal / abnormal-exit population is a HARD failure (it never silently
    // proceeds to measure a cold cache). Its diagnostic SET is the PRE-edit
    // baseline the measured edit's transition is asserted against.
    const popRun = await spawnWithRssSampling(
      bin,
      ["-b", rootTsconfig, "--noEmit"],
      dir,
      30 * 60 * 1000,
      ctx.spawnChild,
    );
    const popOut = popRun.stdout + popRun.stderr;
    typecheckExitCodeOrThrow(popRun, popOut, "warm-incremental warm-cache population (verter-tsc)");
    const preEditDiagnostics = parseDiagnosticSet(popOut, dir);

    const editTarget = firstAppTypeModule(dir);
    const original = readFileSync(editTarget, "utf-8");
    // A TYPE-CHANGING dependency edit: retype the shared `id` field the dependent
    // SFCs USE numerically (each module's own `recompute(): number` returns
    // `props.id + ref0.id`, and importing modules declare `defaults: T#### =
    // { id: <number literal> }`), so retyping `id: number` → `id: string` MUST
    // make the dependents re-diagnose (TS2322 appears / clears). A non-forcing
    // edit (e.g. adding an UNUSED optional field) would let an implementation that
    // ignored the edit pass — the transition assertion below is the proof the
    // dependent was actually rechecked, not merely that the run exited.
    const edited = original.replace("id: number;", "id: string;");
    if (edited === original) {
      throw new Error(
        `incremental-retypecheck edit no-op'd on ${editTarget} — corpus shape drift (expected an \`id: number;\` field to retype)`,
      );
    }
    writeFileSync(editTarget, edited);
    try {
      const t0 = performance.now();
      const r = await spawnWithRssSampling(
        bin,
        ["-b", rootTsconfig, "--noEmit"],
        dir,
        30 * 60 * 1000,
        ctx.spawnChild,
      );
      const totalMs = performance.now() - t0;
      const out = r.stdout + r.stderr;
      const exitCode = typecheckExitCodeOrThrow(
        r,
        out,
        "warm-incremental re-typecheck (verter-tsc)",
      );
      const postEditDiagnostics = parseDiagnosticSet(out, dir);
      // The type-changing dependency edit MUST have altered the dependent diagnostic
      // SET. An implementation that ignored the edit (no recheck) re-reports the
      // PRE-edit set — a NON-transition is a hard workload failure, never a fast
      // pass. This is the correctness proof the measured "warm" recheck actually
      // responded to the dependency edit (the candidate-vs-baseline diagnostic-SET
      // equality is gated separately on the post-edit set below).
      if (diagnosticSetsEqual(preEditDiagnostics, postEditDiagnostics)) {
        throw new Error(
          `warm-incremental re-typecheck on ${editTarget}: the type-changing dependency edit did not alter the dependent diagnostic set ` +
            `(${postEditDiagnostics.length} diagnostics, unchanged from pre-edit) — the dependent was not rechecked, so the measured "warm" recheck proves nothing`,
        );
      }
      return {
        totalMs,
        attribution: null,
        rssBytes: r.peakRssBytes,
        metrics: { diagnostics: countTsErrors(out), exitCode },
        correctness: { exitCode, diagnostics: postEditDiagnostics },
      };
    } finally {
      writeFileSync(editTarget, original);
    }
  },
};

// ── AXIS B/2b — genuinely-warm via the PERSISTENT LSP ───────────────────────
function lspAvailable(ctx: WorkloadContext): { ok: boolean; reason?: string } {
  const bin = findBinary("verter-lsp", ctx.binDir);
  if (!bin) return { ok: false, reason: "verter-lsp binary not found (build crates/verter_lsp)" };
  return { ok: true };
}

export const warmLspIncremental: Workload = {
  id: "warm-lsp-incremental",
  axis: "B",
  title: "AXIS B/2b — warm incremental re-typecheck (persistent LSP, Program retained)",
  interactive: true,
  available: lspAvailable,
  async runOnce(ctx) {
    const { warmDependencyEditLatency } = await import("./lsp-driver.js");
    const s = await warmDependencyEditLatency(
      findBinary("verter-lsp", ctx.binDir)!,
      sideCorpusView(ctx),
      { ops: opsOf(ctx) },
    );
    const total = s.latencies.reduce((a, b) => a + b, 0);
    return {
      totalMs: total,
      attribution: null,
      // The LSP workloads drive an in-process client; the server's RSS is not
      // sampled here (report null, not a misleading 0). Not a gated metric.
      rssBytes: null,
      metrics: { edits: s.latencies.length },
      expectedOps: opsOf(ctx),
      distributions: { warmLatency: s.latencies },
      correctness: { exitCode: 0, diagnostics: s.diagnosticSet },
      behavioral: { affectedUris: s.affectedUrisMax, totalUris: s.totalUris },
    };
  },
};

// ── AXIS B/3 — single-file-edit latency (interactive) ───────────────────────
export const singleFileEditLatency: Workload = {
  id: "single-file-edit-latency",
  axis: "B",
  title: "AXIS B/3 — single-file-edit latency (edit active SFC → updated diagnostics)",
  interactive: true,
  available: lspAvailable,
  async runOnce(ctx) {
    const { editToDiagnosticsLatency } = await import("./lsp-driver.js");
    const s = await editToDiagnosticsLatency(
      findBinary("verter-lsp", ctx.binDir)!,
      sideCorpusView(ctx),
      { ops: opsOf(ctx) },
    );
    const total = s.latencies.reduce((a, b) => a + b, 0);
    return {
      totalMs: total,
      attribution: null,
      rssBytes: null,
      metrics: { edits: s.latencies.length },
      expectedOps: opsOf(ctx),
      distributions: { editLatency: s.latencies },
      correctness: { exitCode: 0, diagnostics: s.diagnosticSet },
      behavioral: { affectedUris: s.affectedUrisMax, totalUris: s.totalUris },
    };
  },
};

// ── AXIS B/4 — IDE query latency (hover + completion, separate) ──────────────
export const ideQueryLatency: Workload = {
  id: "ide-query-latency",
  axis: "B",
  title: "AXIS B/4 — IDE query latency (hover + completion, separate distributions)",
  interactive: true,
  available: lspAvailable,
  async runOnce(ctx) {
    const { ideQueryLatency: query } = await import("./lsp-driver.js");
    const s = await query(findBinary("verter-lsp", ctx.binDir)!, sideCorpusView(ctx), {
      ops: opsOf(ctx),
    });
    const total = [...s.hoverLatencies, ...s.completionLatencies].reduce((a, b) => a + b, 0);
    return {
      totalMs: total,
      attribution: null,
      rssBytes: null,
      metrics: { hoverHits: s.hoverHits, completionItems: s.completionItems },
      expectedOps: opsOf(ctx),
      distributions: { hoverLatency: s.hoverLatencies, completionLatency: s.completionLatencies },
      correctness: { exitCode: 0, diagnostics: s.diagnosticSet },
      // Candidate-vs-baseline CONTENT equality (the gate's contentEqualityGated
      // rail): the normalized hover text per probed position + the completion label
      // SET. A content divergence at a probed position is a correctness regression
      // even when the hover-hit / completion-item COUNTS match (parity alone cannot
      // catch a bogus-but-same-count answer).
      contentSets: {
        hoverContents: s.hoverContents,
        completionLabels: s.completionLabelSet,
      },
    };
  },
};

/** The full §2.7 workload set, in display order. */
export const ALL_WORKLOADS: readonly Workload[] = [
  axisACodegen,
  coldTypecheck,
  incrementalRetypecheck,
  warmLspIncremental,
  singleFileEditLatency,
  ideQueryLatency,
];

// ── helpers ─────────────────────────────────────────────────────────────────
/** The first app-project type module (the incremental-edit target). */
function firstAppTypeModule(dir: string): string {
  const cand = join(dir, "app", "m0040", "types.ts");
  if (existsSync(cand)) return cand;
  const out: string[] = [];
  const walk = (d: string): void => {
    for (const e of readdirSync(d, { withFileTypes: true, encoding: "utf-8" })) {
      if (e.name.startsWith(".")) continue;
      const p = join(d, e.name);
      if (e.isDirectory()) walk(p);
      else if (e.name === "types.ts") out.push(p);
    }
  };
  walk(join(dir, "app"));
  out.sort();
  if (out.length === 0) throw new Error("no app type module found for the incremental edit");
  return out[0];
}
