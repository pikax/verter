/**
 * Paired-run recorder: randomised official/Verter order, cold/warm labels,
 * outlier retention, and swapped-order comparability within a recorded noise
 * bound (JBT1H-AC3). Failed runs are kept. Outliers are labelled, never dropped.
 */

import { classifyUiEvidence } from "./evidence.js";
import { evaluateMemoryRow, memoryMetricsFromCapture, type MemoryRow } from "./process-tree.js";
import {
  isSide,
  metricValue,
  type IdeCapture,
  type ProductReceiptBasis,
  type SessionState,
  type Side,
} from "./types.js";

export type PairOrder = readonly [Side, Side];

/** Deterministic pair order from a recorded seed (odd → Verter first). */
export function pairOrderFromSeed(seed: number): PairOrder {
  return (seed & 1) === 0 ? ["official", "verter"] : ["verter", "official"];
}

export function swappedOrder(order: PairOrder): PairOrder {
  return [order[1], order[0]];
}

export interface TimedSide {
  readonly side: Side;
  readonly sessionState: SessionState;
  readonly uiApplyPaintMs: number;
  readonly outlier: boolean;
}

export interface PairRecord {
  readonly order: PairOrder;
  readonly seed: number;
  readonly sessionState: SessionState;
  readonly sides: readonly TimedSide[];
  readonly captures: readonly IdeCapture[];
  readonly retainedFailed: readonly IdeCapture[];
  readonly memory: readonly MemoryRow[];
}

export interface NoiseBound {
  readonly maxAbsMs: number;
  readonly recordedAs: "campaign-predeclared";
}

export interface ComparabilityVerdict {
  readonly comparable: boolean;
  readonly reason: string;
  readonly deltasMs: readonly { readonly side: Side; readonly absMs: number }[];
}

export interface PairedCampaign {
  readonly basis: ProductReceiptBasis;
  readonly noiseBound: NoiseBound;
  readonly pairs: readonly PairRecord[];
}

export function recordPair(input: {
  readonly seed: number;
  readonly sessionState: SessionState;
  readonly captures: readonly IdeCapture[];
  readonly retainedFailed?: readonly IdeCapture[];
}): PairRecord {
  const order = pairOrderFromSeed(input.seed);
  const bySide = new Map<Side, IdeCapture>();
  for (const capture of input.captures) {
    const ui = classifyUiEvidence(capture);
    if (!ui.admissible) {
      throw new Error(`paired run rejected non-UI-admissible capture: ${ui.reason}`);
    }
    if (capture.sessionState !== input.sessionState) {
      throw new Error(
        `capture sessionState '${capture.sessionState}' does not match pair '${input.sessionState}'`,
      );
    }
    if (bySide.has(capture.side)) {
      throw new Error(`duplicate capture for side '${capture.side}' in one pair`);
    }
    bySide.set(capture.side, capture);
  }
  const basis = input.captures[0]?.receiptBasis;
  if (basis) {
    for (const capture of input.captures) {
      if (!sameReceiptBasis(basis, capture.receiptBasis)) {
        throw new Error(
          `pair mixes receipt bases (sourceRevisions '${basis.sourceRevisions}' vs '${capture.receiptBasis.sourceRevisions}')`,
        );
      }
    }
  }
  const sides: TimedSide[] = [];
  for (const side of order) {
    const capture = bySide.get(side);
    if (!capture) throw new Error(`pair missing ${side} capture (order ${order.join("→")})`);
    const paint = metricValue(capture.paint.uiApplyPaintMs);
    if (paint === null) {
      throw new Error(`${side} capture has no measured ui-apply-paint`);
    }
    sides.push({ side, sessionState: input.sessionState, uiApplyPaintMs: paint, outlier: false });
  }
  const memory = input.captures.map((capture) =>
    evaluateMemoryRow({
      tree: capture.processTree,
      ...memoryMetricsFromCapture(capture),
    }),
  );
  return {
    order,
    seed: input.seed,
    sessionState: input.sessionState,
    sides,
    captures: input.captures,
    retainedFailed: input.retainedFailed ?? [],
    memory,
  };
}

/**
 * Label outliers in-place on a copy: a side whose paint exceeds `bound` from the
 * pair-mean is flagged. The sample stays in the record (JBT1H: retain outliers).
 */
export function labelOutliers(pair: PairRecord, bound: NoiseBound): PairRecord {
  if (pair.sides.length === 0) return pair;
  const mean = pair.sides.reduce((sum, side) => sum + side.uiApplyPaintMs, 0) / pair.sides.length;
  return {
    ...pair,
    sides: pair.sides.map((side) => ({
      ...side,
      outlier: Math.abs(side.uiApplyPaintMs - mean) > bound.maxAbsMs,
    })),
  };
}

export function compareSwappedPairs(
  first: PairRecord,
  second: PairRecord,
  bound: NoiseBound,
): ComparabilityVerdict {
  if (first.order[0] === second.order[0]) {
    return {
      comparable: false,
      reason: `swapped-order proof requires opposite pair order, got ${first.order.join("→")} and ${second.order.join("→")}`,
      deltasMs: [],
    };
  }
  if (first.sessionState !== second.sessionState) {
    return {
      comparable: false,
      reason: `sessionState mismatch (${first.sessionState} vs ${second.sessionState})`,
      deltasMs: [],
    };
  }
  const bases = [...first.captures, ...second.captures].map((capture) => capture.receiptBasis);
  const basis = bases[0];
  if (basis) {
    for (const other of bases) {
      if (!sameReceiptBasis(basis, other)) {
        return {
          comparable: false,
          reason:
            "receipt basis mismatch (sourceRevisions/projectConfiguration/engineIdentity/hostIdentity differ; old-source outcomes are not comparable)",
          deltasMs: [],
        };
      }
    }
  }
  const deltas: { side: Side; absMs: number }[] = [];
  for (const sideName of ["official", "verter"] as const) {
    const a = first.sides.find((row) => row.side === sideName);
    const b = second.sides.find((row) => row.side === sideName);
    if (!a || !b) {
      return {
        comparable: false,
        reason: `missing ${sideName} timing in swapped pair`,
        deltasMs: deltas,
      };
    }
    deltas.push({ side: sideName, absMs: Math.abs(a.uiApplyPaintMs - b.uiApplyPaintMs) });
  }
  const over = deltas.filter((delta) => delta.absMs > bound.maxAbsMs);
  if (over.length > 0) {
    return {
      comparable: false,
      reason:
        `swapped-order timings exceed recorded noise bound ${bound.maxAbsMs}ms: ` +
        over.map((delta) => `${delta.side} Δ=${delta.absMs}ms`).join(", "),
      deltasMs: deltas,
    };
  }
  return {
    comparable: true,
    reason: `both sides within ${bound.maxAbsMs}ms across swapped order`,
    deltasMs: deltas,
  };
}

export function sameReceiptBasis(a: ProductReceiptBasis, b: ProductReceiptBasis): boolean {
  return (
    a.sourceRevisions === b.sourceRevisions &&
    a.projectConfiguration === b.projectConfiguration &&
    a.engineIdentity === b.engineIdentity &&
    a.hostIdentity === b.hostIdentity
  );
}

export function parseSide(value: unknown): Side {
  if (!isSide(value)) throw new Error(`invalid comparison side: ${String(value)}`);
  return value;
}
