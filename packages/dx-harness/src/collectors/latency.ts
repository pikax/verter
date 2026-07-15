/**
 * Latency collector (raw-LSP).
 *
 * Measures request round-trip time per method/scenario/driver/provider and reports
 * p50/p95/p99. Raw-LSP timing uses a MONOTONIC clock (`performance.now()`), never wall
 * clock, so a clock adjustment mid-run cannot corrupt a measurement. The percentile
 * math is R-7 linear interpolation (the numpy / spreadsheet default) over a sorted
 * copy of the samples — pure and unit-tested without a server; the live driver only
 * supplies the timed samples.
 */

import type { EditStep, LatencyThresholds, ProbeMethod } from "../scenario/index.js";
import {
  offsetToPosition,
  openDocument,
  sendTickChange,
  type CollectorLspClient,
} from "./client.js";
import { EditBuffer, runEditScript } from "./editLoop.js";
import {
  collectorEvent,
  type CollectorEvent,
  type CollectorEventKey,
  type EventSink,
  type SignalProvenance,
} from "./event.js";

const RAW_LSP: SignalProvenance = { detectedBy: "rawLsp" };

/** A monotonic timestamp in milliseconds (NOT wall clock) — the latency measurement clock. */
export function monotonicNow(): number {
  return performance.now();
}

/**
 * The p-th percentile of an ASCENDING-sorted sample via R-7 linear interpolation
 * (rank `= p/100 · (n−1)`, interpolating between the bracketing order statistics).
 * Total over an empty sample (`0`) and a single sample (that value). `p` is clamped to
 * `[0, 100]`.
 */
export function percentile(sortedAscending: readonly number[], p: number): number {
  const n = sortedAscending.length;
  if (n === 0) return 0;
  if (n === 1) return sortedAscending[0];
  const clamped = p < 0 ? 0 : p > 100 ? 100 : p;
  const rank = (clamped / 100) * (n - 1);
  const lower = Math.floor(rank);
  const upper = Math.ceil(rank);
  if (lower === upper) return sortedAscending[lower];
  const frac = rank - lower;
  return sortedAscending[lower] + frac * (sortedAscending[upper] - sortedAscending[lower]);
}

/** A latency summary over a set of round-trip samples (milliseconds). */
export interface LatencySummary {
  readonly count: number;
  readonly min: number;
  readonly max: number;
  readonly mean: number;
  readonly p50: number;
  readonly p95: number;
  readonly p99: number;
}

/** Summarize round-trip samples into count/min/max/mean and p50/p95/p99. Total over empty. */
export function summarizeLatency(samples: readonly number[]): LatencySummary {
  if (samples.length === 0) {
    return { count: 0, min: 0, max: 0, mean: 0, p50: 0, p95: 0, p99: 0 };
  }
  const sorted = [...samples].sort((a, b) => a - b);
  const sum = sorted.reduce((acc, value) => acc + value, 0);
  return {
    count: sorted.length,
    min: sorted[0],
    max: sorted[sorted.length - 1],
    mean: sum / sorted.length,
    p50: percentile(sorted, 50),
    p95: percentile(sorted, 95),
    p99: percentile(sorted, 99),
  };
}

/** The pure inputs to one latency classification. */
export interface LatencyInput {
  readonly key: CollectorEventKey;
  readonly method: ProbeMethod;
  /** The measured round-trip samples (milliseconds). */
  readonly samples: readonly number[];
  /** Per-percentile ceilings; a breach of any is flagged. Omitted = report-only. */
  readonly thresholds?: LatencyThresholds;
}

/** Classify a latency sample set: a breach of any supplied percentile ceiling is user-visible. */
export function classifyLatency(input: LatencyInput): CollectorEvent {
  const { key, method, samples, thresholds } = input;
  const summary = summarizeLatency(samples);
  const breaches: string[] = [];
  if (thresholds?.p50Ms !== undefined && summary.p50 > thresholds.p50Ms) breaches.push("p50");
  if (thresholds?.p95Ms !== undefined && summary.p95 > thresholds.p95Ms) breaches.push("p95");
  if (thresholds?.p99Ms !== undefined && summary.p99 > thresholds.p99Ms) breaches.push("p99");

  if (breaches.length > 0) {
    return collectorEvent({
      collector: "latency",
      signal: "latency_breach",
      ok: false,
      severity: "userVisible",
      provenance: RAW_LSP,
      key,
      detail: `${method} latency breached ${breaches.join(", ")}`,
      data: { method, ...summary, breaches, thresholds },
    });
  }
  return collectorEvent({
    collector: "latency",
    signal: "latency_summary",
    ok: true,
    severity: "userVisible",
    provenance: RAW_LSP,
    key,
    detail: `${method} latency p50=${summary.p50.toFixed(1)} p95=${summary.p95.toFixed(1)} p99=${summary.p99.toFixed(1)} (n=${summary.count})`,
    data: { method, ...summary },
  });
}

// ── live raw-LSP driver ──────────────────────────────────────────────────────

/** Options for the live {@link collectLatency} run. */
export interface CollectLatencyOptions {
  readonly client: CollectorLspClient;
  readonly sink: EventSink;
  readonly uri: string;
  readonly languageId?: string;
  readonly buffer: EditBuffer;
  /** Edits applied before timing begins. */
  readonly script?: readonly EditStep[];
  readonly scenario: string;
  readonly probe: string;
  readonly anchor: string;
  readonly provider: string;
  /** The request to time. */
  readonly method: ProbeMethod;
  /** The LSP request method string (e.g. `textDocument/hover`). */
  readonly lspMethod: string;
  /** How many timed requests to issue. */
  readonly iterations: number;
  readonly thresholds?: LatencyThresholds;
  readonly requestTimeoutMs?: number;
}

/**
 * Drive verter through the (optional) edit script, then issue `iterations` timed
 * requests at the settled anchor, measuring each round-trip with the monotonic clock,
 * and classify the percentile summary.
 */
export async function collectLatency(options: CollectLatencyOptions): Promise<void> {
  const { client, sink, uri, buffer } = options;
  const script = options.script ?? [];
  openDocument(client, uri, options.languageId ?? "vue", buffer.text, buffer.version);
  await runEditScript(buffer, script, (tick) => {
    sendTickChange(client, uri, tick);
  });

  const anchorOffset = buffer.anchorOffset(options.anchor);
  const position = offsetToPosition(buffer.text, anchorOffset, client.positionEncoding);
  const params = { textDocument: { uri }, position };
  const samples: number[] = [];
  for (let i = 0; i < options.iterations; i++) {
    const started = monotonicNow();
    await client.sendRequest(options.lspMethod, params, options.requestTimeoutMs);
    samples.push(monotonicNow() - started);
  }

  const key: CollectorEventKey = {
    scenario: options.scenario,
    editStepIndex: script.length - 1,
    driver: "rawLsp",
    provider: options.provider,
    probe: options.probe,
    version: buffer.version,
    anchor: options.anchor,
  };
  sink.emit(
    classifyLatency({
      key,
      method: options.method,
      samples,
      ...(options.thresholds !== undefined ? { thresholds: options.thresholds } : {}),
    }),
  );
}
