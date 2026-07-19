/**
 * Pure latency/outcome summarisation for the corpus gate.
 *
 * Percentiles use nearest-rank on the sorted sample (p50/p90/p95 plus max),
 * matching how the throwaway probes reported. Timeouts contribute their bound
 * (the recorded elapsed ms) to the latency distribution — a timed-out request
 * IS a slow request, so excluding it would flatter the percentiles.
 */
import {
  CORPUS_REQUEST_KINDS,
  type CorpusKindSummary,
  type CorpusRequestKind,
  type CorpusRequestObservation,
} from "./types.js";

/** The interactive threshold the receipts count violations of (ms). */
export const INTERACTIVE_THRESHOLD_MS = 2_500;

/** Nearest-rank percentile over an UNSORTED sample; 0 for an empty sample. */
export function percentile(samples: readonly number[], q: number): number {
  if (samples.length === 0) return 0;
  if (q <= 0) return Math.min(...samples);
  const sorted = [...samples].sort((left, right) => left - right);
  if (q >= 100) return sorted[sorted.length - 1];
  const rank = Math.ceil((q / 100) * sorted.length);
  return sorted[Math.min(sorted.length, Math.max(1, rank)) - 1];
}

/** Summarise one request kind's observations (pure). */
export function summarizeKind(
  observations: readonly CorpusRequestObservation[],
  kind: CorpusRequestKind,
): CorpusKindSummary {
  const relevant = observations.filter((observation) => observation.kind === kind);
  const latencies = relevant.map((observation) => observation.ms);
  return {
    count: relevant.length,
    p50Ms: percentile(latencies, 50),
    p90Ms: percentile(latencies, 90),
    p95Ms: percentile(latencies, 95),
    maxMs: latencies.length === 0 ? 0 : Math.max(...latencies),
    over2500Count: relevant.filter((observation) => observation.ms > INTERACTIVE_THRESHOLD_MS)
      .length,
    timeoutCount: relevant.filter((observation) => observation.verdict === "timeout").length,
    emptyCount: relevant.filter((observation) => observation.verdict === "empty").length,
    unexpectedEmptyCount: relevant.filter((observation) => observation.unexpectedEmpty).length,
    errorCount: relevant.filter((observation) => observation.verdict === "error").length,
  };
}

/** Summarise all four request kinds (pure). */
export function summarizeKinds(
  observations: readonly CorpusRequestObservation[],
): Readonly<Record<CorpusRequestKind, CorpusKindSummary>> {
  const entries = CORPUS_REQUEST_KINDS.map(
    (kind) => [kind, summarizeKind(observations, kind)] as const,
  );
  return Object.fromEntries(entries) as Record<CorpusRequestKind, CorpusKindSummary>;
}

/**
 * Downsample a series to at most `maxPoints` entries, always keeping the first
 * and last points so trend endpoints survive.
 */
export function downsampleSeries<T>(series: readonly T[], maxPoints: number): T[] {
  if (series.length <= maxPoints || maxPoints < 2) return [...series];
  const result: T[] = [series[0]];
  const step = (series.length - 1) / (maxPoints - 1);
  for (let i = 1; i < maxPoints - 1; i += 1) {
    result.push(series[Math.round(i * step)]);
  }
  result.push(series[series.length - 1]);
  return result;
}
