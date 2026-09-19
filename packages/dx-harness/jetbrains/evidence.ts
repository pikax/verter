/**
 * JBT1H-AC1: a mock LSP client (or raw-LSP / screenshot / protocol smoke) cannot
 * count as real JetBrains UI evidence. RPC duration is not application/paint.
 */

import { REAL_IDE_HOST, isRejectedUiHost, type IdeCapture, type Metric } from "./types.js";

export type UiEvidenceVerdict =
  | { readonly admissible: true; readonly hostKind: typeof REAL_IDE_HOST }
  | { readonly admissible: false; readonly hostKind: string; readonly reason: string };

export function classifyUiEvidence(capture: IdeCapture): UiEvidenceVerdict {
  if (capture.usedSleepForReadiness) {
    return {
      admissible: false,
      hostKind: capture.hostKind,
      reason:
        "sleep-as-readiness is forbidden; the capture is not real-editor responsiveness evidence",
    };
  }
  if (isRejectedUiHost(capture.hostKind) || capture.hostKind !== REAL_IDE_HOST) {
    return {
      admissible: false,
      hostKind: capture.hostKind,
      reason:
        `hostKind '${capture.hostKind}' cannot certify JetBrains UI application/paint ` +
        `(JBT1H-AC1: mock LSP / raw-LSP / screenshot / protocol smoke are not real-IDE evidence)`,
    };
  }
  if (capture.paint.uiApplyPaintMs.status !== "measured") {
    return {
      admissible: false,
      hostKind: capture.hostKind,
      reason:
        `real-client UI claim requires a measured ui-apply-paint metric; ` +
        `got ${formatMetric(capture.paint.uiApplyPaintMs)} (RPC-only duration is not paint)`,
    };
  }
  return { admissible: true, hostKind: REAL_IDE_HOST };
}

export function assertRealIdeUiEvidence(capture: IdeCapture): void {
  const verdict = classifyUiEvidence(capture);
  if (!verdict.admissible) {
    throw new Error(`JBT1H-AC1: ${verdict.reason}`);
  }
}

function formatMetric(metric: Metric): string {
  return metric.status === "measured"
    ? `measured ${metric.value} ${metric.unit}`
    : `unknown (${metric.reason})`;
}
