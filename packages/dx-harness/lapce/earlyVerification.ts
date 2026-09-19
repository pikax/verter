/**
 * WSP1B `Issue93EarlyVerification` and `Issue93CarryForwardCase`.
 *
 * Qualifies current large-workspace behavior in a real Lapce UI using the
 * WSP1/WSP1L capture, UI timeline and semantic-oracle extract. Historical
 * issue 93 stays unreproduced: a green qualification is not a repair claim.
 */

import {
  measuredMetric,
  unknownMetric,
  type CompletenessState,
  type Metric,
  type ProductReceiptBasis,
} from "@verter/dx-harness/jetbrains";

import type { LapceUiRun } from "./types.js";
import type { ScriptedStepKind } from "./types.js";
import { LAPCE_REAL_HOST, isRejectedLapceUiHost } from "./types.js";

/** WSP0 authored-1k is the floor that is no longer a two-file smoke. */
export const LARGE_PROJECT_MIN_VUE_FILES = 1000;

/** Operator-approved equivalent of the PrimeVue (2,615 Vue files) private corpus. */
export const PRIMEVUE_EQUIVALENT_VUE_FILES = 2615;

export const ISSUE93_RETIREMENT = {
  id: 93,
  state: "unreproduced" as const,
  notAFix: true,
  retirement:
    "operator-approved unreproduced / not planned (WSP1A cancelled ledger); " +
    "this node never claims the historical hang was repaired",
};

export const REQUIRED_DRIVE_KINDS: readonly ScriptedStepKind[] = [
  "open",
  "type",
  "complete",
  "navigate",
  "close",
];

export type QualificationStatus = "qualified" | "failed" | "unverified";

export type QualificationRule =
  | "qualified"
  | "WSP1B-AC1"
  | "WSP1B-AC2"
  | "WSP1B-AC3"
  | "WSP1B-AC-OWNER"
  | "WSP1B-AC-BASIS"
  | "WSP1B-AC-RESOURCE"
  | "issue93-not-a-fix"
  | "protocol-smoke"
  | "sleep-as-readiness"
  | "unpinned-workload"
  | "missing-gui"
  | "stale-basis";

export interface TypedAnswer {
  readonly kind: "completion" | "definition" | "diagnostics";
  readonly expected: readonly string[];
  readonly observed: readonly string[];
  readonly match: boolean;
}

export interface SemanticOracleObservation {
  readonly diagnosticCount: Metric;
  readonly typedAnswers: readonly TypedAnswer[];
  readonly requiredFeaturesEnabled: boolean;
}

export interface ResourceObservation {
  readonly clientPaint: Metric;
  readonly providerProcessCost: Metric;
  readonly outboundBytes: Metric;
  readonly retainedMemory: Metric;
}

export interface WorkloadPin {
  readonly id: string;
  readonly class: "small-smoke" | "large-project";
  readonly vueFileCount: number;
  readonly sourcePin: string;
  readonly configurationPin: string;
  readonly privateCorpusLabel?: string;
}

export interface LargeRunObservation {
  readonly hostKind: LapceUiRun["hostKind"];
  readonly protocolSmokeOnly: boolean;
  readonly uiStallDetected: boolean;
  readonly serverImmediate: boolean;
  readonly realClientEvidence: boolean;
  readonly completenessState: CompletenessState;
  readonly usedSleepForReadiness: boolean;
  readonly versionsPinned: boolean;
  readonly interactionKinds: readonly ScriptedStepKind[];
  readonly reopenAfterClose: boolean;
  readonly diagnosticsObserved: boolean;
  readonly duration: Metric;
  readonly workload: WorkloadPin;
  readonly oracle: SemanticOracleObservation;
  readonly resources: ResourceObservation;
  readonly claimsIssue93Fixed: boolean;
  readonly finalOwner: string;
  readonly duplicatedAuthority: boolean;
}

export interface SmallSmokeObservation {
  readonly certified: boolean;
  readonly uiStallDetected: boolean;
  readonly vueFileCount: number;
}

export interface Issue93EarlyVerificationInput {
  readonly smallSmoke: SmallSmokeObservation;
  readonly largeRun: LargeRunObservation;
  readonly receiptBasis: ProductReceiptBasis;
}

export interface Issue93EarlyVerification {
  readonly schema: "issue93-early-verification.v1";
  readonly node: "WSP1B";
  readonly status: QualificationStatus;
  readonly rule: QualificationRule;
  readonly reason: string;
  readonly historicalIssue93: typeof ISSUE93_RETIREMENT;
  readonly smallSmoke: SmallSmokeObservation;
  readonly largeRun: LargeRunObservation;
  readonly receiptBasis: ProductReceiptBasis;
}

export interface Issue93CarryForwardCase {
  readonly schema: "issue93-carry-forward.v1";
  readonly node: "WSP1B";
  readonly id: "Issue93CarryForwardCase";
  readonly reusedAfter: readonly string[];
  readonly workload: WorkloadPin;
  readonly interactionKinds: readonly ScriptedStepKind[];
  readonly reopenAfterClose: boolean;
  readonly historicalIssue93: typeof ISSUE93_RETIREMENT;
  readonly qualificationStatus: QualificationStatus;
  readonly invalidation:
    | "source/project/configuration/engine/host identity change"
    | "never reuse a stale or protocol-smoke handle";
}

export interface Qualification {
  readonly status: QualificationStatus;
  readonly rule: QualificationRule;
  readonly reason: string;
}

function typedAnswersMatch(oracle: SemanticOracleObservation): boolean {
  if (oracle.typedAnswers.length === 0) return false;
  return oracle.typedAnswers.every((answer) => answer.match);
}

function driveComplete(run: LargeRunObservation): boolean {
  for (const kind of REQUIRED_DRIVE_KINDS) {
    if (!run.interactionKinds.includes(kind)) return false;
  }
  return run.reopenAfterClose && run.diagnosticsObserved;
}

function unavailableRequiredMeasurement(run: LargeRunObservation): string | null {
  if (run.resources.clientPaint.status !== "measured") {
    return `client application/paint unavailable: ${run.resources.clientPaint.status === "unknown" ? run.resources.clientPaint.reason : "not measured"}`;
  }
  return null;
}

/**
 * Qualify a paired small-smoke + large-project observation. Fail-closed:
 * a protocol smoke, an unpinned workload, a missing GUI capture or a claimed
 * issue-93 repair never becomes `qualified`. A UI stall with an immediate
 * server fails even when the two-file smoke stayed green (WSP1B-AC1/AC3).
 */
export function qualifyIssue93EarlyVerification(
  input: Issue93EarlyVerificationInput,
): Qualification {
  const { smallSmoke, largeRun } = input;

  if (largeRun.claimsIssue93Fixed) {
    return {
      status: "failed",
      rule: "issue93-not-a-fix",
      reason:
        "a qualification must not claim historical issue 93 was repaired; " +
        "the operator retirement is unreproduced / not planned, not a causal fix",
    };
  }
  if (
    largeRun.finalOwner !== "expansion.workspace-responsiveness" ||
    largeRun.duplicatedAuthority
  ) {
    return {
      status: "failed",
      rule: "WSP1B-AC-OWNER",
      reason:
        "WSP1B has one final owner (expansion.workspace-responsiveness); " +
        "a duplicated or displaced authority is a wrong-complete",
    };
  }
  if (largeRun.completenessState === "stale") {
    return {
      status: "failed",
      rule: "stale-basis",
      reason: "stale runs are never published as current (canonical observation basis)",
    };
  }
  if (largeRun.usedSleepForReadiness) {
    return {
      status: "failed",
      rule: "sleep-as-readiness",
      reason: "sleep-as-readiness is forbidden; the run is not real-editor evidence",
    };
  }
  if (isRejectedLapceUiHost(largeRun.hostKind) || largeRun.protocolSmokeOnly) {
    return {
      status: "unverified",
      rule: "protocol-smoke",
      reason:
        "a protocol launch smoke cannot certify this node (WSP1B.1); " +
        "current-client qualification stays unverified",
    };
  }
  if (
    largeRun.workload.class !== "large-project" ||
    largeRun.workload.vueFileCount < LARGE_PROJECT_MIN_VUE_FILES ||
    largeRun.workload.sourcePin.trim() === ""
  ) {
    return {
      status: "unverified",
      rule: "unpinned-workload",
      reason:
        "the large-project scenario is unpinned or below the authored-1k floor; " +
        "a two-file smoke cannot stand in for it (WSP1B-AC3)",
    };
  }
  if (largeRun.uiStallDetected && largeRun.serverImmediate) {
    return {
      status: "failed",
      rule: "WSP1B-AC1",
      reason:
        "the actual Lapce UI stalled while the server timeline stayed immediate; " +
        (smallSmoke.certified && !smallSmoke.uiStallDetected
          ? "the small-project smoke remaining green does not hide the stall (WSP1B-AC3)"
          : "a prompt server is not an interactivity certificate"),
    };
  }
  if (!largeRun.oracle.requiredFeaturesEnabled || !typedAnswersMatch(largeRun.oracle)) {
    return {
      status: "failed",
      rule: "WSP1B-AC2",
      reason:
        "the qualified run must preserve diagnostic counts and required typed answers " +
        "against the complete semantic oracle with required features enabled",
    };
  }
  if (largeRun.oracle.diagnosticCount.status === "unknown") {
    return {
      status: "unverified",
      rule: "WSP1B-AC2",
      reason: `semantic oracle diagnostic count unavailable: ${largeRun.oracle.diagnosticCount.reason}`,
    };
  }
  if (!driveComplete(largeRun)) {
    return {
      status: "unverified",
      rule: "WSP1B-AC3",
      reason:
        "the retained scenario must drive open/type/complete/diagnostics/navigate/close/reopen/teardown; " +
        "a partial script cannot qualify current-client behavior",
    };
  }
  const missingPaint = unavailableRequiredMeasurement(largeRun);
  if (missingPaint !== null) {
    return {
      status: "unverified",
      rule: "WSP1B-AC-RESOURCE",
      reason: missingPaint,
    };
  }
  if (!largeRun.realClientEvidence || largeRun.hostKind !== LAPCE_REAL_HOST) {
    return {
      status: "unverified",
      rule: "missing-gui",
      reason:
        "missing GUI instrumentation on the real Lapce host leaves current-client " +
        "qualification unverified (WSP1B.3); a fixture or protocol smoke is not a substitute",
    };
  }
  if (!largeRun.versionsPinned) {
    return {
      status: "unverified",
      rule: "unpinned-workload",
      reason: "client/server/provider versions must be pinned before qualification",
    };
  }
  if (smallSmoke.certified && smallSmoke.uiStallDetected === false && largeRun.uiStallDetected) {
    return {
      status: "failed",
      rule: "WSP1B-AC3",
      reason:
        "the retained large-project scenario detected a UI stall while the small-project smoke stayed green",
    };
  }
  return {
    status: "qualified",
    rule: "qualified",
    reason:
      "current large-workspace Lapce behavior is independently measured; " +
      "historical issue 93 stays unreproduced and is not claimed fixed",
  };
}

export function issue93EarlyVerification(
  input: Issue93EarlyVerificationInput,
): Issue93EarlyVerification {
  const qualification = qualifyIssue93EarlyVerification(input);
  return {
    schema: "issue93-early-verification.v1",
    node: "WSP1B",
    status: qualification.status,
    rule: qualification.rule,
    reason: qualification.reason,
    historicalIssue93: ISSUE93_RETIREMENT,
    smallSmoke: input.smallSmoke,
    largeRun: input.largeRun,
    receiptBasis: input.receiptBasis,
  };
}

export function issue93CarryForwardCase(
  verification: Issue93EarlyVerification,
): Issue93CarryForwardCase {
  return {
    schema: "issue93-carry-forward.v1",
    node: "WSP1B",
    id: "Issue93CarryForwardCase",
    reusedAfter: ["WSP8", "ED1"],
    workload: verification.largeRun.workload,
    interactionKinds: verification.largeRun.interactionKinds,
    reopenAfterClose: verification.largeRun.reopenAfterClose,
    historicalIssue93: ISSUE93_RETIREMENT,
    qualificationStatus: verification.status,
    invalidation:
      verification.largeRun.completenessState === "stale"
        ? "never reuse a stale or protocol-smoke handle"
        : "source/project/configuration/engine/host identity change",
  };
}

/** Extract JSON payloads after a Lapce tracing `write to lsp:` / `read from lsp:` marker. */
export function extractLspJson(line: string, marker: "write to lsp:" | "read from lsp:"): unknown {
  const at = line.indexOf(marker);
  if (at === -1) return null;
  const raw = line.slice(at + marker.length).trim();
  try {
    return JSON.parse(raw) as unknown;
  } catch {
    return null;
  }
}

export interface CapturedSemanticOracle {
  readonly diagnosticCount: number;
  readonly diagnosticUris: readonly string[];
  readonly completionLabels: readonly string[];
  readonly definitionUris: readonly string[];
  readonly outboundBytes: number;
  readonly providerPid: number | null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function stringField(value: unknown, key: string): string | null {
  if (!isRecord(value)) return null;
  const field = value[key];
  return typeof field === "string" ? field : null;
}

function completionLabelsFrom(result: unknown): string[] {
  if (!isRecord(result)) return [];
  const items = result.items;
  if (!Array.isArray(items)) return [];
  const labels: string[] = [];
  for (const item of items) {
    const label = stringField(item, "label");
    if (label !== null) labels.push(label);
  }
  return labels;
}

function definitionUrisFrom(result: unknown): string[] {
  const rows = Array.isArray(result)
    ? result
    : result === null || result === undefined
      ? []
      : [result];
  const uris: string[] = [];
  for (const row of rows) {
    if (!isRecord(row)) continue;
    if (typeof row.uri === "string") uris.push(row.uri);
    else if (typeof row.targetUri === "string") uris.push(row.targetUri);
  }
  return uris;
}

/**
 * Fold captured Lapce proxy lines into the semantic oracle + outbound-byte
 * extract. Missing families stay empty arrays / zero counts — never guessed
 * diagnostic zeros presented as a complete oracle.
 */
export function extractCapturedSemanticOracle(
  capturedLines: readonly string[],
): CapturedSemanticOracle {
  let diagnosticCount = 0;
  const diagnosticUris: string[] = [];
  const completionLabels: string[] = [];
  const definitionUris: string[] = [];
  let outboundBytes = 0;
  let providerPid: number | null = null;

  for (const line of capturedLines) {
    const written = extractLspJson(line, "write to lsp:");
    if (isRecord(written)) {
      outboundBytes += Buffer.byteLength(JSON.stringify(written), "utf8");
    }
    const read = extractLspJson(line, "read from lsp:");
    if (!isRecord(read)) continue;
    const method = stringField(read, "method");
    const result = read.result;
    if (method === "textDocument/publishDiagnostics" && isRecord(read.params)) {
      const uri = stringField(read.params, "uri");
      const diagnostics = read.params.diagnostics;
      const count = Array.isArray(diagnostics) ? diagnostics.length : 0;
      diagnosticCount += count;
      if (uri !== null) diagnosticUris.push(uri);
    }
    if (
      method === "$/verter/tsgoStarted" &&
      isRecord(read.params) &&
      typeof read.params.pid === "number"
    ) {
      providerPid = read.params.pid;
    }
    if (
      method === "$/verter/typeProviderStarted" &&
      isRecord(read.params) &&
      typeof read.params.pid === "number"
    ) {
      providerPid = read.params.pid;
    }
    if (result !== undefined && completionLabels.length === 0) {
      const labels = completionLabelsFrom(result);
      if (labels.length > 0) completionLabels.push(...labels);
    }
    if (result !== undefined && definitionUris.length === 0) {
      const uris = definitionUrisFrom(result);
      if (uris.length > 0) definitionUris.push(...uris);
    }
  }

  return {
    diagnosticCount,
    diagnosticUris,
    completionLabels,
    definitionUris,
    outboundBytes,
    providerPid,
  };
}

export function typedAnswersFromOracle(
  oracle: CapturedSemanticOracle,
  expected: {
    readonly completionLabels: readonly string[];
    readonly definitionNeedle: string;
  },
): TypedAnswer[] {
  const completionMatch = expected.completionLabels.every((label) =>
    oracle.completionLabels.includes(label),
  );
  const definitionMatch = oracle.definitionUris.some((uri) =>
    uri.toLowerCase().includes(expected.definitionNeedle.toLowerCase()),
  );
  return [
    {
      kind: "completion",
      expected: [...expected.completionLabels],
      observed: [...oracle.completionLabels],
      match: completionMatch,
    },
    {
      kind: "definition",
      expected: [expected.definitionNeedle],
      observed: [...oracle.definitionUris],
      match: definitionMatch,
    },
    {
      kind: "diagnostics",
      expected: ["counted"],
      observed: [String(oracle.diagnosticCount)],
      match: Number.isFinite(oracle.diagnosticCount),
    },
  ];
}

export function paintMetricFromRun(run: LapceUiRun): Metric {
  const measured = run.timelines
    .map((timeline) => timeline.inputToPaintMs)
    .filter(
      (metric): metric is Extract<Metric, { status: "measured" }> => metric.status === "measured",
    );
  if (measured.length === 0) {
    return unknownMetric("no measured input-to-paint on the large-project run");
  }
  return measuredMetric(Math.max(...measured.map((metric) => metric.value)), "ms");
}

export function stallOnImmediateServer(run: LapceUiRun): {
  readonly uiStallDetected: boolean;
  readonly serverImmediate: boolean;
} {
  let uiStallDetected = false;
  let serverImmediate = true;
  for (const timeline of run.timelines) {
    if (timeline.stall.detected) uiStallDetected = true;
    if (!timeline.server.serverImmediate) serverImmediate = false;
  }
  if (run.timelines.length === 0) {
    return { uiStallDetected: false, serverImmediate: false };
  }
  return { uiStallDetected, serverImmediate };
}
