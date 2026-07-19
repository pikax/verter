/**
 * Receipt emission, loading, and run-over-run comparison for the corpus gate.
 *
 * Receipts are machine-readable JSON written OUTSIDE the repo (an env-directed
 * path or a temp file) and identify the corpus only by its anonymous label.
 * The compare mode diffs two receipts route-by-route and kind-by-kind so a
 * Phase-2 block can state precisely what a change did to real-corpus latency,
 * wedge behaviour, and memory.
 */
import { mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { tmpdir } from "node:os";
import path from "node:path";

import { CORPUS_REQUEST_KINDS, type CorpusGateReceipt, type CorpusGateRoute } from "./types.js";

/** Resolve the receipt destination; creates parent directories as needed. */
export function corpusReceiptDestination(
  receipt: CorpusGateReceipt,
  envPath: string | null,
): string {
  if (envPath) {
    if (envPath.endsWith(".json")) {
      mkdirSync(path.dirname(path.resolve(envPath)), { recursive: true });
      return path.resolve(envPath);
    }
    const dir = path.resolve(envPath);
    mkdirSync(dir, { recursive: true });
    return path.join(dir, `corpus-gate-${Date.now()}.json`);
  }
  return path.join(tmpdir(), `verter-corpus-gate-receipt-${Date.now()}.json`);
}

/** Write the receipt and return its path (also logs the destination). */
export function writeCorpusReceipt(receipt: CorpusGateReceipt, envPath: string | null): string {
  const destination = corpusReceiptDestination(receipt, envPath);
  writeFileSync(destination, JSON.stringify(receipt, null, 2));
  console.log(`[corpus-gate] receipt (${receipt.corpusLabel}) → ${destination}`);
  return destination;
}

/** Load and structurally validate a prior receipt. */
export function loadCorpusReceipt(receiptPath: string): CorpusGateReceipt {
  const parsed = JSON.parse(readFileSync(receiptPath, "utf8")) as CorpusGateReceipt;
  if (parsed?.harness !== "corpus-gate" || parsed?.schemaVersion !== 1) {
    throw new Error(
      `not a corpus-gate schemaVersion-1 receipt: ${receiptPath} ` +
        `(harness=${JSON.stringify((parsed as { harness?: unknown })?.harness)})`,
    );
  }
  return parsed;
}

export interface CorpusCompareLine {
  readonly route: CorpusGateRoute;
  readonly metric: string;
  readonly baseline: number | boolean | null;
  readonly current: number | boolean | null;
  /** Numeric delta (current - baseline) when both sides are numbers. */
  readonly delta: number | null;
}

export interface CorpusCompareResult {
  readonly comparable: boolean;
  /** Why the comparison is weakened (different sample, label, config). */
  readonly caveats: readonly string[];
  readonly lines: readonly CorpusCompareLine[];
}

function numberDelta(baseline: number | null, current: number | null): number | null {
  return baseline !== null && current !== null ? current - baseline : null;
}

/** Diff two receipts route-by-route (pure). */
export function compareCorpusReceipts(
  baseline: CorpusGateReceipt,
  current: CorpusGateReceipt,
): CorpusCompareResult {
  const caveats: string[] = [];
  if (baseline.corpusLabel !== current.corpusLabel) {
    caveats.push(
      `corpus label differs (${baseline.corpusLabel} vs ${current.corpusLabel}) — cross-corpus deltas are not meaningful`,
    );
  }
  if (baseline.config.sampleSize !== current.config.sampleSize) {
    caveats.push(
      `sample size differs (${baseline.config.sampleSize} vs ${current.config.sampleSize})`,
    );
  }
  const lines: CorpusCompareLine[] = [];
  const routes = new Set<CorpusGateRoute>([
    ...(Object.keys(baseline.routes) as CorpusGateRoute[]),
    ...(Object.keys(current.routes) as CorpusGateRoute[]),
  ]);
  for (const route of routes) {
    const before = baseline.routes[route];
    const after = current.routes[route];
    if (!before || !after) {
      caveats.push(`route ${route} present in only one receipt`);
      continue;
    }
    if (before.sampleManifestHash !== after.sampleManifestHash) {
      caveats.push(`route ${route}: sample manifest differs — latency deltas are approximate`);
    }
    lines.push({
      route,
      metric: "wedged",
      baseline: before.wedged,
      current: after.wedged,
      delta: null,
    });
    lines.push({
      route,
      metric: "requestsSent",
      baseline: before.accounting.requestsSent,
      current: after.accounting.requestsSent,
      delta: numberDelta(before.accounting.requestsSent, after.accounting.requestsSent),
    });
    for (const kind of CORPUS_REQUEST_KINDS) {
      const beforeKind = before.kinds[kind];
      const afterKind = after.kinds[kind];
      for (const metric of ["p50Ms", "p90Ms", "p95Ms", "maxMs"] as const) {
        lines.push({
          route,
          metric: `${kind}.${metric}`,
          baseline: beforeKind[metric],
          current: afterKind[metric],
          delta: numberDelta(beforeKind[metric], afterKind[metric]),
        });
      }
      for (const metric of [
        "over2500Count",
        "timeoutCount",
        "emptyCount",
        "unexpectedEmptyCount",
      ] as const) {
        lines.push({
          route,
          metric: `${kind}.${metric}`,
          baseline: beforeKind[metric],
          current: afterKind[metric],
          delta: numberDelta(beforeKind[metric], afterKind[metric]),
        });
      }
    }
    const maxRss = (report: typeof before): number | null => {
      const values = report.memory
        .map((trend) => trend.maxRssBytes)
        .filter((value): value is number => value !== null);
      return values.length > 0 ? Math.max(...values) : null;
    };
    lines.push({
      route,
      metric: "maxRssBytes",
      baseline: maxRss(before),
      current: maxRss(after),
      delta: numberDelta(maxRss(before), maxRss(after)),
    });
  }
  return { comparable: caveats.length === 0, caveats, lines };
}

/** Render a compare result as human-readable text lines. */
export function formatCompare(result: CorpusCompareResult): string[] {
  const rendered: string[] = [];
  for (const caveat of result.caveats) rendered.push(`CAVEAT: ${caveat}`);
  for (const line of result.lines) {
    const delta =
      line.delta === null
        ? ""
        : ` (${line.delta > 0 ? "+" : ""}${Math.round(line.delta * 100) / 100})`;
    rendered.push(
      `${line.route} ${line.metric}: ${String(line.baseline)} -> ${String(line.current)}${delta}`,
    );
  }
  return rendered;
}
