/**
 * The self-referential perf-regression gate (the CI perf gate).
 *
 * Runs a CANDIDATE Verter build against a PINNED-BASELINE Verter build on the
 * SAME runner / job / corpus / tsgo / thread count, INTERLEAVED, and compares
 * the RATIO of each side's statistic (median for time/throughput/RSS, the named
 * percentile for tail latency) with a bootstrap 95% CI — never absolute
 * milliseconds, which are too noisy on shared CI hardware.
 *
 * Both axes are Verter-vs-Verter-baseline and vize-FREE:
 *  - AXIS A — native-compiler codegen regression, gated on the signals the
 *    audited in-process compile actually emits: codegen throughput
 *    (`compile_throughput_ratio`), the codegen(+source-map) emit time
 *    (`codegen_time_ratio`), source-map bytes (`source_map_bytes`), output bytes
 *    (`output_bytes_ratio`), and the carrier output count + coverage
 *    (`generated_carrier_count`). Each side runs in its OWN child process loading
 *    THAT side's `@verter/native` build, so the self-comparison is genuine.
 *    Axis-A per-PID peak RSS and the FULL non-checker aggregate are honestly
 *    DEFERRED (see the manifest `deferred` section), never faked.
 *  - AXIS B — the tsgo carrier-typecheck/LSP regression: cold + incremental
 *    re-typecheck (exit-code + diagnostic-set correctness; child/engine RSS
 *    deferred), the genuinely warm persistent-LSP signal, and the interactive
 *    latency distributions + behavioral invariants.
 *
 * A one-sided gated metric (`higher-is-better` / `lower-is-better`) FAILS only
 * when `ratio > threshold` AND the bootstrap 95% CI lower bound on the ratio also
 * exceeds the threshold. An `invariant` metric is TWO-SIDED: it ALSO fails on a
 * DROP — `ratio < 1/threshold` AND the 95% CI UPPER bound `< 1/threshold` — so a
 * correctness-bearing count/byte-size that SHRINKS (skipped work) is a regression,
 * not a perf win. On a FULL (non-smoke)
 * run, a skipped/unavailable workload, a degenerate (all-zero) gated metric, an
 * engine-version mismatch, or an armed run that fell back to self-check are all
 * hard FAILS — the gate never reads a misconfiguration or missing instrument as
 * green. The Rust-requiring axis-B attribution (per-phase split, carrier
 * counts, warm-Program reuse) is honestly DEFERRED (see the manifest `deferred`
 * section), not faked.
 *
 * Usage:
 *   node --import tsx src/perf/gate.ts
 *     [--candidate-bin <dir>] [--baseline-bin <dir>]          # binary builds
 *     [--candidate-native <pkgRoot>] [--baseline-native <pkgRoot>]  # axis-A native
 *     [--samples N] [--threads N] [--ops N] [--smoke] [--out <artifact.json>]
 */
import { availableParallelism, tmpdir } from "node:os";
import { writeFileSync, readFileSync, rmSync, realpathSync } from "node:fs";
import { join, dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { createRequire } from "node:module";
import { ensureCorpus, type EnsuredCorpus } from "./corpus.js";
import {
  ALL_WORKLOADS,
  resetSideWorkTrees,
  type AxisAChildRunner,
  type Workload,
  type WorkloadContext,
  type WorkloadSample,
} from "./workloads.js";
import {
  ratioDecision,
  throughputRatioDecision,
  invariantRatioDecision,
  type RatioDecision,
  type RatioStatistic,
} from "./stats.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const VERTER_ROOT = resolve(__dirname, "..", "..", "..", "..");
const BASELINE_MANIFEST = join(__dirname, "..", "..", "baselines", "block6.json");

// ── Manifest schema ──────────────────────────────────────────────────────────
/** Where a gated metric reads its sample set from a `WorkloadSample`. */
export type MetricSource =
  | { kind: "scalar"; key: string }
  | { kind: "distribution"; key: string }
  | { kind: "rss" }
  | { kind: "attribution"; key: string }
  | { kind: "total_wall" };

export interface MetricSpec {
  threshold: number;
  /**
   * `higher-is-better` / `lower-is-better` gate a one-sided regression;
   * `invariant` gates a TWO-SIDED equality (candidate must stay within the
   * threshold tolerance of baseline in EITHER direction) — used for
   * correctness-bearing counts (e.g. generated carriers) where a DROP is a
   * regression (missing work), not a perf win.
   */
  direction: "higher-is-better" | "lower-is-better" | "invariant";
  statistic: RatioStatistic;
  source: MetricSource;
  note?: string;
}

export interface BehavioralSpec {
  /**
   * Diagnostic-publication-locality bound: (published-diagnostic URIs)/totalUris
   * ≤ this after a single-file edit. A publication proxy, NOT real
   * invalidation/recheck (deferred — see the manifest `deferred`).
   */
  maxAffectedUriFraction?: number;
}

/**
 * A per-sample COVERAGE invariant: the produced-count metric (`actual`) must
 * EQUAL the expected-count metric (`expected`) on every sample, on BOTH sides. A
 * subset compile (fewer carriers than SFCs) FAILS a full run even when the
 * candidate/baseline ratio is ~1.0 (both sides skipping the same work). This is a
 * within-sample equality, orthogonal to the candidate-vs-baseline `invariant`
 * ratio metric.
 */
export interface CoverageSpec {
  /** Metric key for the produced count (e.g. `carrierCount`). */
  actual: string;
  /** Metric key for the expected count it must equal (e.g. `sfcCount`). */
  expected: string;
  /** Free-text provenance/honesty note (manifest-only; not read by the gate). */
  note?: string;
}

export interface WorkloadSpec {
  axis: "A" | "B";
  title: string;
  /** Metrics that DECIDE the gate's pass/fail. */
  gated: Record<string, MetricSpec>;
  /**
   * Informational metrics that are measured and SURFACED in the report but NEVER
   * gate (e.g. cold/warm `total_wall_time_ratio`, which is tsgo-checker-dominated
   * and noisy). A `reported` metric is a sibling of `gated`, NOT a `reportedOnly`
   * flag inside `gated` — keeping fake-passing metrics out of the gated schema.
   */
  reported?: Record<string, MetricSpec>;
  /** Gate exit-code + diagnostic-SET equality between candidate and baseline. */
  correctnessGated?: boolean;
  /**
   * Names of `WorkloadSample.contentSets` keys that gate candidate-vs-baseline
   * SET equality (the same candidate-vs-baseline equality discipline as
   * `correctnessGated`, but for an observable-output content SET rather than the
   * diagnostic set). A content divergence at a probed position / per-compile output
   * — e.g. an IDE hover whose text differs, a completion label-set that diverges, a
   * carrier/source-map whose CONTENT changed even at an unchanged byte count — is a
   * correctness regression. Each named set must be non-empty + equal across every
   * candidate AND baseline sample on a full run; smoke tolerates a missing set.
   */
  contentEqualityGated?: string[];
  behavioral?: BehavioralSpec;
  /** Per-sample produced-vs-expected count coverage invariant (e.g. carriers == SFCs). */
  coverage?: CoverageSpec;
  /** Free-text provenance/honesty note (manifest-only; not read by the gate). */
  note?: string;
}

export interface DeferredMetric {
  metric: string;
  reason: string;
  requiresRust: string;
}

export interface BaselineManifest {
  methodologyVersion: string;
  baselineRef: string;
  corpusHash: string;
  tsgoVersion: string;
  samplesPerSide: number;
  workloads: Record<string, WorkloadSpec>;
  deferred?: DeferredMetric[];
}

const KNOWN_DIRECTIONS = new Set<string>(["higher-is-better", "lower-is-better", "invariant"]);
const KNOWN_STATISTICS = new Set<string>(["median", "mean", "p50", "p95", "p99"]);
const KNOWN_SOURCE_KINDS = new Set<string>([
  "scalar",
  "distribution",
  "rss",
  "attribution",
  "total_wall",
]);
const SOURCE_KINDS_REQUIRING_KEY = new Set<string>(["scalar", "distribution", "attribution"]);

function isPlainObject(v: unknown): v is Record<string, unknown> {
  return v !== null && typeof v === "object" && !Array.isArray(v);
}

function validateMetricSpec(metric: unknown, where: string): void {
  if (!isPlainObject(metric)) {
    throw new Error(`perf manifest: ${where} is not a metric object`);
  }
  const { threshold, direction, statistic, source } = metric;
  if (typeof threshold !== "number" || !Number.isFinite(threshold) || threshold <= 0) {
    throw new Error(
      `perf manifest: ${where}.threshold must be a finite positive number (got ${JSON.stringify(threshold)})`,
    );
  }
  if (typeof direction !== "string" || !KNOWN_DIRECTIONS.has(direction)) {
    throw new Error(
      `perf manifest: ${where}.direction is unknown (got ${JSON.stringify(direction)}; expected one of ${[...KNOWN_DIRECTIONS].join(", ")})`,
    );
  }
  if (typeof statistic !== "string" || !KNOWN_STATISTICS.has(statistic)) {
    throw new Error(
      `perf manifest: ${where}.statistic is unknown (got ${JSON.stringify(statistic)}; expected one of ${[...KNOWN_STATISTICS].join(", ")})`,
    );
  }
  const kind = isPlainObject(source) ? source.kind : undefined;
  if (typeof kind !== "string" || !KNOWN_SOURCE_KINDS.has(kind)) {
    throw new Error(
      `perf manifest: ${where}.source.kind is unknown (got ${JSON.stringify(kind)}; expected one of ${[...KNOWN_SOURCE_KINDS].join(", ")})`,
    );
  }
  if (SOURCE_KINDS_REQUIRING_KEY.has(kind)) {
    const key = (source as Record<string, unknown>).key;
    if (typeof key !== "string" || key.length === 0) {
      throw new Error(
        `perf manifest: ${where}.source of kind '${kind}' requires a non-empty string key`,
      );
    }
  }
}

function validateMetricMap(map: unknown, where: string): void {
  if (!isPlainObject(map)) {
    throw new Error(`perf manifest: ${where} must be an object map of metric specs`);
  }
  for (const [name, metric] of Object.entries(map)) {
    validateMetricSpec(metric, `${where}.${name}`);
  }
}

/**
 * Strictly validate a raw (parsed-JSON) perf manifest and return it typed. A
 * malformed metric silently DISABLED a gate before this existed: an unknown
 * `statistic` fell back to median, an unknown `direction` to lower-is-better, and a
 * missing/NaN `threshold` made `ratio > threshold` false (inert). The committed
 * manifest is validated on load (`readBaselineManifest`) so every malformation is a
 * hard throw — never a coerced default. Documentation-only extra top-level keys
 * (`note`/`predicate`/…) are tolerated; only the fields the gate reads are checked.
 */
export function validateManifest(raw: unknown): BaselineManifest {
  if (!isPlainObject(raw)) {
    throw new Error("perf manifest: not an object");
  }
  for (const key of ["methodologyVersion", "baselineRef", "corpusHash", "tsgoVersion"] as const) {
    const v = raw[key];
    if (typeof v !== "string" || v.length === 0) {
      throw new Error(
        `perf manifest: ${key} must be a non-empty string (got ${JSON.stringify(v)})`,
      );
    }
  }
  if (
    typeof raw.samplesPerSide !== "number" ||
    !Number.isInteger(raw.samplesPerSide) ||
    raw.samplesPerSide <= 0
  ) {
    throw new Error(
      `perf manifest: samplesPerSide must be a positive integer (got ${JSON.stringify(raw.samplesPerSide)})`,
    );
  }
  const workloads = raw.workloads;
  if (!isPlainObject(workloads) || Object.keys(workloads).length === 0) {
    throw new Error("perf manifest: declares no workloads");
  }
  const knownIds = new Set(ALL_WORKLOADS.map((w) => w.id));
  for (const [id, wl] of Object.entries(workloads)) {
    if (!knownIds.has(id)) {
      throw new Error(
        `perf manifest: unknown workload id '${id}' (not one of ${[...knownIds].join(", ")})`,
      );
    }
    if (!isPlainObject(wl)) {
      throw new Error(`perf manifest: workload '${id}' is not an object`);
    }
    if (wl.axis !== "A" && wl.axis !== "B") {
      throw new Error(
        `perf manifest: workload '${id}'.axis must be "A" or "B" (got ${JSON.stringify(wl.axis)})`,
      );
    }
    if (typeof wl.title !== "string" || wl.title.length === 0) {
      throw new Error(`perf manifest: workload '${id}'.title must be a non-empty string`);
    }
    validateMetricMap(wl.gated, `workload '${id}'.gated`);
    if (wl.reported !== undefined) validateMetricMap(wl.reported, `workload '${id}'.reported`);
    const gatedNonEmpty = Object.keys(wl.gated as Record<string, unknown>).length > 0;
    const reportedNonEmpty = isPlainObject(wl.reported) && Object.keys(wl.reported).length > 0;
    const contentGated =
      Array.isArray(wl.contentEqualityGated) && wl.contentEqualityGated.length > 0;
    if (!gatedNonEmpty && !reportedNonEmpty && !contentGated && wl.correctnessGated !== true) {
      throw new Error(
        `perf manifest: workload '${id}' is inert — it declares no gated metric, no reported metric, and no correctness/content signal (it measures nothing)`,
      );
    }
  }
  if (raw.deferred !== undefined) {
    if (!Array.isArray(raw.deferred)) {
      throw new Error("perf manifest: deferred must be an array");
    }
    raw.deferred.forEach((d, i) => {
      if (
        !isPlainObject(d) ||
        typeof d.metric !== "string" ||
        typeof d.reason !== "string" ||
        typeof d.requiresRust !== "string"
      ) {
        throw new Error(
          `perf manifest: deferred[${i}] must carry string metric/reason/requiresRust`,
        );
      }
    });
  }
  return raw as unknown as BaselineManifest;
}

export function readBaselineManifest(): BaselineManifest {
  return validateManifest(JSON.parse(readFileSync(BASELINE_MANIFEST, "utf-8")));
}

/**
 * The engine the carrier typecheck drives (tsgo = TypeScript 7 native), pinned
 * in the WORKSPACE-ROOT devDeps. Resolve from the root, NOT from this package
 * (which carries its own older `typescript` for tooling) — they differ.
 */
export function resolveEngineVersion(root: string = VERTER_ROOT): string {
  try {
    const req = createRequire(join(root, "package.json"));
    const v = (req("typescript/package.json") as { version: string }).version;
    return `typescript@${v}`;
  } catch {
    return "unknown";
  }
}

// ── Report shape ─────────────────────────────────────────────────────────────
export interface MetricResult {
  metric: string;
  threshold: number;
  direction: string;
  statistic: RatioStatistic;
  reportedOnly: boolean;
  decision: RatioDecision | null;
  degenerate: boolean;
  /** Raw per-side sample sets (in order) — persisted for reproducibility. */
  candidateSamples: number[];
  baselineSamples: number[];
  unavailableReason?: string;
}
export interface CorrectnessResult {
  equal: boolean;
  detail: string;
  candidate: { exitCode: number; diagnostics: string[] } | null;
  baseline: { exitCode: number; diagnostics: string[] } | null;
}
export interface BehavioralResult {
  candidate: { affectedUris: number; totalUris: number } | null;
  baseline: { affectedUris: number; totalUris: number } | null;
  fraction: number | null;
  withinFraction: boolean;
}
export interface CoverageResult {
  equal: boolean;
  detail: string;
}
export interface ContentEqualityResult {
  key: string;
  equal: boolean;
  detail: string;
}
export interface WorkloadGateResult {
  id: string;
  axis: "A" | "B";
  title: string;
  skipped: boolean;
  skipReason?: string;
  metrics: MetricResult[];
  correctness?: CorrectnessResult;
  behavioral?: BehavioralResult;
  coverage?: CoverageResult;
  contentEquality?: ContentEqualityResult[];
}
export interface GateReport {
  timestamp: string;
  pass: boolean;
  mode: "self-check" | "armed";
  corpusHash: string;
  baselineRef: string;
  candidateBin: string;
  baselineBin: string;
  candidateNative: string;
  baselineNative: string;
  threads: number;
  samplesPerSide: number;
  tsgoVersion: string;
  engineResolved: string;
  baselineEngineResolved: string;
  selfCheck: boolean;
  results: WorkloadGateResult[];
  failures: string[];
  warnings: string[];
}

/**
 * The artifact written when the gate fails BEFORE it can produce a `GateReport`
 * (a corpus-hash mismatch, an import/setup failure, a build problem). The CI
 * `always()` upload must never find a missing file, so a pre-report failure is
 * still serialized as a `pass: false` error report.
 */
export interface GateErrorReport {
  timestamp: string;
  pass: false;
  error: string;
  stack?: string;
}

export function buildGateErrorReport(err: unknown): GateErrorReport {
  const e = err instanceof Error ? err : new Error(String(err));
  return { timestamp: new Date().toISOString(), pass: false, error: e.message, stack: e.stack };
}

// ── Pure evaluation (the testable core) ──────────────────────────────────────
export interface SideSamples {
  readonly samples: readonly WorkloadSample[];
}
export interface WorkloadEvaluationInput {
  readonly id: string;
  readonly spec: WorkloadSpec;
  readonly available: boolean;
  readonly unavailableReason?: string;
  readonly candidate: SideSamples;
  readonly baseline: SideSamples;
}
export interface GateEvaluationInput {
  readonly manifest: BaselineManifest;
  readonly workloads: readonly WorkloadEvaluationInput[];
  readonly smoke: boolean;
  readonly selfCheck: boolean;
  /** Engine resolved from the CANDIDATE workspace. */
  readonly engineResolved: string;
  /** Engine resolved from the BASELINE worktree (== candidate in self-check). */
  readonly baselineEngineResolved: string;
  readonly meta: {
    readonly corpusHash: string;
    /** Raw side identities (undefined ⇒ resolved from the default target/). */
    readonly candidateBin?: string;
    readonly baselineBin?: string;
    readonly candidateNative?: string;
    readonly baselineNative?: string;
    /**
     * The baseline worktree root (`--baseline-root`). An armed (non-self-check)
     * run REQUIRES it — the baseline tsgo engine is resolved from this worktree,
     * never borrowed from the candidate root. Undefined in self-check/smoke.
     */
    readonly baselineRoot?: string;
    /**
     * The candidate workspace root. When present in an armed run, the guard asserts
     * `baselineRoot` resolves to a DIFFERENT location (a separate baseline worktree,
     * never the candidate root). Undefined in self-check/smoke.
     */
    readonly candidateRoot?: string;
    readonly threads: number;
    readonly samplesPerSide: number;
  };
}

/**
 * A gated metric's per-side sample set WITH presence tracking. A missing scalar /
 * attribution value, a `null` RSS sample, or an absent/empty per-operation
 * distribution is counted in `missing` — NEVER coerced to `0` — so partially
 * missing instrumentation (some samples carry the payload, some do not) is caught
 * per sample, not only when the WHOLE vector degenerates to all-zero.
 */
export interface SampledMetric {
  readonly values: number[];
  /** Samples whose gated payload for this metric was absent (uncoerced). */
  readonly missing: number;
  readonly total: number;
}

/**
 * Whether a `<= 0` reading for this source is a MISSING datum, not a real
 * measurement. RSS, wall-time, and the attribution ms/byte fields are magnitudes
 * that are never legitimately ≤ 0; a gated scalar count of 0 means no work
 * happened — never a real "win". So a `<= 0` here is counted as missing
 * (uncoerced), exactly like a `null`/absent payload. Distribution element values
 * are NOT filtered on 0 — per-sample COMPLETENESS is enforced by length instead.
 */
function zeroIsMissing(kind: MetricSource["kind"]): boolean {
  return kind === "rss" || kind === "total_wall" || kind === "attribution" || kind === "scalar";
}

function sampleSetFor(side: SideSamples, source: MetricSource): SampledMetric {
  const total = side.samples.length;
  const values: number[] = [];
  let missing = 0;
  const zMissing = zeroIsMissing(source.kind);
  // A scalar/rss/attribution/total_wall reading is MISSING when it is null /
  // absent / non-numeric, OR (for these magnitude/count sources) finite and ≤ 0.
  // A non-finite (NaN/Infinity) value is still pushed so the whole-vector
  // `hasNonFinite` check reports it distinctly as broken instrumentation.
  const take = (v: number | null | undefined): void => {
    if (typeof v !== "number") {
      missing++;
      return;
    }
    if (zMissing && Number.isFinite(v) && v <= 0) {
      missing++;
      return;
    }
    values.push(v);
  };
  switch (source.kind) {
    case "scalar":
      for (const s of side.samples) take(s.metrics[source.key]);
      break;
    case "rss":
      for (const s of side.samples) take(s.rssBytes);
      break;
    case "attribution":
      for (const s of side.samples) {
        const a = s.attribution as Record<string, number | null> | null;
        take(a == null ? undefined : a[source.key]);
      }
      break;
    case "total_wall":
      for (const s of side.samples) take(s.totalMs);
      break;
    case "distribution":
      for (const s of side.samples) {
        const d = s.distributions?.[source.key];
        // A per-operation distribution is COMPLETE only when it is present, non-
        // empty, AND (when the sample carries an expected op count) has EXACTLY
        // that many entries. A sample that returned 49 latencies for 50 requested
        // ops is partial — counted as missing, never pooled into the percentile.
        const expected = s.expectedOps;
        const complete = d != null && d.length > 0 && (expected == null || d.length === expected);
        if (complete) values.push(...d);
        else missing++;
      }
      break;
    default: {
      // Exhaustiveness rail: every `MetricSource` kind is handled above. A source
      // whose `kind` slipped past `validateManifest` must FAIL LOUDLY here rather
      // than fall through to an empty sample vector (which the gate would read as a
      // degenerate-but-present, false-green metric).
      const _exhaustive: never = source;
      throw new Error(`unknown metric source kind: ${String((_exhaustive as MetricSource).kind)}`);
    }
  }
  return { values, missing, total };
}

const isAllZero = (xs: number[]): boolean => xs.length === 0 || xs.every((v) => v === 0);
/** A NaN/Infinity sample is a broken/missing measurement — never a real datum. */
const hasNonFinite = (xs: number[]): boolean => xs.some((v) => !Number.isFinite(v));

/**
 * A resolved, comparable key for a filesystem path used by the armed-mode
 * distinctness guard. Resolves symlinks + the canonical case via
 * `realpathSync.native` when the path exists, else normalizes via `resolve`; then
 * strips a trailing separator, unifies separators, and case-folds. So a trailing
 * slash, a case-only difference (case-insensitive FS), or a symlink can NOT make
 * two equal paths look distinct and evade the candidate-vs-baseline guard. The
 * fold is deliberately conservative — for a distinctness guard, erring toward
 * "same" (reject) is safe.
 */
function resolvedPathKey(p: string): string {
  let resolved: string;
  try {
    resolved = realpathSync.native(p);
  } catch {
    resolved = resolve(p);
  }
  return resolved
    .replace(/[\\/]+$/, "")
    .split(/[\\/]/)
    .join("/")
    .toLowerCase();
}
/** Whether two paths resolve to the SAME filesystem location (see `resolvedPathKey`). */
function sameResolvedPath(a: string, b: string): boolean {
  return resolvedPathKey(a) === resolvedPathKey(b);
}

function decideRatio(cand: number[], base: number[], spec: MetricSpec): RatioDecision {
  const opts = { statistic: spec.statistic };
  // EXHAUSTIVE over the direction enum — an unknown direction is a hard throw, never
  // a silent lower-is-better default that would disable a higher-is-better/invariant
  // gate. validateManifest already rejects an unknown direction at load; this is the
  // matching defense at evaluation.
  switch (spec.direction) {
    case "higher-is-better":
      return throughputRatioDecision(cand, base, spec.threshold, opts);
    case "invariant":
      return invariantRatioDecision(cand, base, spec.threshold, opts);
    case "lower-is-better":
      return ratioDecision(cand, base, spec.threshold, opts);
    default: {
      const exhaustive: never = spec.direction;
      throw new Error(`perf gate: unknown metric direction ${JSON.stringify(exhaustive)}`);
    }
  }
}

type CorrectnessDatum = { exitCode: number; diagnostics: string[] };
type BehavioralDatum = { affectedUris: number; totalUris: number };

function correctnessEqual(
  a: CorrectnessDatum,
  b: CorrectnessDatum,
): { equal: boolean; detail: string } {
  if (a.exitCode !== b.exitCode) {
    return { equal: false, detail: `exit code ${a.exitCode} != baseline ${b.exitCode}` };
  }
  const as = [...a.diagnostics].sort();
  const bs = [...b.diagnostics].sort();
  const equal = as.length === bs.length && as.every((v, i) => v === bs[i]);
  if (!equal) {
    const onlyA = as.filter((x) => !bs.includes(x)).length;
    const onlyB = bs.filter((x) => !as.includes(x)).length;
    return {
      equal: false,
      detail: `diagnostic set differs (+${onlyA} candidate-only / +${onlyB} baseline-only)`,
    };
  }
  return { equal: true, detail: "match" };
}

/**
 * Validate EVERY sample (not just the first): on a full run every correctness-
 * gated sample on both sides must carry data, and every candidate AND baseline
 * sample must agree (deterministic same-corpus typecheck ⇒ one diagnostic set).
 * A later-sample diagnostic/exit-code divergence is a real regression that the
 * first-sample-only check used to miss.
 */
function evaluateCorrectness(wl: WorkloadEvaluationInput, smoke: boolean): CorrectnessResult {
  const candAll = wl.candidate.samples.map((s) => s.correctness ?? null);
  const baseAll = wl.baseline.samples.map((s) => s.correctness ?? null);
  const firstC = candAll.find((x): x is CorrectnessDatum => x !== null) ?? null;
  const firstB = baseAll.find((x): x is CorrectnessDatum => x !== null) ?? null;
  const anyMissing =
    candAll.length === 0 ||
    baseAll.length === 0 ||
    candAll.some((x) => x === null) ||
    baseAll.some((x) => x === null);
  if (anyMissing) {
    // On a full run a missing correctness sample is broken instrumentation; on a
    // smoke run we tolerate it (the predicate self-check is not a correctness gate).
    if (!smoke) {
      return {
        equal: false,
        detail:
          "missing correctness data (exit code + diagnostic set) on a correctness-gated sample (full run)",
        candidate: firstC,
        baseline: firstB,
      };
    }
    if (!firstC || !firstB) {
      return {
        equal: true,
        detail: "smoke: no correctness data",
        candidate: firstC,
        baseline: firstB,
      };
    }
  }
  // Reference = baseline's first sample; every candidate AND baseline sample must
  // match it (collapses to a single distinct correctness value across the run).
  const ref = firstB!;
  for (const x of [...candAll, ...baseAll]) {
    if (x === null) continue;
    const cmp = correctnessEqual(x, ref);
    if (!cmp.equal) {
      return { equal: false, detail: cmp.detail, candidate: firstC, baseline: firstB };
    }
  }
  return {
    equal: true,
    detail: "exit code + diagnostic set match across all samples",
    candidate: firstC,
    baseline: firstB,
  };
}

/** Order-independent equality of two string SETs (sorted compare). */
function stringSetsEqual(a: readonly string[], b: readonly string[]): boolean {
  if (a.length !== b.length) return false;
  const as = [...a].sort();
  const bs = [...b].sort();
  return as.every((v, i) => v === bs[i]);
}

/**
 * Candidate-vs-baseline content-SET equality for ONE `contentSets` key (the same
 * discipline as {@link evaluateCorrectness}, applied to an observable-output set —
 * IDE hover contents / completion labels, axis-A carrier+source-map content
 * hashes). On a FULL run every candidate AND baseline sample must carry a
 * non-empty set for this key, and every set must equal the baseline's first
 * (a deterministic same-corpus query/compile yields ONE content set; a divergence
 * is a correctness regression). Smoke tolerates a missing set.
 */
function evaluateContentEquality(
  wl: WorkloadEvaluationInput,
  key: string,
  smoke: boolean,
): ContentEqualityResult {
  const get = (s: WorkloadSample): string[] | null => {
    const set = s.contentSets?.[key];
    return set != null && set.length > 0 ? set : null;
  };
  const candAll = wl.candidate.samples.map(get);
  const baseAll = wl.baseline.samples.map(get);
  const firstB = baseAll.find((x): x is string[] => x !== null) ?? null;
  const anyMissing =
    candAll.length === 0 ||
    baseAll.length === 0 ||
    candAll.some((x) => x === null) ||
    baseAll.some((x) => x === null);
  if (anyMissing) {
    if (!smoke) {
      return {
        key,
        equal: false,
        detail: `missing/empty ${key} content set on a content-equality-gated sample (full run) — broken/no-result instrumentation`,
      };
    }
    if (!firstB) return { key, equal: true, detail: `smoke: no ${key} content data` };
  }
  const ref = firstB!;
  for (const x of [...candAll, ...baseAll]) {
    if (x === null) continue;
    if (!stringSetsEqual(x, ref)) {
      const extra = x.filter((v) => !ref.includes(v)).length;
      const missing = ref.filter((v) => !x.includes(v)).length;
      return {
        key,
        equal: false,
        detail: `${key} content set differs from baseline (+${extra} divergent / -${missing} absent vs baseline)`,
      };
    }
  }
  return { key, equal: true, detail: `${key} content set matches across all samples` };
}

/**
 * Per-sample coverage invariant: on a full run the produced count (`actual`) must
 * EQUAL the expected count (`expected`) on EVERY candidate AND baseline sample. A
 * subset compile (actual < expected) fails even at a ~1.0 candidate/baseline ratio
 * (both sides skipping the same work); a missing count or a non-positive expected
 * is broken instrumentation. Smoke tolerates it.
 */
function evaluateCoverage(
  wl: WorkloadEvaluationInput,
  cov: CoverageSpec,
  smoke: boolean,
): CoverageResult {
  if (smoke) return { equal: true, detail: "smoke: coverage not gated" };
  const sides: ["candidate" | "baseline", readonly WorkloadSample[]][] = [
    ["candidate", wl.candidate.samples],
    ["baseline", wl.baseline.samples],
  ];
  for (const [name, samples] of sides) {
    for (const s of samples) {
      const actual = s.metrics[cov.actual];
      const expected = s.metrics[cov.expected];
      if (typeof actual !== "number" || typeof expected !== "number") {
        return {
          equal: false,
          detail: `${name} sample missing ${cov.actual}/${cov.expected} — broken carrier-coverage instrumentation`,
        };
      }
      if (expected <= 0) {
        return {
          equal: false,
          detail: `${name} sample reports ${cov.expected}=${expected} (no expected work — broken corpus/instrumentation)`,
        };
      }
      if (actual !== expected) {
        return {
          equal: false,
          detail: `${name} sample produced ${actual}/${expected} (${cov.actual} != ${cov.expected} — subset/over compile, not full coverage)`,
        };
      }
    }
  }
  return { equal: true, detail: `${cov.actual} == ${cov.expected} on every sample` };
}

const localityFraction = (x: BehavioralDatum): number =>
  x.totalUris > 0 ? x.affectedUris / x.totalUris : 0;
/** The worst (max locality fraction) behavioral sample on a side, or null. */
function worstBehavioral(side: SideSamples): BehavioralDatum | null {
  let worst: BehavioralDatum | null = null;
  for (const s of side.samples) {
    if (!s.behavioral) continue;
    if (worst === null || localityFraction(s.behavioral) > localityFraction(worst)) {
      worst = s.behavioral;
    }
  }
  return worst;
}

export function evaluateGate(input: GateEvaluationInput): GateReport {
  const { manifest, smoke, selfCheck, engineResolved, baselineEngineResolved } = input;
  const failures: string[] = [];
  const warnings: string[] = [];
  const engineFail = (msg: string): void => {
    if (smoke) warnings.push(msg);
    else failures.push(msg);
  };

  // Armed mode requires a PINNED, immutable baseline: a full 40-hex commit hash.
  // The PENDING placeholder is the ONLY sanctioned unarmed sentinel; any other
  // non-SHA value ("", "TODO", a branch name, a moving tag) is an unpinned ref
  // that must NEVER arm the gate (it would float the baseline). Both force
  // self-check mode; a non-self-check (distinct-side) run against an unpinned
  // baseline is a hard fail — physically incapable of a green armed verdict.
  const isPendingRef = /^\s*PENDING/i.test(manifest.baselineRef);
  const isPinnedSha = /^[0-9a-f]{40}$/i.test(manifest.baselineRef.trim());
  const armedExpected = isPinnedSha;
  const mode: "self-check" | "armed" = selfCheck || !armedExpected ? "self-check" : "armed";

  // Arming discipline — an unarmed run is a loud self-check, never a green armed gate.
  if (!armedExpected) {
    if (isPendingRef) {
      warnings.push(
        "perf gate NOT ARMED — baselineRef is PENDING; this is a same-commit predicate self-check only " +
          "(not a regression gate). Arm via a `perf: refresh baseline` change that pins a 40-hex baseline SHA.",
      );
    } else {
      warnings.push(
        `perf gate NOT ARMED — baselineRef ${JSON.stringify(manifest.baselineRef)} is not a full 40-hex commit hash; ` +
          "an unpinned/moving ref (a branch, a tag, a placeholder) cannot arm the gate. Pin it to the baseline commit hash via a `perf: refresh baseline` change.",
      );
    }
    // An unpinned baseline handed DISTINCT side paths (a non-self-check
    // invocation) is a misconfiguration: there is no pinned baseline build to
    // compare against, so it can ONLY run as a same-commit self-check. Hard-fail
    // rather than silently present a green gate on an unpinned baseline.
    if (!selfCheck) {
      const why = isPendingRef
        ? "PENDING (unarmed)"
        : `not a pinned 40-hex SHA (${JSON.stringify(manifest.baselineRef)})`;
      failures.push(
        `baselineRef is ${why} but the gate ran in non-self-check mode — an unpinned baseline can only run as a same-commit self-check, never an armed comparison; refusing a false green.`,
      );
    }
  }
  // An armed manifest that fell back to self-check ⇒ the pinned baseline build is
  // missing ⇒ refuse a false green.
  if (armedExpected && selfCheck) {
    failures.push(
      "baselineRef is armed but the gate ran in self-check mode (the pinned baseline build is absent) — refusing a false green.",
    );
  }
  // An armed (real-ref, non-self-check) comparison MUST resolve the baseline tsgo
  // engine from a SEPARATE baseline worktree (`--baseline-root`); without it the
  // baseline engine would be borrowed from the candidate root, letting an armed
  // run pass while the baseline worktree pins a different tsgo. Self-check + smoke
  // need no baseline root (there the baseline IS the candidate root).
  if (armedExpected && !selfCheck && !input.meta.baselineRoot) {
    failures.push(
      "armed run requires --baseline-root (the baseline worktree whose tsgo engine is resolved and compared) — refusing to resolve the baseline engine from the candidate root.",
    );
  }
  // An armed comparison must use DISTINCT, present sides on BOTH axes — a missing
  // or equal side silently self-compares (candidate-vs-candidate) while reporting
  // armed. The `?? candidate` fallbacks that re-introduced that bug are removed.
  if (armedExpected && !selfCheck) {
    const { candidateBin, baselineBin, candidateNative, baselineNative, candidateRoot } =
      input.meta;
    // Distinctness compares RESOLVED paths, not raw strings — a trailing slash, a
    // case-only difference, or a symlink must not make two equal paths look distinct
    // and slip a candidate-vs-candidate self-comparison through as armed.
    if (!candidateBin || !baselineBin || sameResolvedPath(candidateBin, baselineBin)) {
      failures.push(
        `armed run requires DISTINCT candidate/baseline binary dirs (candidate=${candidateBin ?? "(missing)"}, baseline=${baselineBin ?? "(missing)"}) — refusing an axis-B candidate-vs-candidate comparison reported as armed.`,
      );
    }
    if (!candidateNative || !baselineNative || sameResolvedPath(candidateNative, baselineNative)) {
      failures.push(
        `armed run requires DISTINCT candidate/baseline native roots (candidate=${candidateNative ?? "(missing)"}, baseline=${baselineNative ?? "(missing)"}) — refusing an axis-A candidate-vs-candidate comparison reported as armed.`,
      );
    }
    // The baseline worktree must be SEPARATE from the candidate root — a baselineRoot
    // that resolves to the candidate root would resolve the baseline tsgo engine from
    // the candidate's own worktree (the very self-comparison --baseline-root prevents).
    if (
      input.meta.baselineRoot &&
      candidateRoot &&
      sameResolvedPath(input.meta.baselineRoot, candidateRoot)
    ) {
      failures.push(
        `armed run requires a baseline root SEPARATE from the candidate root (both resolve to ${candidateRoot}) — refusing to resolve the baseline engine from the candidate worktree.`,
      );
    }
  }

  // Sample floor — a full run below the manifest's samplesPerSide is underpowered
  // and must not publish a "result".
  if (!smoke && input.meta.samplesPerSide < manifest.samplesPerSide) {
    failures.push(
      `underpowered run: ${input.meta.samplesPerSide} samples/side < manifest floor ${manifest.samplesPerSide} — refusing to publish an underpowered gate result.`,
    );
  }

  // Engine pin: both worktrees' resolved engines must equal the manifest version
  // AND each other — a baseline-ref change could resolve a different tsgo in the
  // baseline worktree.
  if (engineResolved !== manifest.tsgoVersion) {
    engineFail(
      `engine version mismatch: candidate resolved ${engineResolved} != manifest tsgoVersion ${manifest.tsgoVersion}`,
    );
  }
  if (baselineEngineResolved !== manifest.tsgoVersion) {
    engineFail(
      `baseline engine version mismatch: baseline resolved ${baselineEngineResolved} != manifest tsgoVersion ${manifest.tsgoVersion}`,
    );
  }
  if (engineResolved !== baselineEngineResolved) {
    engineFail(
      `engine mismatch between worktrees: candidate ${engineResolved} != baseline ${baselineEngineResolved}`,
    );
  }

  // Completeness — every declared manifest workload must actually be evaluated; a
  // dropped/renamed producer or a manifest typo must not vanish silently.
  if (!smoke) {
    const evaluated = new Set(input.workloads.map((w) => w.id));
    const missing = Object.keys(manifest.workloads).filter((k) => !evaluated.has(k));
    if (missing.length > 0) {
      failures.push(
        `manifest workload(s) not evaluated on a full run: ${missing.join(", ")} — a dropped/renamed producer or a manifest typo (refusing a silently-incomplete green).`,
      );
    }
  }

  const results: WorkloadGateResult[] = [];
  for (const wl of input.workloads) {
    const { id, spec } = wl;
    if (!wl.available) {
      if (!smoke) {
        failures.push(
          `${id}: required workload unavailable on a full run — ${wl.unavailableReason ?? "skipped"} (CI misconfig / missing binary / broken instrumentation).`,
        );
      }
      results.push({
        id,
        axis: spec.axis,
        title: spec.title,
        skipped: true,
        skipReason: wl.unavailableReason,
        metrics: [],
      });
      continue;
    }

    const metricResults: MetricResult[] = [];
    // Evaluate ONE metric. A `reported` metric is surfaced in the result for
    // information but NEVER contributes a failure — only `gated` metrics decide the
    // gate's pass/fail.
    const evalMetric = (metric: string, mspec: MetricSpec, reported: boolean): void => {
      const cand = sampleSetFor(wl.candidate, mspec.source);
      const base = sampleSetFor(wl.baseline, mspec.source);
      // Per-sample presence: ANY sample missing this payload is broken/partial
      // instrumentation (a missing value is NOT coerced to 0). It outranks the
      // whole-vector all-zero/non-finite checks.
      const missingPayload = cand.missing > 0 || base.missing > 0;
      const allZero = isAllZero(cand.values) || isAllZero(base.values);
      const nonFinite = hasNonFinite(cand.values) || hasNonFinite(base.values);
      const unusable = missingPayload || allZero || nonFinite;
      const decision = unusable ? null : decideRatio(cand.values, base.values, mspec);
      const unusableReason = missingPayload
        ? `gated payload missing (null / ≤0, never coerced to 0) on ${cand.missing}/${cand.total} candidate + ${base.missing}/${base.total} baseline samples — partial/missing instrumentation`
        : nonFinite
          ? "non-finite (NaN/Infinity) vector — broken instrumentation"
          : allZero
            ? "degenerate (all-zero) vector — missing instrumentation"
            : undefined;
      metricResults.push({
        metric,
        threshold: mspec.threshold,
        direction: mspec.direction,
        statistic: mspec.statistic,
        reportedOnly: reported,
        decision,
        degenerate: unusable,
        candidateSamples: cand.values,
        baselineSamples: base.values,
        unavailableReason: unusableReason,
      });
      if (reported) return; // reported metrics are informational — they never gate
      if (unusable) {
        if (!smoke) {
          failures.push(
            `${id}.${metric}: ${
              missingPayload
                ? `gated payload MISSING (null / ≤0, never coerced to 0) on ${cand.missing}/${cand.total} candidate + ${base.missing}/${base.total} baseline samples — partial/missing`
                : nonFinite
                  ? "non-finite (NaN/Infinity) — broken"
                  : "degenerate (all-zero) — missing"
            } instrumentation on a full run (not a pass).`,
          );
        }
      } else if (decision && decision.fail) {
        failures.push(
          `${id}.${metric}: ratio=${decision.statisticRatio.toFixed(3)} lb95=${decision.lowerBound95.toFixed(3)} ub95=${decision.upperBound95.toFixed(3)} (direction ${mspec.direction}, threshold ${mspec.threshold})`,
        );
      }
    };
    // GATED metrics decide pass/fail; REPORTED metrics are surfaced but never gate.
    for (const [metric, mspec] of Object.entries(spec.gated)) evalMetric(metric, mspec, false);
    for (const [metric, mspec] of Object.entries(spec.reported ?? {}))
      evalMetric(metric, mspec, true);

    let correctness: CorrectnessResult | undefined;
    if (spec.correctnessGated) {
      correctness = evaluateCorrectness(wl, smoke);
      if (!correctness.equal)
        failures.push(`${id}: correctness regression — ${correctness.detail}`);
    }

    let coverage: CoverageResult | undefined;
    if (spec.coverage) {
      coverage = evaluateCoverage(wl, spec.coverage, smoke);
      if (!coverage.equal) failures.push(`${id}: carrier-coverage regression — ${coverage.detail}`);
    }

    let contentEquality: ContentEqualityResult[] | undefined;
    if (spec.contentEqualityGated && spec.contentEqualityGated.length > 0) {
      contentEquality = [];
      for (const key of spec.contentEqualityGated) {
        const r = evaluateContentEquality(wl, key, smoke);
        contentEquality.push(r);
        if (!r.equal) failures.push(`${id}: ${key} content regression — ${r.detail}`);
      }
    }

    let behavioral: BehavioralResult | undefined;
    if (spec.behavioral) {
      // Gate on the WORST observed candidate sample, not the first; a later
      // whole-project publication burst must not hide behind a local first sample.
      const c = worstBehavioral(wl.candidate);
      const b = worstBehavioral(wl.baseline);
      // Behavioral payloads are required on EVERY candidate AND baseline sample —
      // the artifact reports candidate/baseline behavioral results, so a full run
      // missing EITHER side's data is broken instrumentation, not a pass.
      const candMissing = wl.candidate.samples.some((s) => !s.behavioral) || c === null;
      const baseMissing = wl.baseline.samples.some((s) => !s.behavioral) || b === null;
      let fraction: number | null = null;
      let withinFraction = true;
      if (!smoke && (candMissing || baseMissing)) {
        const which =
          candMissing && baseMissing
            ? "candidate+baseline"
            : candMissing
              ? "candidate"
              : "baseline";
        failures.push(
          `${id}: behavioral data (affected-URI count) missing on a behavioral-gated ${which} sample (full run).`,
        );
        withinFraction = false;
      } else if (spec.behavioral.maxAffectedUriFraction !== undefined) {
        const thr = spec.behavioral.maxAffectedUriFraction;
        // A missing/degenerate locality denominator (totalUris <= 0, or no behavioral
        // datum) on EITHER side cannot certify the locality invariant: it must NOT be
        // coerced to fraction 0 (perfect locality) and the threshold check must NOT be
        // silently skipped. A full run HARD-FAILS; smoke warns.
        const candDenomBad = !c || !(c.totalUris > 0);
        const baseDenomBad = !b || !(b.totalUris > 0);
        if (candDenomBad || baseDenomBad) {
          const which =
            candDenomBad && baseDenomBad
              ? "candidate+baseline"
              : candDenomBad
                ? "candidate"
                : "baseline";
          const msg = `${id}: locality denominator (totalUris) is missing or zero on the ${which} behavioral sample — cannot certify diagnostic-publication locality (no denominator).`;
          if (smoke) {
            warnings.push(msg);
          } else {
            failures.push(msg);
            withinFraction = false;
          }
        }
        const candFrac = c && c.totalUris > 0 ? c.affectedUris / c.totalUris : null;
        const baseFrac = b && b.totalUris > 0 ? b.affectedUris / b.totalUris : null;
        fraction = candFrac; // reported (candidate) worst-sample fraction
        // The locality invariant is ABSOLUTE, not a candidate-vs-baseline ratio:
        // a baseline sample that republishes ~the whole project is just as much a
        // violation as a candidate one. Enforce the bound on the WORST sample of
        // BOTH sides (the threshold was previously applied to the candidate only).
        if (candFrac !== null && candFrac > thr) {
          withinFraction = false;
          failures.push(
            `${id}: a single-file edit re-PUBLISHED diagnostics for ${c!.affectedUris}/${c!.totalUris} URIs (candidate worst-sample fraction ${candFrac.toFixed(3)} > ${thr}) — over-broad diagnostic-publication locality.`,
          );
        }
        if (baseFrac !== null && baseFrac > thr) {
          withinFraction = false;
          failures.push(
            `${id}: a single-file edit re-PUBLISHED diagnostics for ${b!.affectedUris}/${b!.totalUris} URIs (baseline worst-sample fraction ${baseFrac.toFixed(3)} > ${thr}) — over-broad diagnostic-publication locality.`,
          );
        }
      }
      behavioral = { candidate: c, baseline: b, fraction, withinFraction };
    }

    results.push({
      id,
      axis: spec.axis,
      title: spec.title,
      skipped: false,
      metrics: metricResults,
      correctness,
      behavioral,
      coverage,
      contentEquality,
    });
  }

  return {
    timestamp: new Date().toISOString(),
    pass: failures.length === 0,
    mode,
    corpusHash: input.meta.corpusHash,
    baselineRef: manifest.baselineRef,
    candidateBin: input.meta.candidateBin ?? "(workspace target/)",
    baselineBin: input.meta.baselineBin ?? "(workspace target/)",
    candidateNative: input.meta.candidateNative ?? "(packages/native)",
    baselineNative: input.meta.baselineNative ?? "(packages/native)",
    threads: input.meta.threads,
    samplesPerSide: input.meta.samplesPerSide,
    tsgoVersion: manifest.tsgoVersion,
    engineResolved,
    baselineEngineResolved: input.baselineEngineResolved,
    selfCheck,
    results,
    failures,
    warnings,
  };
}

// ── Measurement orchestration ────────────────────────────────────────────────
interface GateOptions {
  candidateBin?: string;
  baselineBin?: string;
  candidateNative?: string;
  baselineNative?: string;
  /** The baseline worktree root (its engine is resolved + compared too). */
  baselineRoot?: string;
  samples: number;
  threads: number;
  ops?: number;
  smoke: boolean;
  out?: string;
  /**
   * Injectable axis-A child runner (tests only; production leaves it undefined and
   * the workload spawns the real per-side native child). Threaded into BOTH side
   * contexts so an orchestration spec can prove the baseline side runs with the
   * DISTINCT `--baseline-native` root and that a synthetic regression fails.
   */
  axisAChildRunner?: AxisAChildRunner;
}

function parseArgs(argv: string[]): GateOptions {
  const o: GateOptions = { samples: 7, threads: availableParallelism(), smoke: false };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--candidate-bin") o.candidateBin = argv[++i];
    else if (a === "--baseline-bin") o.baselineBin = argv[++i];
    else if (a === "--candidate-native") o.candidateNative = argv[++i];
    else if (a === "--baseline-native") o.baselineNative = argv[++i];
    else if (a === "--baseline-root") o.baselineRoot = argv[++i];
    else if (a === "--samples") o.samples = Number(argv[++i]);
    else if (a === "--threads") o.threads = Number(argv[++i]);
    else if (a === "--ops") o.ops = Number(argv[++i]);
    else if (a === "--smoke") o.smoke = true;
    else if (a === "--out") o.out = argv[++i];
  }
  return o;
}

/** The resolved per-side measurement inputs `buildSideContexts` needs. */
export interface SideContextInputs {
  readonly corpus: EnsuredCorpus;
  readonly candidateBin?: string;
  readonly baselineBin?: string;
  readonly candidateNative?: string;
  readonly baselineNative?: string;
  readonly threads: number;
  readonly ops: number;
  readonly workRoot: string;
  /** Injectable axis-A child runner (tests only; production leaves it undefined). */
  readonly axisAChildRunner?: AxisAChildRunner;
}

/**
 * Build the candidate + baseline `WorkloadContext`s the gate measures. The
 * BASELINE side ALWAYS takes the baseline bin/native — there is NO candidate-side
 * fallback (a fallback would load the candidate's native on the baseline side and
 * silently self-compare while reporting armed). Extracted from `runGate` so the
 * `--baseline-native` → baseline-side wiring is unit-testable through
 * `runInterleaved` without materializing a corpus.
 */
export function buildSideContexts(i: SideContextInputs): {
  candidateCtx: WorkloadContext;
  baselineCtx: WorkloadContext;
} {
  const shared = {
    corpus: i.corpus,
    threads: i.threads,
    ops: i.ops,
    quiet: true,
    axisAChildRunner: i.axisAChildRunner,
  } as const;
  return {
    candidateCtx: {
      ...shared,
      binDir: i.candidateBin,
      nativeRoot: i.candidateNative,
      workDir: join(i.workRoot, "candidate"),
    },
    baselineCtx: {
      ...shared,
      binDir: i.baselineBin,
      nativeRoot: i.baselineNative,
      workDir: join(i.workRoot, "baseline"),
    },
  };
}

/**
 * Interleave a workload across the two builds (`baseline, candidate, candidate,
 * baseline`, repeated) so monotonic drift balances across the two sides.
 */
export async function runInterleaved(
  w: Workload,
  candidateCtx: WorkloadContext,
  baselineCtx: WorkloadContext,
  samples: number,
): Promise<{ candidate: SideSamples; baseline: SideSamples }> {
  const cand: WorkloadSample[] = [];
  const base: WorkloadSample[] = [];
  const pattern: ("b" | "c")[] = ["b", "c", "c", "b"];
  let bi = 0;
  let ci = 0;
  let p = 0;
  while (bi < samples || ci < samples) {
    const which = pattern[p % pattern.length];
    p++;
    if (which === "b" && bi < samples) {
      base.push(await w.runOnce(baselineCtx));
      bi++;
    } else if (which === "c" && ci < samples) {
      cand.push(await w.runOnce(candidateCtx));
      ci++;
    } else if (bi < samples) {
      base.push(await w.runOnce(baselineCtx));
      bi++;
    } else if (ci < samples) {
      cand.push(await w.runOnce(candidateCtx));
      ci++;
    }
  }
  return { candidate: { samples: cand }, baseline: { samples: base } };
}

export async function runGate(options: GateOptions): Promise<GateReport> {
  const manifest = readBaselineManifest();
  const corpus: EnsuredCorpus = await ensureCorpus(
    options.smoke
      ? { config: { fileCount: 200, moduleCount: 20, importsPerFile: 6, compositeModuleCount: 4 } }
      : {},
  );

  if (!options.smoke && corpus.contentHash !== manifest.corpusHash) {
    throw new Error(
      `Corpus hash ${corpus.contentHash} does not match baseline manifest ${manifest.corpusHash}.\n` +
        `A corpus change must refresh baselines/block6.json (corpusHash) in the SAME change — ` +
        `the integrity rail that keeps a corpus drift from reading as a perf delta.`,
    );
  }

  // NO candidate-side fallback for the baseline: an armed run with a missing
  // baseline bin/native must NOT silently self-compare (evaluateGate hard-fails
  // an armed run whose sides are missing or equal). In self-check mode all four
  // are undefined and resolve to the same default target/ + packages/native.
  const candBin = options.candidateBin;
  const baseBin = options.baselineBin;
  const candNative = options.candidateNative;
  const baseNative = options.baselineNative;
  const selfCheck = baseBin === candBin && baseNative === candNative;
  // Engine pin is verified for BOTH worktrees: a baseline-ref change could resolve
  // a different tsgo in the baseline worktree. An armed (non-self-check) run MUST
  // resolve the baseline engine from `--baseline-root`; it is NEVER borrowed from
  // the candidate root (that would let an armed run pass while the baseline pins a
  // different tsgo — evaluateGate hard-fails the missing root). In self-check the
  // baseline IS the candidate, so the candidate root is the correct source.
  const baselineEngineResolved = options.baselineRoot
    ? resolveEngineVersion(options.baselineRoot)
    : selfCheck
      ? resolveEngineVersion()
      : "(unresolved: --baseline-root required for an armed run)";
  const samples = options.smoke ? Math.min(options.samples, 4) : options.samples;
  const ops = options.ops ?? (options.smoke ? 8 : 50);

  // Per-side isolated working trees: the on-disk-cache workloads (verter-tsc +
  // the LSP workloads) operate on a private COPY of the corpus per side, so a
  // candidate run cannot warm or perturb the baseline run (and vice-versa).
  // Distinct dirs even in self-check mode, so the measurement stays apples-to-
  // apples isolated. They live in the OS tmp dir, never in the repo tree.
  const workRoot = join(tmpdir(), "verter-perf-work");
  rmSync(workRoot, { recursive: true, force: true });
  resetSideWorkTrees();
  const { candidateCtx, baselineCtx } = buildSideContexts({
    corpus,
    candidateBin: candBin,
    baselineBin: baseBin,
    candidateNative: candNative,
    baselineNative: baseNative,
    threads: options.threads,
    ops,
    workRoot,
    axisAChildRunner: options.axisAChildRunner,
  });

  const workloadInputs: WorkloadEvaluationInput[] = [];
  for (const w of ALL_WORKLOADS) {
    const spec = manifest.workloads[w.id];
    if (!spec) continue;
    const availC = w.available(candidateCtx);
    const availB = w.available(baselineCtx);
    if (!availC.ok || !availB.ok) {
      workloadInputs.push({
        id: w.id,
        spec,
        available: false,
        unavailableReason: availC.reason ?? availB.reason,
        candidate: { samples: [] },
        baseline: { samples: [] },
      });
      continue;
    }
    try {
      const { candidate, baseline } = await runInterleaved(w, candidateCtx, baselineCtx, samples);
      workloadInputs.push({ id: w.id, spec, available: true, candidate, baseline });
    } catch (e) {
      workloadInputs.push({
        id: w.id,
        spec,
        available: false,
        unavailableReason: `run failed: ${(e as Error).message}`,
        candidate: { samples: [] },
        baseline: { samples: [] },
      });
    }
  }

  const report = evaluateGate({
    manifest,
    workloads: workloadInputs,
    smoke: options.smoke,
    selfCheck,
    engineResolved: resolveEngineVersion(),
    baselineEngineResolved,
    meta: {
      corpusHash: corpus.contentHash,
      candidateBin: candBin,
      baselineBin: baseBin,
      candidateNative: candNative,
      baselineNative: baseNative,
      baselineRoot: options.baselineRoot,
      // The candidate engine is resolved from VERTER_ROOT (resolveEngineVersion()
      // with no arg), so that is the candidate root the baseline worktree must differ
      // from in an armed run.
      candidateRoot: VERTER_ROOT,
      threads: options.threads,
      samplesPerSide: samples,
    },
  });
  // Remove the per-side working trees (tmp only; never part of the repo tree).
  rmSync(workRoot, { recursive: true, force: true });
  resetSideWorkTrees();
  return report;
}

function printGate(report: GateReport): void {
  const W = 92;
  console.log("\n" + "═".repeat(W));
  console.log(" Self-referential perf-regression gate (candidate vs pinned baseline)");
  console.log("═".repeat(W));
  console.log(
    `  mode        : ${report.mode}${report.mode === "self-check" ? "  (candidate === baseline)" : ""}`,
  );
  console.log(`  corpus hash : ${report.corpusHash}`);
  console.log(`  baseline ref: ${report.baselineRef}`);
  console.log(`  candidate   : ${report.candidateBin}  [native ${report.candidateNative}]`);
  console.log(`  baseline    : ${report.baselineBin}  [native ${report.baselineNative}]`);
  console.log(
    `  engine      : candidate ${report.engineResolved} / baseline ${report.baselineEngineResolved} (manifest ${report.tsgoVersion})`,
  );
  console.log(`  samples/side: ${report.samplesPerSide}   threads: ${report.threads}`);
  console.log("─".repeat(W));
  // Column-0 so GitHub Actions recognizes the workflow command.
  for (const w of report.warnings) console.log(`::warning::${w}`);
  for (const r of report.results) {
    console.log(`\n[axis ${r.axis}] ${r.title}`);
    if (r.skipped) {
      console.log(`  SKIPPED — ${r.skipReason}`);
      continue;
    }
    for (const m of r.metrics) {
      if (m.degenerate) {
        // Neutral label — whether a degenerate gated metric is a hard fail
        // depends on full-vs-smoke; the authoritative verdict is in `failures`.
        console.log(`  [n/a   ] ${m.metric.padEnd(28)} ${m.unavailableReason}`);
        continue;
      }
      const d = m.decision!;
      const tag = m.reportedOnly ? "report" : d.fail ? "FAIL  " : "ok    ";
      console.log(
        `  [${tag}] ${m.metric.padEnd(28)} ${m.statistic} ratio=${d.statisticRatio.toFixed(3)} ` +
          `lb95=${d.lowerBound95.toFixed(3)} ub95=${d.upperBound95.toFixed(3)} thr=${m.threshold}`,
      );
    }
    if (r.correctness) {
      console.log(
        `  [${r.correctness.equal ? "ok    " : "FAIL  "}] correctness               ${r.correctness.detail}`,
      );
    }
    if (r.coverage) {
      console.log(
        `  [${r.coverage.equal ? "ok    " : "FAIL  "}] coverage                  ${r.coverage.detail}`,
      );
    }
    for (const ce of r.contentEquality ?? []) {
      console.log(
        `  [${ce.equal ? "ok    " : "FAIL  "}] content:${ce.key.padEnd(18)} ${ce.detail}`,
      );
    }
    if (r.behavioral) {
      const ok = r.behavioral.withinFraction;
      const frac = r.behavioral.fraction !== null ? r.behavioral.fraction.toFixed(3) : "n/a";
      console.log(
        `  [${ok ? "ok    " : "FAIL  "}] behavioral (locality)     affected-fraction=${frac}`,
      );
    }
  }
  console.log("\n" + "─".repeat(W));
  console.log(report.pass ? "  GATE: PASS" : `  GATE: FAIL (${report.failures.length})`);
  for (const f of report.failures) console.log(`    - ${f}`);
  console.log("═".repeat(W) + "\n");
}

async function main(): Promise<void> {
  const options = parseArgs(process.argv);
  let report: GateReport;
  try {
    report = await runGate(options);
  } catch (e) {
    // A pre-report failure must STILL emit the artifact so the always() upload is
    // never empty; record the reason, then exit nonzero.
    if (options.out) writeFileSync(options.out, JSON.stringify(buildGateErrorReport(e), null, 2));
    console.error(e);
    process.exit(2);
    return;
  }
  if (options.out) writeFileSync(options.out, JSON.stringify(report, null, 2));
  printGate(report);
  process.exit(report.pass ? 0 : 1);
}

const invokedDirectly = process.argv[1]?.replace(/\\/g, "/").endsWith("perf/gate.ts");
if (invokedDirectly) {
  main().catch((e) => {
    console.error(e);
    process.exit(2);
  });
}
