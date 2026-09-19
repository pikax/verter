/**
 * WSP1L vocabulary: the Lapce interaction driver and its UI timeline. Reuses the
 * JBT1H metric/completeness/receipt vocabulary and the WSP1 server-trace types;
 * unavailable numbers stay `{ status: "unknown" }`, never guessed zeros.
 */

import type { ProtocolStage } from "@verter/lsp-test-client";

export {
  COMPLETENESS_STATES,
  isCompletenessState,
  measuredMetric,
  metricValue,
  unknownMetric,
  type CompletenessState,
  type Metric,
  type ProductReceiptBasis,
} from "@verter/dx-harness/jetbrains";

import type { CompletenessState, Metric, ProductReceiptBasis } from "@verter/dx-harness/jetbrains";

/** The only host kind that can certify a real-Lapce UI timeline (WSP1L-AC3). */
export const LAPCE_REAL_HOST = "real-lapce" as const;
/**
 * Hosts that can never certify Lapce UI input-to-paint evidence: a protocol
 * smoke, mock LSP client, screenshot or raw-LSP session is not the real editor
 * event loop (WSP1L-AC3 / product-experience contract).
 */
export const REJECTED_LAPCE_UI_HOSTS = [
  "protocol-smoke",
  "mock-lsp",
  "screenshot",
  "raw-lsp",
] as const;
export type RejectedLapceUiHost = (typeof REJECTED_LAPCE_UI_HOSTS)[number];
/** The hermetic deterministic host used to prove the driver's discriminations. */
export const LAPCE_FIXTURE_HOST = "instrumented-fixture" as const;
export type LapceHostKind =
  | typeof LAPCE_REAL_HOST
  | typeof LAPCE_FIXTURE_HOST
  | RejectedLapceUiHost;

export function isRejectedLapceUiHost(value: unknown): value is RejectedLapceUiHost {
  return (
    typeof value === "string" && (REJECTED_LAPCE_UI_HOSTS as readonly string[]).includes(value)
  );
}

/** Client-side UI stages, ordered input → paint. */
export const UI_STAGE_ORDER = ["input_dispatched", "decoded", "applied", "painted"] as const;
export type UiStage = (typeof UI_STAGE_ORDER)[number];

export interface UiStageStamp {
  readonly stage: UiStage;
  readonly atMs: number;
}

/** The scripted interactions `LapceInteractionDriver` drives (charter WSP1L). */
export const SCRIPTED_STEP_KINDS = ["open", "type", "complete", "navigate", "close"] as const;
export type ScriptedStepKind = (typeof SCRIPTED_STEP_KINDS)[number];

export interface ScriptedStep {
  readonly kind: ScriptedStepKind;
  readonly label?: string;
  /** Correlates this UI interaction with the WSP1 server InteractionTrace. */
  readonly requestEpoch: number;
  readonly sourceEpoch: number | null;
}

/** Raw per-interaction UI instrumentation captured from the host. */
export interface UiInteractionRecord {
  readonly step: ScriptedStep;
  readonly stamps: readonly UiStageStamp[];
}

/** How the server side of the same epoch behaved (from a WSP1 InteractionTrace). */
export interface ServerCorrelation {
  readonly requestEpoch: number;
  readonly sourceEpoch: number | null;
  readonly method: string;
  readonly serverCompleteMs: number | null;
  readonly serverTotalMs: Metric;
  readonly serverFirstBlockedStage: ProtocolStage | null;
  /** True only when the server trace shows no blocked stage and an immediate total. */
  readonly serverImmediate: boolean;
  readonly reason: string;
}

export interface UiStallEvidence {
  readonly detected: boolean;
  readonly thresholdMs: number;
  readonly maxGapMs: Metric;
  /** The stage before the worst event-loop gap when a stall is detected. */
  readonly stalledAfter: UiStage | null;
}

export interface UiTimeline {
  readonly schema: "ui-timeline.v1";
  readonly step: ScriptedStep;
  readonly inputToPaintMs: Metric;
  readonly decodeMs: Metric;
  readonly applyMs: Metric;
  readonly paintMs: Metric;
  readonly eventLoopStallMs: Metric;
  readonly stall: UiStallEvidence;
  readonly server: ServerCorrelation;
}

/** WSP1L.1 record: which automation/instrumentation path was used, and its perturbation. */
export interface AutomationPath {
  readonly kind: string;
  readonly perturbation: string;
  readonly recordedAs: string;
}

/** Pinned Lapce/volt/server/provider identity (charter deliverable). */
export const VERSION_MANIFEST_SCHEMA = "lapce-version-manifest.v1" as const;

export interface VersionManifestItem {
  readonly item: string;
  readonly version: string | null;
  readonly status: "pinned" | "unrecorded";
  readonly reason?: string;
}

export interface LapceVersionManifest {
  readonly schema: typeof VERSION_MANIFEST_SCHEMA;
  readonly items: readonly VersionManifestItem[];
  readonly recordedAs: string;
}

/** One scripted run against one host, with its UI timelines. */
export interface LapceUiRun {
  readonly schema: "lapce-ui-run.v1";
  readonly hostKind: LapceHostKind;
  readonly automationPath: AutomationPath;
  readonly usedSleepForReadiness: boolean;
  readonly versions: LapceVersionManifest;
  /** True when the LSP protocol exchange completed (a smoke is not a UI timeline). */
  readonly protocolSmokePassed: boolean;
  readonly timelines: readonly UiTimeline[];
  readonly receiptBasis: ProductReceiptBasis;
  readonly completenessState: CompletenessState;
}

export type Certification =
  | { readonly certified: true; readonly run: LapceUiRun }
  | { readonly certified: false; readonly rule: string; readonly reason: string };

export interface RealClientClaimVerdict {
  readonly admissible: boolean;
  readonly hostKind: string;
  readonly reason: string;
}
