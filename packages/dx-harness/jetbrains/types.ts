/**
 * JBT1H vocabulary: real-IDE comparison receipts, completeness, metrics and
 * process-tree rows. Reuses DX0 ProductReceiptBasis field names and completeness
 * states; unavailable numbers are `{ status: "unknown" }`, never guessed zeros.
 */

export const COMPLETENESS_STATES = [
  "complete",
  "complete-empty",
  "partial",
  "pending",
  "unsupported",
  "ambiguous",
  "failed",
  "cancelled",
  "stale",
] as const;
export type CompletenessState = (typeof COMPLETENESS_STATES)[number];
export function isCompletenessState(value: unknown): value is CompletenessState {
  return typeof value === "string" && (COMPLETENESS_STATES as readonly string[]).includes(value);
}

export const SIDES = ["official", "verter"] as const;
export type Side = (typeof SIDES)[number];
export function isSide(value: unknown): value is Side {
  return value === "official" || value === "verter";
}

export const SESSION_STATES = ["cold", "warm"] as const;
export type SessionState = (typeof SESSION_STATES)[number];
export function isSessionState(value: unknown): value is SessionState {
  return value === "cold" || value === "warm";
}

export const BASIS_KINDS = ["fresh", "incremental"] as const;
export type BasisKind = (typeof BASIS_KINDS)[number];
export function isBasisKind(value: unknown): value is BasisKind {
  return value === "fresh" || value === "incremental";
}

/** Hosts that can never certify real JetBrains UI application/paint (JBT1H-AC1). */
export const REJECTED_UI_HOSTS = ["mock-lsp", "raw-lsp", "screenshot", "protocol-smoke"] as const;
export type RejectedUiHost = (typeof REJECTED_UI_HOSTS)[number];
export const REAL_IDE_HOST = "real-jetbrains-ide" as const;
export type RealIdeHostKind = typeof REAL_IDE_HOST;
export type CaptureHostKind = RealIdeHostKind | RejectedUiHost;

export function isRejectedUiHost(value: unknown): value is RejectedUiHost {
  return typeof value === "string" && (REJECTED_UI_HOSTS as readonly string[]).includes(value);
}

export type Metric =
  | { readonly status: "measured"; readonly value: number; readonly unit: string }
  | { readonly status: "unknown"; readonly reason: string };

export function measuredMetric(value: number, unit: string): Metric {
  if (!Number.isFinite(value)) {
    return { status: "unknown", reason: `non-finite measurement ${String(value)}` };
  }
  return { status: "measured", value, unit };
}

export function unknownMetric(reason: string): Metric {
  return { status: "unknown", reason };
}

export function metricValue(metric: Metric): number | null {
  return metric.status === "measured" ? metric.value : null;
}

/** DX0 ProductReceiptBasis v1 fields bound on every comparison run. */
export interface ProductReceiptBasis {
  readonly sourceRevisions: string;
  readonly projectConfiguration: string;
  readonly engineIdentity: string;
  readonly hostIdentity: string;
  readonly completenessState: CompletenessState;
}

export const REQUIRED_METRIC_IDS = [
  "ui-apply-paint",
  "rpc-duration",
  "provider-cpu",
  "provider-wall",
  "outbound-bytes",
  "retained-memory",
] as const;
export type RequiredMetricId = (typeof REQUIRED_METRIC_IDS)[number];

export const COMPARISON_WORKFLOWS = [
  "completion",
  "diagnostics",
  "source-navigation",
  "find-usages",
  "generics",
  "public-types",
  "component-extraction",
  "rename-move-import-updates",
  "formatting",
  "inlays",
  "styles",
  "run-debug-workflows",
] as const;
export type ComparisonWorkflowId = (typeof COMPARISON_WORKFLOWS)[number];

export interface VersionSet {
  readonly ideProduct: string;
  readonly ideBuild: string;
  readonly pluginVersion: string | null;
  readonly engine: Metric;
}

export interface ProcessTreeMember {
  readonly pid: number;
  readonly parentPid: number | null;
  readonly image: string;
  readonly role: "ide" | "provider" | "descendant";
  readonly rssBytes: Metric;
}

export type TypeProviderStatus = "observed" | "missing" | "excluded" | "unknown";

export interface ProcessTreeSnapshot {
  readonly rootPid: number;
  readonly members: readonly ProcessTreeMember[];
  readonly typeProviderStatus: TypeProviderStatus;
  readonly typeProviderPids: readonly number[];
  readonly typeProviderReason: string;
}

export interface CorrectnessRow {
  readonly workflow: ComparisonWorkflowId;
  readonly side: Side;
  readonly completeness: CompletenessState;
  readonly reason: string;
}

export interface PaintSample {
  readonly uiApplyPaintMs: Metric;
  readonly rpcDurationMs: Metric;
}

export interface IdeCapture {
  readonly hostKind: CaptureHostKind;
  readonly usedSleepForReadiness: boolean;
  readonly versions: VersionSet;
  readonly paint: PaintSample;
  readonly processTree: ProcessTreeSnapshot;
  readonly workflows: readonly CorrectnessRow[];
  readonly receiptBasis: ProductReceiptBasis;
  readonly side: Side;
  readonly sessionState: SessionState;
  readonly basisKind: BasisKind;
  /** Present when the capture measured the aggregate; otherwise derived or unknown. */
  readonly retainedMemory?: Metric;
  readonly providerCpu?: Metric;
  readonly providerWall?: Metric;
  readonly outboundBytes?: Metric;
}
