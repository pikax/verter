/**
 * The §2.7 perf workload runner.
 *
 * Runs the external-TS-engine workloads (the §2.7 set — one axis-A native
 * codegen workload plus the axis-B carrier-typecheck/LSP workloads) on the
 * committed synthetic-15k corpus at a fixed thread count, reporting median / p95
 * (interactive workloads additionally p50 / p99) plus the §2.7 overhead
 * attribution and the per-run PEAK RSS (max + median of per-run peaks; a true
 * steady-state figure is a deferred follow-up) per workload. This is the
 * standalone bench (the manager runs the full corpus); the same workload
 * producers back the self-referential gate.
 *
 * Usage:
 *   node --import tsx src/perf/run.ts [--runs N] [--threads N] [--json]
 *        [--smoke] [--workload <id>] [--out <file>]
 *
 *   --smoke  : use a SMALL corpus slice (NOT the gate corpus — for harness
 *              verification only). The full 15k run takes minutes.
 */
import { availableParallelism } from "node:os";
import { writeFileSync } from "node:fs";
import { ensureCorpus, type EnsuredCorpus } from "./corpus.js";
import {
  ALL_WORKLOADS,
  type Workload,
  type WorkloadContext,
  type WorkloadSample,
} from "./workloads.js";
import { resolveEngineVersion } from "./gate.js";
import { summarize, type SampleSummary } from "./stats.js";

// Default = the runner's available parallelism (os.availableParallelism(),
// logical), overridable via --threads; recorded in every result.
const DEFAULT_THREADS = availableParallelism();

interface RunnerOptions {
  runs: number;
  threads: number;
  json: boolean;
  smoke: boolean;
  workloadId?: string;
  out?: string;
}

function parseArgs(argv: string[]): RunnerOptions {
  const o: RunnerOptions = { runs: 5, threads: DEFAULT_THREADS, json: false, smoke: false };
  for (let i = 2; i < argv.length; i++) {
    const a = argv[i];
    if (a === "--runs") o.runs = Number(argv[++i]);
    else if (a === "--threads") o.threads = Number(argv[++i]);
    else if (a === "--json") o.json = true;
    else if (a === "--smoke") o.smoke = true;
    else if (a === "--workload") o.workloadId = argv[++i];
    else if (a === "--out") o.out = argv[++i];
  }
  return o;
}

export interface WorkloadResult {
  id: string;
  axis: "A" | "B";
  title: string;
  interactive: boolean;
  skipped: boolean;
  skipReason?: string;
  runs: number;
  totalMs: SampleSummary | null;
  /**
   * Attribution buckets aggregated as medians across runs (ms / bytes / ops).
   * EVERY field is `number | null`: a field with no present measurement across the
   * runs is `null` (UNAVAILABLE) — never a fabricated `0` (which would read as a
   * present, lower-is-better datum). The printer renders `(not sampled)` for null.
   */
  attribution: {
    codegenMs: number | null;
    sourcemapMs: number | null;
    parseTransformTransportMs: number | null;
    nonCheckerMs: number | null;
    outputBytes: number | null;
    sourceMapBytes: number | null;
    codeTransformOps: number | null;
  } | null;
  /**
   * Measured target-process RSS across runs (bytes), or null when no run sampled
   * RSS (e.g. the LSP workloads). `medianPeakRssBytes` is the MEDIAN of the
   * per-run PEAK samples — NOT a steady-state figure (true steady-state RSS is a
   * deferred follow-up; see baselines/block6.json `deferred`).
   */
  peakRssBytes: number | null;
  medianPeakRssBytes: number | null;
  metrics: Record<string, SampleSummary>;
  /** Pooled per-operation latency distributions (interactive/warm workloads). */
  latencyDistributions: Record<string, SampleSummary>;
}

export interface RunnerReport {
  timestamp: string;
  corpusHash: string;
  corpusFiles: number;
  isGateCorpus: boolean;
  threads: number;
  runsPerWorkload: number;
  tsgoVersion: string;
  results: WorkloadResult[];
}

/**
 * Discover the engine version for the result manifest — the WORKSPACE-ROOT
 * `typescript` (tsgo = the TS 7 native engine), resolved exactly like the gate's
 * `resolveEngineVersion`. It MUST NOT resolve this package's own older
 * `typescript` devDep (via `import.meta.url`), which would record a different,
 * non-canonical version than the gate pins.
 */
export function discoverTsgoVersion(): string {
  return resolveEngineVersion();
}

/**
 * Reduce per-run RSS samples to an honest pair: the max PEAK and the MEDIAN of
 * the per-run peaks. Samples that did not sample RSS (`null`, e.g. the LSP
 * workloads) are ignored; when NO run sampled RSS, both are `null` — never a
 * misleading `0`. This is PEAK RSS; a true steady-state figure is a deferred
 * follow-up (see baselines/block6.json `deferred`).
 */
export function summarizeRssSamples(samples: readonly WorkloadSample[]): {
  peak: number | null;
  medianPeak: number | null;
} {
  const rss = samples.map((s) => s.rssBytes).filter((v): v is number => v !== null);
  if (rss.length === 0) return { peak: null, medianPeak: null };
  return { peak: Math.max(...rss), medianPeak: summarize(rss).p50 };
}

/**
 * The median of the PRESENT (non-null) values across runs, or `null` when NONE is
 * present — NEVER a fabricated `0`. A missing/unavailable attribution measurement
 * (a deferred field, an audit-disabled run) must read as ABSENT, not as a present
 * `0` that would undercount a lower-is-better ratio in the harness summary.
 */
export function medianPresentOrNull(xs: readonly (number | null)[]): number | null {
  const present = xs.filter((v): v is number => typeof v === "number");
  return present.length ? summarize(present).p50 : null;
}

/**
 * Summarize a metric across runs using ONLY present, finite readings — a sample
 * MISSING the metric (or carrying a non-finite value) is OMITTED, never fabricated
 * as a present `0` (which would skew a lower-is-better summary, the same defect the
 * attribution `medianPresentOrNull` rail avoids). A metric key reaches the summary
 * only because at least one sample carries it, so the present set is non-empty.
 */
export function summarizeMetricSamples(values: readonly (number | undefined)[]): SampleSummary {
  const present = values.filter((v): v is number => typeof v === "number" && Number.isFinite(v));
  return summarize(present);
}

async function runWorkload(
  w: Workload,
  ctx: WorkloadContext,
  runs: number,
  quiet: boolean,
): Promise<WorkloadResult> {
  const avail = w.available(ctx);
  if (!avail.ok) {
    return {
      id: w.id,
      axis: w.axis,
      title: w.title,
      interactive: w.interactive,
      skipped: true,
      skipReason: avail.reason,
      runs: 0,
      totalMs: null,
      attribution: null,
      peakRssBytes: null,
      medianPeakRssBytes: null,
      metrics: {},
      latencyDistributions: {},
    };
  }

  const samples: WorkloadSample[] = [];
  for (let i = 0; i < runs; i++) {
    if (!quiet) process.stderr.write(`  [${w.id}] run ${i + 1}/${runs}…\r`);
    samples.push(await w.runOnce(ctx));
  }
  if (!quiet) process.stderr.write(`  [${w.id}] ${runs} runs done.            \n`);

  const totals = samples.map((s) => s.totalMs);
  const rss = summarizeRssSamples(samples);
  const attrs = samples
    .map((s) => s.attribution)
    .filter((a): a is NonNullable<typeof a> => a != null);

  // Per-metric summaries.
  const metricKeys = new Set<string>();
  for (const s of samples) for (const k of Object.keys(s.metrics)) metricKeys.add(k);
  const metrics: Record<string, SampleSummary> = {};
  for (const k of metricKeys) {
    // Summarize only PRESENT readings; a sample missing this metric is omitted, never
    // fabricated as a present 0 (which would skew the standalone display summary).
    metrics[k] = summarizeMetricSamples(samples.map((s) => s.metrics[k]));
  }

  // Attribution = median of each bucket's PRESENT values across runs. A bucket with
  // NO present value across the runs stays `null` (UNAVAILABLE) — never a fabricated
  // `0` (which would read as a present lower-is-better datum). The gate (gate.ts) is
  // the surface that FAILS a full run on a missing gated field; this standalone
  // bench is display-only and renders `(not sampled)` for a null bucket.
  const attribution =
    attrs.length > 0
      ? {
          codegenMs: medianPresentOrNull(attrs.map((a) => a.codegenMs)),
          sourcemapMs: medianPresentOrNull(attrs.map((a) => a.sourcemapMs)),
          parseTransformTransportMs: medianPresentOrNull(
            attrs.map((a) => a.parseTransformTransportMs),
          ),
          nonCheckerMs: medianPresentOrNull(attrs.map((a) => a.nonCheckerMs)),
          outputBytes: medianPresentOrNull(attrs.map((a) => a.outputBytes)),
          sourceMapBytes: medianPresentOrNull(attrs.map((a) => a.sourceMapBytes)),
          codeTransformOps: medianPresentOrNull(attrs.map((a) => a.codeTransformOps)),
        }
      : null;

  // Pool each per-operation latency distribution across runs and summarize the
  // REAL p50/p95/p99 over the pooled distribution (not a per-run collapse).
  const distKeys = new Set<string>();
  for (const s of samples) for (const k of Object.keys(s.distributions ?? {})) distKeys.add(k);
  const latencyDistributions: Record<string, SampleSummary> = {};
  for (const k of distKeys) {
    const pooled = samples.flatMap((s) => [...(s.distributions?.[k] ?? [])]);
    if (pooled.length) latencyDistributions[k] = summarize(pooled);
  }

  return {
    id: w.id,
    axis: w.axis,
    title: w.title,
    interactive: w.interactive,
    skipped: false,
    runs,
    totalMs: summarize(totals),
    attribution,
    peakRssBytes: rss.peak,
    medianPeakRssBytes: rss.medianPeak,
    metrics,
    latencyDistributions,
  };
}

export async function runAll(options: RunnerOptions): Promise<RunnerReport> {
  const corpus: EnsuredCorpus = await ensureCorpus(
    options.smoke
      ? { config: { fileCount: 200, moduleCount: 20, importsPerFile: 6, compositeModuleCount: 4 } }
      : {},
  );
  const ctx: WorkloadContext = { corpus, threads: options.threads, quiet: options.json };
  const workloads = options.workloadId
    ? ALL_WORKLOADS.filter((w) => w.id === options.workloadId)
    : ALL_WORKLOADS;

  const results: WorkloadResult[] = [];
  for (const w of workloads) {
    results.push(await runWorkload(w, ctx, options.runs, options.json));
  }

  return {
    timestamp: new Date().toISOString(),
    corpusHash: corpus.contentHash,
    corpusFiles: corpus.manifest.counts.totalFiles,
    isGateCorpus: corpus.isGateCorpus,
    threads: options.threads,
    runsPerWorkload: options.runs,
    tsgoVersion: discoverTsgoVersion(),
    results,
  };
}

// ── reporting ────────────────────────────────────────────────────────────────
const fmtMs = (ms: number) => (ms >= 1000 ? `${(ms / 1000).toFixed(2)}s` : `${ms.toFixed(2)}ms`);
const fmtMB = (b: number) => `${(b / (1024 * 1024)).toFixed(1)}MB`;
/** A null (unavailable / not-sampled) measurement renders as text, never a fake 0. */
const NOT_SAMPLED = "(not sampled)";
const fmtMsOrNA = (ms: number | null) => (ms === null ? NOT_SAMPLED : fmtMs(ms));
const fmtMbOrNA = (b: number | null) => (b === null ? NOT_SAMPLED : fmtMB(b));
const fmtKbOrNA = (b: number | null) => (b === null ? NOT_SAMPLED : `${(b / 1024).toFixed(0)}KB`);
const numOrNA = (n: number | null) => (n === null ? NOT_SAMPLED : String(n));

export function printReport(report: RunnerReport): void {
  const W = 86;
  console.log("\n" + "═".repeat(W));
  console.log(" External-TS-engine perf runner — §2.7 workloads");
  console.log("═".repeat(W));
  console.log(`  corpus hash : ${report.corpusHash}`);
  console.log(
    `  corpus      : ${report.corpusFiles} files${report.isGateCorpus ? " (gate corpus)" : " (SMOKE slice — not the gate corpus)"}`,
  );
  console.log(`  threads     : ${report.threads}`);
  console.log(`  runs        : ${report.runsPerWorkload} per workload`);
  console.log(`  tsgo        : ${report.tsgoVersion}`);
  console.log("─".repeat(W));

  for (const r of report.results) {
    console.log(`\n[axis ${r.axis}] ${r.title}`);
    if (r.skipped) {
      console.log(`  SKIPPED — ${r.skipReason}`);
      continue;
    }
    const t = r.totalMs!;
    if (r.interactive) {
      const dists = Object.entries(r.latencyDistributions);
      if (dists.length) {
        for (const [name, d] of dists) {
          console.log(
            `  ${name.padEnd(16)}: p50 ${fmtMs(d.p50)}  p95 ${fmtMs(d.p95)}  p99 ${fmtMs(d.p99)}  (n=${d.n})`,
          );
        }
      } else {
        console.log(`  latency: p50 ${fmtMs(t.p50)}  p95 ${fmtMs(t.p95)}  p99 ${fmtMs(t.p99)}`);
      }
    } else {
      console.log(`  wall   : median ${fmtMs(t.p50)}  p95 ${fmtMs(t.p95)}  p99 ${fmtMs(t.p99)}`);
    }
    if (r.attribution) {
      const a = r.attribution;
      console.log(
        `  attrib : codegen ${fmtMsOrNA(a.codegenMs)} | sourcemap ${fmtMsOrNA(a.sourcemapMs)} | ` +
          `parse/transform/transport ${fmtMsOrNA(a.parseTransformTransportMs)} | non-checker ${fmtMsOrNA(a.nonCheckerMs)}`,
      );
      console.log(
        `         output ${fmtKbOrNA(a.outputBytes)} | sourcemap ${fmtKbOrNA(a.sourceMapBytes)} | ` +
          `transform-ops ${numOrNA(a.codeTransformOps)}`,
      );
    }
    if (r.peakRssBytes !== null) {
      console.log(
        `  memory : peak ${fmtMB(r.peakRssBytes)} | median-peak ${fmtMbOrNA(r.medianPeakRssBytes)}`,
      );
    } else {
      console.log(`  memory : (not sampled for this workload)`);
    }
    const interesting = ["carrierCount", "diagnostics", "filesPerSec", "completionItems"];
    const shown = interesting
      .filter((k) => r.metrics[k])
      .map((k) => `${k}=${r.metrics[k].p50.toFixed(0)}`);
    if (shown.length) console.log(`  metrics: ${shown.join("  ")}`);
  }
  console.log("\n" + "═".repeat(W) + "\n");
}

async function main(): Promise<void> {
  const options = parseArgs(process.argv);
  const report = await runAll(options);
  if (options.json) {
    const json = JSON.stringify(report, null, 2);
    if (options.out) writeFileSync(options.out, json);
    else console.log(json);
  } else {
    printReport(report);
    if (options.out) writeFileSync(options.out, JSON.stringify(report, null, 2));
  }
}

const invokedDirectly = process.argv[1]?.replace(/\\/g, "/").endsWith("perf/run.ts");
if (invokedDirectly) {
  main().catch((e) => {
    console.error(e);
    process.exit(1);
  });
}
