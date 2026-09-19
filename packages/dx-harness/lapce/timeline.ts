/**
 * WSP1L.2: correlate Lapce UI timestamps with the WSP1 server/protocol trace so
 * the first blocked stage is identifiable (WSP1L-AC1), and compare two scripted
 * runs within a recorded noise bound (WSP1L-AC2).
 */

import {
  firstBlockedServerStage,
  type InteractionTrace,
  type ProtocolStage,
  SERVER_STAGE_ORDER,
} from "@verter/lsp-test-client";
import { measuredMetric, unknownMetric, type Metric } from "@verter/dx-harness/jetbrains";
import {
  UI_STAGE_ORDER,
  type ScriptedStep,
  type ServerCorrelation,
  type UiInteractionRecord,
  type UiStage,
  type UiTimeline,
} from "./types.js";

/** Recorded noise bound for two-run comparability (WSP1L-AC2), never guessed. */
export interface NoiseBound {
  readonly maxAbsMs: number;
  readonly recordedAs: string;
}

/** Above this event-loop gap a UI interaction counts as stalled (WSP1L-AC1). */
export interface StallThreshold {
  readonly stallThresholdMs: number;
  readonly recordedAs: string;
}

/** A server trace counts as immediate only within this recorded bound. */
export interface ServerImmediacyBound {
  readonly maxServerTotalMs: number;
  readonly recordedAs: string;
}

export type FirstBlockedStageKind =
  | { readonly side: "ui"; readonly stage: UiStage }
  | { readonly side: "server"; readonly stage: ProtocolStage };

export interface BlockedStageVerdict {
  readonly uiStallDetected: boolean;
  readonly serverImmediate: boolean;
  readonly firstBlockedStage: FirstBlockedStageKind | null;
  readonly reason: string;
}

function latestUiStamps(record: UiInteractionRecord): Map<UiStage, number> {
  const latest = new Map<UiStage, number>();
  for (const stamp of record.stamps) {
    const seen = latest.get(stamp.stage);
    if (seen === undefined || stamp.atMs >= seen) latest.set(stamp.stage, stamp.atMs);
  }
  return latest;
}

function gapMetric(from: number, to: number): Metric {
  return measuredMetric(to - from, "ms");
}

function stageGap(latest: Map<UiStage, number>, from: UiStage, to: UiStage): Metric | null {
  const a = latest.get(from);
  const b = latest.get(to);
  if (a === undefined || b === undefined) return null;
  return gapMetric(a, b);
}

function worstEventLoopGap(latest: Map<UiStage, number>): {
  readonly gapMs: Metric;
  readonly stalledAfter: UiStage | null;
} {
  let worst: { gap: number; stalledAfter: UiStage } | null = null;
  for (let i = 0; i < UI_STAGE_ORDER.length - 1; i += 1) {
    const gap = stageGap(latest, UI_STAGE_ORDER[i], UI_STAGE_ORDER[i + 1]);
    if (gap === null || gap.status !== "measured") continue;
    if (worst === null || gap.value > worst.gap) {
      worst = { gap: gap.value, stalledAfter: UI_STAGE_ORDER[i] };
    }
  }
  if (worst === null) {
    return {
      gapMs: unknownMetric("fewer than two UI stages recorded for this interaction"),
      stalledAfter: null,
    };
  }
  return { gapMs: measuredMetric(worst.gap, "ms"), stalledAfter: worst.stalledAfter };
}

export function correlateServerTrace(
  step: ScriptedStep,
  trace: InteractionTrace,
  bound: ServerImmediacyBound,
): ServerCorrelation {
  const first = trace.stamps.find((stamp) => stamp.stage === "request_received")?.atMs ?? null;
  const complete = trace.stamps.find((stamp) => stamp.stage === "complete")?.atMs ?? null;
  const total: Metric =
    first !== null && complete !== null
      ? measuredMetric(complete - first, "ms")
      : unknownMetric("server trace has no request_received→complete pair for this epoch");
  const blocked = trace.firstBlockedStage ?? firstBlockedServerStage(trace.stamps);
  let immediate = false;
  let reason: string;
  if (blocked !== null) {
    reason = `server trace shows a blocked server stage '${blocked}' for epoch ${step.requestEpoch}`;
  } else if (total.status === "measured") {
    immediate = total.value <= bound.maxServerTotalMs;
    reason = immediate
      ? `server completed in ${total.value} ms (<= ${bound.maxServerTotalMs} ms recorded bound) with no blocked stage`
      : `server total ${total.value} ms exceeds the recorded immediacy bound ${bound.maxServerTotalMs} ms`;
  } else {
    reason = `server immediacy undecided: ${total.reason}`;
  }
  return {
    requestEpoch: step.requestEpoch,
    sourceEpoch: step.sourceEpoch,
    method: trace.method,
    serverCompleteMs: complete,
    serverTotalMs: total,
    serverFirstBlockedStage: blocked,
    serverImmediate: immediate,
    reason,
  };
}

export function buildUiTimeline(
  record: UiInteractionRecord,
  trace: InteractionTrace,
  options: {
    readonly stallThreshold: StallThreshold;
    readonly immediacyBound: ServerImmediacyBound;
  },
): UiTimeline {
  const latest = latestUiStamps(record);
  const input = latest.get("input_dispatched");
  const decoded = latest.get("decoded");
  const applied = latest.get("applied");
  const painted = latest.get("painted");
  const worst = worstEventLoopGap(latest);
  return {
    schema: "ui-timeline.v1",
    step: record.step,
    inputToPaintMs:
      input !== undefined && painted !== undefined
        ? gapMetric(input, painted)
        : unknownMetric(
            input === undefined && painted === undefined
              ? "neither input_dispatched nor painted recorded"
              : input === undefined
                ? "input_dispatched not recorded"
                : "painted not recorded",
          ),
    decodeMs: stageGap(latest, "input_dispatched", "decoded") ?? unknown("decode"),
    applyMs: stageGap(latest, "decoded", "applied") ?? unknown("apply"),
    paintMs: stageGap(latest, "applied", "painted") ?? unknown("paint"),
    eventLoopStallMs: worst.gapMs,
    stall: {
      detected:
        worst.gapMs.status === "measured" &&
        worst.gapMs.value >= options.stallThreshold.stallThresholdMs,
      thresholdMs: options.stallThreshold.stallThresholdMs,
      maxGapMs: worst.gapMs,
      stalledAfter: worst.stalledAfter,
    },
    server: correlateServerTrace(record.step, trace, options.immediacyBound),
  };
}

function unknown(stage: string): Metric {
  return unknownMetric(`${stage} stage pair not recorded; not guessed`);
}

/**
 * WSP1L-AC1: a deliberately stalled UI thread is detected while the server
 * timeline shows immediate completion. The verdict names the FIRST blocked
 * stage and keeps a server-side stall distinct from a UI-side stall.
 */
export function detectUiStallWithImmediateServer(timeline: UiTimeline): BlockedStageVerdict {
  const { stall, server } = timeline;
  if (stall.detected && server.serverImmediate) {
    const stalledAfter = stall.stalledAfter ?? "input_dispatched";
    return {
      uiStallDetected: true,
      serverImmediate: true,
      firstBlockedStage: { side: "ui", stage: stalledAfter },
      reason:
        `UI event loop stalled ${formatMetric(stall.maxGapMs)} after '${stalledAfter}' ` +
        `(>= ${stall.thresholdMs} ms threshold) while the server completed immediately: ${server.reason}`,
    };
  }
  if (!server.serverImmediate && server.serverFirstBlockedStage !== null) {
    return {
      uiStallDetected: false,
      serverImmediate: false,
      firstBlockedStage: { side: "server", stage: server.serverFirstBlockedStage },
      reason:
        `the first blocked stage is server-side ('${server.serverFirstBlockedStage}'); ` +
        `this is not a UI stall: ${server.reason}`,
    };
  }
  if (!server.serverImmediate) {
    return {
      uiStallDetected: false,
      serverImmediate: false,
      firstBlockedStage: null,
      reason: `server immediacy undecided, so a UI-stall attribution is not certified: ${server.reason}`,
    };
  }
  return {
    uiStallDetected: false,
    serverImmediate: true,
    firstBlockedStage: null,
    reason: `no stage blocked: UI max event-loop gap ${formatMetric(stall.maxGapMs)} stays under the ${stall.thresholdMs} ms threshold and the server completed immediately`,
  };
}

export interface TimelineComparison {
  readonly comparable: boolean;
  readonly reason: string;
  readonly deltasMs: readonly { readonly metric: string; readonly absMs: number }[];
  readonly unknownMetrics: readonly string[];
}

/** WSP1L-AC2: two runs of the same scripted interaction within the noise bound. */
export function compareScriptedTimelines(
  a: UiTimeline,
  b: UiTimeline,
  noiseBound: NoiseBound,
): TimelineComparison {
  if (a.step.requestEpoch !== b.step.requestEpoch || a.step.kind !== b.step.kind) {
    return {
      comparable: false,
      reason: `runs are not the same scripted interaction (epoch ${a.step.requestEpoch}/${b.step.requestEpoch}, kind ${a.step.kind}/${b.step.kind})`,
      deltasMs: [],
      unknownMetrics: [],
    };
  }
  const metrics: readonly [string, Metric, Metric][] = [
    ["input-to-paint", a.inputToPaintMs, b.inputToPaintMs],
    ["decode", a.decodeMs, b.decodeMs],
    ["apply", a.applyMs, b.applyMs],
    ["paint", a.paintMs, b.paintMs],
    ["event-loop-stall", a.eventLoopStallMs, b.eventLoopStallMs],
  ];
  const deltasMs: { metric: string; absMs: number }[] = [];
  const unknownMetrics: string[] = [];
  for (const [name, left, right] of metrics) {
    if (left.status !== "measured" || right.status !== "measured") {
      unknownMetrics.push(name);
      continue;
    }
    deltasMs.push({ metric: name, absMs: Math.abs(left.value - right.value) });
  }
  const headlineUnknown = unknownMetrics.includes("input-to-paint");
  const worst = deltasMs.reduce<(typeof deltasMs)[number] | null>(
    (acc, row) => (acc === null || row.absMs > acc.absMs ? row : acc),
    null,
  );
  if (headlineUnknown) {
    return {
      comparable: false,
      reason:
        `input-to-paint is unknown in at least one run (${unknownMetrics.join(", ")}); ` +
        `comparability is not certified on unknowns, and zeros are never guessed`,
      deltasMs,
      unknownMetrics,
    };
  }
  if (worst !== null && worst.absMs > noiseBound.maxAbsMs) {
    return {
      comparable: false,
      reason:
        `'${worst.metric}' differs by ${worst.absMs} ms, exceeding the recorded noise bound ` +
        `${noiseBound.maxAbsMs} ms (${noiseBound.recordedAs})`,
      deltasMs,
      unknownMetrics,
    };
  }
  return {
    comparable: true,
    reason:
      worst === null
        ? "all stage metrics measured and equal"
        : `worst delta '${worst.metric}' ${worst.absMs} ms stays within the recorded noise bound ${noiseBound.maxAbsMs} ms (${noiseBound.recordedAs})`,
    deltasMs,
    unknownMetrics,
  };
}

function formatMetric(metric: Metric): string {
  return metric.status === "measured"
    ? `${metric.value} ${metric.unit}`
    : `unknown (${metric.reason})`;
}

export { SERVER_STAGE_ORDER };
