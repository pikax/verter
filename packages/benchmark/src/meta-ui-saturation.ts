/**
 * Meta-UI CPU saturation bench.
 *
 * Answers one question: **does the scheduler actually use the CPU when
 * resolving component metadata for a whole UI library?**
 *
 * The standard `bench:meta:ui` runner drives the interactive
 * single-request path one component at a time (and isolates each query
 * in its own child process), so at any instant ~1 core is busy and the
 * machine never spikes. That is by design — parallelism in Verter comes
 * from the *batch* path (`getComponentMetaBatch` →
 * `Scheduler::dispatch_meta_jobs` → `cpu_pool.install(|| par_iter)`),
 * which fans N independent component queries across the Rayon CPU pool
 * inside a single native call.
 *
 * This bench drives the SAME corpus two ways against cold sessions and
 * reports the honest "cores used" number for each:
 *
 *   cores used = (process CPU time during the call) / (wall time)
 *
 * `process.cpuUsage()` measures RUSAGE_SELF, which includes every thread
 * in the process — so the native Rayon workers' CPU time is counted.
 * A `cores used` near 1.0 means the work ran on a single core (no spike);
 * a value approaching `availableParallelism()` means the pool saturated
 * the machine.
 *
 *   pnpm --filter @verter/benchmark bench:meta:ui:saturation -- --ui-root=<dir>
 *
 * Requires the meta-UI corpus (run `bench:meta:ui:setup` first) and the
 * built native binding (`pnpm run build:native`). This is a developer
 * diagnostic, not a unit test — it is excluded from `pnpm test`.
 */

import { existsSync } from "node:fs";
import { createRequire } from "node:module";
import { availableParallelism } from "node:os";
import { dirname, resolve } from "node:path";
import { fileURLToPath, pathToFileURL } from "node:url";

import { buildCheckerConfig, parseMetaUiBenchArgs, prepareMetaUiProject } from "./meta-ui-bench.js";

const __filename = fileURLToPath(import.meta.url);
const __dirname = dirname(__filename);
const repoRoot = resolve(__dirname, "../../..");

/** Minimal structural view of the public `ComponentMetaSession`. */
interface MetaSessionLike {
  updateFile(filePath: string, source: string): void;
  getComponentMeta(filePath: string): Promise<MetaResult | null | undefined>;
  getComponentMetaBatch(filePaths: string[]): Promise<Array<MetaResult | null | undefined>>;
  close(): void;
}

interface MetaResult {
  props?: unknown[];
  events?: unknown[];
  slots?: unknown[];
  exposed?: unknown[];
}

interface ComponentMetaModule {
  openComponentMetaSession: (
    config: { root: string; config: Record<string, unknown> },
    checkerOptions?: Record<string, unknown>,
  ) => Promise<MetaSessionLike>;
  shutdownMetaRuntime: () => void;
}

/**
 * Load the component-meta module from source when available (mirrors the
 * worker's `loadVerterCompatModule`), falling back to the published
 * package. Loading source avoids resolving a stale `dist`.
 */
async function loadVerterMetaModule(): Promise<ComponentMetaModule> {
  const sourceEntry = resolve(repoRoot, "packages", "component-meta", "src", "index.ts");
  if (existsSync(sourceEntry)) {
    return (await import(pathToFileURL(sourceEntry).href)) as unknown as ComponentMetaModule;
  }
  const require = createRequire(import.meta.url);
  return require("@verter/component-meta") as ComponentMetaModule;
}

/** Match the per-query worker's checker options for fidelity. */
const CHECKER_OPTIONS: Record<string, unknown> = {
  forceUseTs: true,
  schema: { literalBooleanSchema: true },
  runtimeMode: "dedicated",
};

interface PassMeasurement {
  label: string;
  wallMs: number;
  cpuMs: number;
  coresUsed: number;
  componentsPerSec: number;
  resolved: number;
  total: number;
}

function hasSurface(meta: MetaResult | null | undefined): boolean {
  if (!meta) return false;
  return Boolean(
    meta.props?.length || meta.events?.length || meta.slots?.length || meta.exposed?.length,
  );
}

/** Run `fn` under a wall-clock + process-CPU-time measurement. */
async function measure(
  label: string,
  total: number,
  fn: () => Promise<number>,
): Promise<PassMeasurement> {
  globalThis.gc?.();
  const cpu0 = process.cpuUsage();
  const t0 = performance.now();
  const resolved = await fn();
  const wallMs = performance.now() - t0;
  const cpu1 = process.cpuUsage(cpu0);
  const cpuMs = (cpu1.user + cpu1.system) / 1000;
  return {
    label,
    wallMs,
    cpuMs,
    coresUsed: wallMs > 0 ? cpuMs / wallMs : 0,
    componentsPerSec: wallMs > 0 ? (total / wallMs) * 1000 : 0,
    resolved,
    total,
  };
}

function fmtMs(ms: number): string {
  return ms >= 1000 ? `${(ms / 1000).toFixed(2)}s` : `${ms.toFixed(0)}ms`;
}

function col(value: string, width: number): string {
  return value.length >= width ? value : value + " ".repeat(width - value.length);
}

function printHeader(): void {
  console.log(
    `  ${col("mode", 12)}  ${col("n", 5)}  ${col("wall", 9)}  ${col("thrupt", 9)}  ${col("cpu", 9)}  ${col("cores", 8)}  ${col("resolved", 9)}`,
  );
}

function printRow(m: PassMeasurement): void {
  console.log(
    `  ${col(m.label, 12)}  ${col(String(m.total), 5)}  ${col(fmtMs(m.wallMs), 9)}  ${col(`${Math.round(m.componentsPerSec)}/s`, 9)}  ` +
      `${col(fmtMs(m.cpuMs), 9)}  ${col(`${m.coresUsed.toFixed(2)}x`, 8)}  ${col(`${m.resolved}/${m.total}`, 9)}`,
  );
}

/** Parse repeated `--exclude=a,b` flags into lowercased substring tokens. */
function parseExcludes(argv: string[]): string[] {
  const tokens: string[] = [];
  for (const arg of argv) {
    if (arg.startsWith("--exclude=")) {
      for (const tok of arg.slice("--exclude=".length).split(",")) {
        const t = tok.trim().toLowerCase();
        if (t) tokens.push(t);
      }
    }
  }
  return tokens;
}

/** Parse `--seq-sample=N` (sequential-baseline size); null when absent. */
function parseSeqSample(argv: string[]): number | null {
  for (const arg of argv) {
    if (arg.startsWith("--seq-sample=")) {
      const n = Number.parseInt(arg.slice("--seq-sample=".length), 10);
      if (Number.isFinite(n) && n >= 0) return n;
    }
  }
  return null;
}

/** Evenly-spaced sample of `k` items across `items` (stable order). */
function sampleEvenly<T>(items: T[], k: number): T[] {
  if (k <= 0) return [];
  if (k >= items.length) return items.slice();
  const out: T[] = [];
  const stride = items.length / k;
  for (let i = 0; i < k; i += 1) out.push(items[Math.floor(i * stride)]);
  return out;
}

async function main(): Promise<void> {
  const argv = process.argv.slice(2);
  const args = parseMetaUiBenchArgs(argv);
  const prepared = prepareMetaUiProject(args);
  const cores = availableParallelism();

  // `--exclude=<csv>` drops components whose relative path contains any
  // token (case-insensitive). Use it to skip components that hang the
  // resolver — e.g. `--exclude=chatmessage` removes ChatMessage(s).vue.
  // One hanging component blocks the entire synchronous batch call, so
  // such components MUST be excluded for a whole-corpus stress run.
  const excludes = parseExcludes(argv);
  const kept = prepared.componentSnapshots.filter(
    (c) => !excludes.some((tok) => c.relativePath.toLowerCase().includes(tok)),
  );
  const dropped = prepared.componentSnapshots.length - kept.length;

  if (kept.length === 0) {
    console.error("No components to run (after exclusions). Did you run `bench:meta:ui:setup`?");
    process.exitCode = 1;
    return;
  }

  const checkerConfig = buildCheckerConfig(prepared, kept);
  const { openComponentMetaSession, shutdownMetaRuntime } = await loadVerterMetaModule();
  const allPaths = kept.map((c) => c.absolutePath);

  // The sequential pass is the slow half (each component resolved cold,
  // one at a time) and is only a baseline, so for large corpora it runs
  // on an evenly-spaced sample rather than the whole set. The batch pass
  // — the actual stress — always covers every kept component. `0`
  // disables the sequential baseline entirely.
  const seqSampleSize = parseSeqSample(argv) ?? Math.min(kept.length, 24);
  const seqPaths = sampleEvenly(allPaths, seqSampleSize);

  const openSession = async (): Promise<MetaSessionLike> => {
    const session = await openComponentMetaSession(
      { root: prepared.uiRoot, config: checkerConfig },
      CHECKER_OPTIONS,
    );
    for (const component of kept) {
      session.updateFile(component.absolutePath, component.transformedSource);
    }
    return session;
  };

  console.log(
    `\nmeta-ui CPU saturation — ${kept.length} components` +
      `${dropped > 0 ? ` (${dropped} excluded: ${excludes.join(", ")})` : ""}, ${cores} cores available\n`,
  );
  printHeader();

  // Sequential cold pass first (sampled): pays cold disk I/O so the batch
  // pass that follows is CPU-bound (its `cores used` reflects pure CPU
  // fan-out, not disk waits).
  let seq: PassMeasurement | null = null;
  if (seqPaths.length > 0) {
    const seqSession = await openSession();
    try {
      seq = await measure("sequential", seqPaths.length, async () => {
        let ok = 0;
        for (const path of seqPaths) {
          if (hasSurface(await seqSession.getComponentMeta(path))) ok += 1;
        }
        return ok;
      });
    } finally {
      seqSession.close();
    }
    printRow(seq);
  }

  // Batch cold pass on a fresh (dedicated-runtime) session: one native
  // call fans every kept component across the CPU pool. This is the
  // stress run.
  const batchSession = await openSession();
  let batch: PassMeasurement;
  try {
    batch = await measure("batch", allPaths.length, async () => {
      const metas = await batchSession.getComponentMetaBatch(allPaths);
      return metas.filter(hasSurface).length;
    });
  } finally {
    batchSession.close();
  }
  printRow(batch);

  shutdownMetaRuntime();

  console.log("");
  // Speedup is rate-based (components/sec) so it stays valid even when
  // the sequential baseline ran on a sample of a different size.
  if (seq && seq.componentsPerSec > 0) {
    const speedup = batch.componentsPerSec / seq.componentsPerSec;
    console.log(
      `  Batch used ${batch.coresUsed.toFixed(2)} of ${cores} cores ` +
        `(${((batch.coresUsed / cores) * 100).toFixed(0)}% of the machine), ` +
        `${speedup.toFixed(1)}x the per-component throughput of sequential.`,
    );
  } else {
    console.log(
      `  Batch used ${batch.coresUsed.toFixed(2)} of ${cores} cores ` +
        `(${((batch.coresUsed / cores) * 100).toFixed(0)}% of the machine).`,
    );
  }
  if (batch.coresUsed < 1.5) {
    console.log(
      "  ⚠ Batch stayed near a single core — the scheduler is not parallelising this corpus. " +
        "Check that the native binding exposes getComponentMetaBatch and that work is not serialising on shared state.",
    );
  } else {
    console.log("  ✓ The scheduler fanned the batch across multiple cores.");
  }
  console.log("");
}

if (process.argv[1] && resolve(process.argv[1]) === __filename) {
  main().catch((error) => {
    console.error(error instanceof Error ? (error.stack ?? error.message) : error);
    process.exitCode = 1;
  });
}
