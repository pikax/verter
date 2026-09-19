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

import type { CaptureProvenance, LapceUiRun } from "./types.js";
import type { ScriptedStepKind } from "./types.js";
import { LAPCE_REAL_HOST, isRejectedLapceUiHost } from "./types.js";
import { uiStampDigest, type UiStampPayload } from "./stamp.js";

/** WSP0 authored-1k is the floor that is no longer a two-file smoke. */
export const LARGE_PROJECT_MIN_VUE_FILES = 1000;

/** Operator-approved equivalent of the PrimeVue (2,615 Vue files) private corpus. */
export const PRIMEVUE_EQUIVALENT_VUE_FILES = 2615;

/** Repeated type/complete cycles required before a run counts as sustained churn. */
export const MIN_SUSTAINED_TYPE_STEPS = 3;
export const MIN_SUSTAINED_COMPLETE_STEPS = 3;

export const ISSUE93_RETIREMENT = {
  id: 93,
  state: "unreproduced" as const,
  notAFix: true,
  retirement:
    "operator-approved unreproduced / not planned (WSP1A cancelled ledger); " +
    "this node never claims the historical hang was repaired",
};

export const OPERATOR_MANUAL_PROBE = {
  basis: "main 4bbe1ba3dc933552493d6cf063c61a048f04cea8",
  lspSha256: "66289bf28b13679f8c50d8d63308b5a154668ff55028603cb85dc3af52c14dc9",
  lapce: "0.4.6",
  typescript: "7.0.2",
  corpus:
    "PrimeVue (2,615 Vue files) opened during setup; unlabeled private corpus, source not published",
  result: "seems to be working correctly, no hang found",
  limitations: [
    "exact tested project, interaction sequence and duration were not specified",
    "no WSP1/WSP1L UI timeline or semantic oracle was captured",
    "this probe cannot certify WSP1B; the qualifying run is the synthetic-15k slice capture",
  ],
} as const;

export const WSP1L_SMALL_SMOKE_SURFACE =
  "tests/workspace-responsiveness/WSP1L/products/real-client-capture.v1.json";

export const REQUIRED_DRIVE_KINDS: readonly ScriptedStepKind[] = [
  "open",
  "type",
  "complete",
  "navigate",
  "close",
];

/**
 * Retained WSP8/ED1 replay script. Same-document close/reopen is required;
 * Lapce 0.4.6+WSP1L does not emit a paint ack for a second open of the same
 * path, so a recapture that cannot observe that paint stays unverified.
 */
export const ISSUE93_RETAINED_SCRIPT = [
  { kind: "open" as const, requestEpoch: 1, role: "helper" },
  { kind: "open" as const, requestEpoch: 2, role: "fixture", line: 4, column: 20 },
  { kind: "navigate" as const, requestEpoch: 3, role: "definition" },
  { kind: "type" as const, requestEpoch: 4, role: "churn-1", text: " " },
  { kind: "complete" as const, requestEpoch: 5, role: "churn-1" },
  { kind: "type" as const, requestEpoch: 6, role: "churn-2", text: " " },
  { kind: "complete" as const, requestEpoch: 7, role: "churn-2" },
  { kind: "type" as const, requestEpoch: 8, role: "churn-3", text: " " },
  { kind: "complete" as const, requestEpoch: 9, role: "churn-3" },
  { kind: "close" as const, requestEpoch: 10, role: "fixture" },
  { kind: "open" as const, requestEpoch: 11, role: "reopen-fixture", line: 4, column: 20 },
] as const;

/** Seeded synthetic-15k oracle (generatorVersion=1.1.0, seed=0x5eed15). */
export const ISSUE93_ORACLE_EXPECTATION = {
  completionLabels: ["greeting", "user", "message"] as const,
  definition: {
    uriSuffix: "/Fixture.vue",
    range: { start: { line: 1, character: 6 }, end: { line: 1, character: 14 } },
  },
  diagnosticsByUriSuffix: {
    "/Fixture.vue": 0,
    "/Helper.vue": 0,
    "/Comp0000_000.vue": 7,
  } as const,
};

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

export interface LspRange {
  readonly start: { readonly line: number; readonly character: number };
  readonly end: { readonly line: number; readonly character: number };
}

export interface DefinitionLocation {
  readonly uri: string;
  readonly range: LspRange | null;
}

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
  readonly serverBuildPin: string;
  readonly providerEnginePin: string;
  readonly interactionKinds: readonly ScriptedStepKind[];
  readonly reopenAfterClose: boolean;
  readonly sameDocumentReopen: boolean;
  readonly diagnosticsObserved: boolean;
  readonly duration: Metric;
  readonly workload: WorkloadPin;
  readonly oracle: SemanticOracleObservation;
  readonly resources: ResourceObservation;
  readonly claimsIssue93Fixed: boolean;
  readonly finalOwner: string;
  readonly duplicatedAuthority: boolean;
  readonly captureProvenance?: CaptureProvenance;
  readonly observedUiStamps?: readonly UiStampPayload[];
}

export interface SmallSmokeObservation {
  readonly certified: boolean;
  readonly uiStallDetected: boolean;
  readonly vueFileCount: number;
  readonly surface?: string;
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
  readonly operatorManualProbe: typeof OPERATOR_MANUAL_PROBE;
  readonly smallSmoke: SmallSmokeObservation;
  readonly largeRun: LargeRunObservation;
  readonly receiptBasis: ProductReceiptBasis;
}

export interface CarryForwardStep {
  readonly kind: ScriptedStepKind;
  readonly requestEpoch: number;
  readonly role?: string;
  readonly text?: string;
  readonly line?: number;
  readonly column?: number;
}

export interface Issue93CarryForwardCase {
  readonly schema: "issue93-carry-forward.v1";
  readonly node: "WSP1B";
  readonly id: "Issue93CarryForwardCase";
  readonly reusedAfter: readonly string[];
  readonly workload: WorkloadPin;
  readonly interactionKinds: readonly ScriptedStepKind[];
  readonly reopenAfterClose: boolean;
  readonly script: readonly CarryForwardStep[];
  readonly diagnostics: string;
  readonly teardown: string;
  readonly reopenNote: string;
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

export function hasSustainedChurn(kinds: readonly ScriptedStepKind[]): boolean {
  const types = kinds.filter((kind) => kind === "type").length;
  const completes = kinds.filter((kind) => kind === "complete").length;
  return types >= MIN_SUSTAINED_TYPE_STEPS && completes >= MIN_SUSTAINED_COMPLETE_STEPS;
}

export function isSha256BuildPin(pin: string): boolean {
  return /^sha256:[0-9a-f]{64}$/i.test(pin.trim());
}

export function isProviderEnginePin(pin: string): boolean {
  return /\d+\.\d+/.test(pin.trim());
}

const REQUIRED_ORACLE_FAMILIES = ["completion", "definition", "diagnostics"] as const;

function typedAnswersMatch(oracle: SemanticOracleObservation): boolean {
  for (const kind of REQUIRED_ORACLE_FAMILIES) {
    const rows = oracle.typedAnswers.filter((answer) => answer.kind === kind);
    if (rows.length === 0) return false;
    if (!rows.every((answer) => answer.match)) return false;
  }
  return true;
}

export function observationIdentity(run: LargeRunObservation): {
  readonly sourceRevisions: string;
  readonly projectConfiguration: string;
  readonly engineIdentity: string;
  readonly hostIdentity: string;
} {
  return {
    sourceRevisions: run.workload.sourcePin,
    projectConfiguration: run.workload.configurationPin,
    engineIdentity: `verter-lsp ${run.serverBuildPin} / ${run.providerEnginePin}`,
    hostIdentity:
      run.captureProvenance !== undefined
        ? `real-lapce:${run.captureProvenance.lapceClientVersion}`
        : run.hostKind,
  };
}

export function receiptBindsObservation(
  receipt: ProductReceiptBasis,
  run: LargeRunObservation,
): boolean {
  const expected = observationIdentity(run);
  return (
    receipt.sourceRevisions === expected.sourceRevisions &&
    receipt.projectConfiguration === expected.projectConfiguration &&
    receipt.engineIdentity === expected.engineIdentity &&
    receipt.hostIdentity === expected.hostIdentity
  );
}

/** Join the observed provider kind to an authoritative engine version (never a forged notification field). */
export function pinProviderEngine(
  oracle: Pick<CapturedSemanticOracle, "providerKind">,
  authoritativeVersion: string,
): string {
  const kind = oracle.providerKind?.trim() ?? "";
  const version = authoritativeVersion.trim();
  if (kind === "" || !isProviderEnginePin(version)) return "";
  return `${kind} ${version}`;
}

function driveComplete(run: LargeRunObservation): boolean {
  for (const kind of REQUIRED_DRIVE_KINDS) {
    if (!run.interactionKinds.includes(kind)) return false;
  }
  return (
    run.reopenAfterClose &&
    run.sameDocumentReopen &&
    run.diagnosticsObserved &&
    hasSustainedChurn(run.interactionKinds)
  );
}

function unavailableRequiredMeasurement(run: LargeRunObservation): string | null {
  const required: readonly (readonly [string, Metric])[] = [
    ["client application/paint", run.resources.clientPaint],
    ["provider process cost", run.resources.providerProcessCost],
    ["outbound bytes", run.resources.outboundBytes],
    ["retained memory", run.resources.retainedMemory],
  ];
  for (const [name, metric] of required) {
    if (metric.status !== "measured") {
      return `${name} unavailable: ${metric.status === "unknown" ? metric.reason : "not measured"}`;
    }
  }
  return null;
}

function completenessVerdict(state: CompletenessState, which: string): Qualification | null {
  if (state === "complete") return null;
  if (state === "stale") {
    return {
      status: "failed",
      rule: "stale-basis",
      reason: `${which} is stale; stale runs are never published as current (canonical observation basis)`,
    };
  }
  if (state === "cancelled" || state === "failed") {
    return {
      status: "failed",
      rule: "WSP1B-AC-BASIS",
      reason: `${which} completeness is ${state}; cancelled and failed receipts cannot qualify`,
    };
  }
  return {
    status: "unverified",
    rule: "WSP1B-AC-BASIS",
    reason: `${which} completeness is ${state}; only a complete current basis can qualify`,
  };
}

function provenanceBinds(run: LargeRunObservation): boolean {
  const provenance = run.captureProvenance;
  const stamps = run.observedUiStamps;
  if (provenance === undefined || provenance.schema !== "driven-lapce-capture.v1") return false;
  if (stamps === undefined) return false;
  return uiStampDigest(stamps) === provenance.captureSha256;
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
  if (largeRun.usedSleepForReadiness) {
    return {
      status: "failed",
      rule: "sleep-as-readiness",
      reason: "sleep-as-readiness is forbidden; the run is not real-editor evidence",
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
  if (largeRun.uiStallDetected) {
    return {
      status: "failed",
      rule: "WSP1B-AC3",
      reason:
        "the retained large-project scenario detected a UI stall; " +
        "a missing or stalled small-project smoke does not hide it",
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
  const runCompleteness = completenessVerdict(largeRun.completenessState, "largeRun");
  if (runCompleteness !== null) return runCompleteness;
  const receiptCompleteness = completenessVerdict(
    input.receiptBasis.completenessState,
    "receiptBasis",
  );
  if (receiptCompleteness !== null) return receiptCompleteness;
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
        "the retained scenario must drive open/type/complete/diagnostics/navigate/close/reopen/teardown " +
        "with sustained churn and a same-document close/reopen; a partial script cannot qualify",
    };
  }
  const missingResource = unavailableRequiredMeasurement(largeRun);
  if (missingResource !== null) {
    return {
      status: "unverified",
      rule: "WSP1B-AC-RESOURCE",
      reason: missingResource,
    };
  }
  if (
    !largeRun.realClientEvidence ||
    largeRun.hostKind !== LAPCE_REAL_HOST ||
    !provenanceBinds(largeRun)
  ) {
    return {
      status: "unverified",
      rule: "missing-gui",
      reason:
        "missing GUI instrumentation on the real Lapce host leaves current-client " +
        "qualification unverified (WSP1B.3); a fixture or protocol smoke is not a substitute",
    };
  }
  if (
    !largeRun.versionsPinned ||
    !isSha256BuildPin(largeRun.serverBuildPin) ||
    !isProviderEnginePin(largeRun.providerEnginePin)
  ) {
    return {
      status: "unverified",
      rule: "unpinned-workload",
      reason:
        "client/server/provider versions must be pinned to immutable build identities before qualification",
    };
  }
  if (largeRun.duration.status !== "measured") {
    return {
      status: "unverified",
      rule: "unpinned-workload",
      reason: `duration unavailable: ${
        largeRun.duration.status === "unknown" ? largeRun.duration.reason : "not measured"
      }`,
    };
  }
  if (!receiptBindsObservation(input.receiptBasis, largeRun)) {
    return {
      status: "failed",
      rule: "stale-basis",
      reason:
        "receipt source/project/engine/host identity does not bind this observation; " +
        "another basis cannot certify the current run",
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
    operatorManualProbe: OPERATOR_MANUAL_PROBE,
    smallSmoke: {
      ...input.smallSmoke,
      surface: input.smallSmoke.surface ?? WSP1L_SMALL_SMOKE_SURFACE,
    },
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
    interactionKinds: ISSUE93_RETAINED_SCRIPT.map((step) => step.kind),
    reopenAfterClose: true,
    script: ISSUE93_RETAINED_SCRIPT.map((step) => ({ ...step })),
    diagnostics:
      "final textDocument/publishDiagnostics per opened URI against ISSUE93_ORACLE_EXPECTATION",
    teardown: "driveLapceSession kills the client tree after the last ack",
    reopenNote:
      "same-document close/reopen of Fixture.vue is required; Lapce 0.4.6+WSP1L does not emit a paint ack for a second open of the same path, so missing paint leaves qualification unverified (WSP1B.3)",
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

export interface CapturedDiagnosticSet {
  readonly uri: string;
  readonly version: number | null;
  readonly count: number;
  readonly messages: readonly string[];
}

export interface CapturedSemanticOracle {
  readonly diagnosticCount: number;
  readonly diagnosticUris: readonly string[];
  readonly diagnosticsByUri: readonly CapturedDiagnosticSet[];
  readonly completionLabels: readonly string[];
  readonly completionPending: boolean;
  readonly definitionLocations: readonly DefinitionLocation[];
  readonly definitionPending: boolean;
  readonly definitionUris: readonly string[];
  readonly outboundBytes: number;
  readonly providerPid: number | null;
  readonly providerKind: string | null;
  readonly providerVersion: string | null;
}

function isRecord(value: unknown): value is Record<string, unknown> {
  return value !== null && typeof value === "object" && !Array.isArray(value);
}

function stringField(value: unknown, key: string): string | null {
  if (!isRecord(value)) return null;
  const field = value[key];
  return typeof field === "string" ? field : null;
}

function idField(value: unknown): string | number | null {
  if (!isRecord(value)) return null;
  const id = value.id;
  return typeof id === "number" || typeof id === "string" ? id : null;
}

function numberField(value: unknown, key: string): number | null {
  if (!isRecord(value)) return null;
  const field = value[key];
  return typeof field === "number" && Number.isFinite(field) ? field : null;
}

function textDocumentUri(params: unknown): string | null {
  if (!isRecord(params)) return null;
  const doc = params.textDocument;
  return stringField(doc, "uri") ?? stringField(params, "uri");
}

function textDocumentVersion(params: unknown): number | null {
  if (!isRecord(params)) return null;
  return numberField(params.textDocument, "version") ?? numberField(params, "version");
}

interface TrackedDocument {
  version: number | null;
  open: boolean;
}

interface MethodOracle<T> {
  seq: number;
  pending: boolean;
  value: T;
}

function asRange(value: unknown): LspRange | null {
  if (!isRecord(value)) return null;
  const start = value.start;
  const end = value.end;
  if (!isRecord(start) || !isRecord(end)) return null;
  if (
    typeof start.line !== "number" ||
    typeof start.character !== "number" ||
    typeof end.line !== "number" ||
    typeof end.character !== "number"
  ) {
    return null;
  }
  return {
    start: { line: start.line, character: start.character },
    end: { line: end.line, character: end.character },
  };
}

function rangesEqual(left: LspRange, right: LspRange): boolean {
  return (
    left.start.line === right.start.line &&
    left.start.character === right.start.character &&
    left.end.line === right.end.line &&
    left.end.character === right.end.character
  );
}

export function decodeUriPath(uri: string): string {
  try {
    return decodeURIComponent(new URL(uri).pathname).replaceAll("\\", "/");
  } catch {
    return uri.replaceAll("\\", "/");
  }
}

export function uriPathEndsWith(uri: string, suffix: string): boolean {
  const path = decodeUriPath(uri).toLowerCase();
  const want = suffix.replaceAll("\\", "/").toLowerCase();
  const needle = want.startsWith("/") ? want : `/${want}`;
  return path === needle.slice(1) || path.endsWith(needle);
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

function definitionLocationsFrom(result: unknown): DefinitionLocation[] {
  const rows = Array.isArray(result)
    ? result
    : result === null || result === undefined
      ? []
      : [result];
  const locations: DefinitionLocation[] = [];
  for (const row of rows) {
    if (!isRecord(row)) continue;
    const uri =
      typeof row.uri === "string"
        ? row.uri
        : typeof row.targetUri === "string"
          ? row.targetUri
          : null;
    if (uri === null) continue;
    const range =
      asRange(row.targetSelectionRange) ?? asRange(row.targetRange) ?? asRange(row.range);
    locations.push({ uri, range });
  }
  return locations;
}

/**
 * Fold captured Lapce proxy lines into the semantic oracle + outbound-byte
 * extract. Missing families stay empty arrays / zero counts — never guessed
 * diagnostic zeros presented as a complete oracle.
 *
 * Outbound bytes are the UTF-8 JSON bodies of `read from lsp:` records
 * (server → client). Client ingress (`write to lsp:`) is not outbound.
 *
 * Completion and definition answers are the freshest *request* for that method
 * (joined by JSON-RPC id). A later request that is still pending, or that
 * returned empty/error, is authoritative even if an older success arrives last.
 * Diagnostics bind to the current open document lifetime and version.
 */
export function extractCapturedSemanticOracle(
  capturedLines: readonly string[],
): CapturedSemanticOracle {
  const diagnosticsByUri = new Map<string, CapturedDiagnosticSet>();
  const documents = new Map<string, TrackedDocument>();
  let outboundBytes = 0;
  let providerPid: number | null = null;
  let providerKind: string | null = null;
  let nextSeq = 0;
  const pendingById = new Map<string | number, { readonly method: string; readonly seq: number }>();
  const latestSeq = { completion: 0, definition: 0 };
  let completion: MethodOracle<string[]> = { seq: 0, pending: false, value: [] };
  let definition: MethodOracle<DefinitionLocation[]> = { seq: 0, pending: false, value: [] };

  const trackRequest = (method: string, id: string | number): void => {
    nextSeq += 1;
    pendingById.set(id, { method, seq: nextSeq });
    if (method === "textDocument/completion") {
      latestSeq.completion = nextSeq;
      completion = { seq: nextSeq, pending: true, value: [] };
    }
    if (method === "textDocument/definition") {
      latestSeq.definition = nextSeq;
      definition = { seq: nextSeq, pending: true, value: [] };
    }
  };

  const applyDocumentLifecycle = (written: Record<string, unknown>): void => {
    const method = stringField(written, "method");
    if (method === null) return;
    const params = written.params;
    const uri = textDocumentUri(params);
    if (uri === null) return;
    if (method === "textDocument/didOpen") {
      documents.set(uri, { version: textDocumentVersion(params), open: true });
      diagnosticsByUri.delete(uri);
      return;
    }
    if (method === "textDocument/didChange") {
      const previous = documents.get(uri);
      documents.set(uri, {
        version: textDocumentVersion(params) ?? previous?.version ?? null,
        open: previous?.open ?? true,
      });
      return;
    }
    if (method === "textDocument/didClose") {
      documents.set(uri, { version: null, open: false });
      diagnosticsByUri.delete(uri);
    }
  };

  for (const line of capturedLines) {
    const written = extractLspJson(line, "write to lsp:");
    if (isRecord(written)) {
      applyDocumentLifecycle(written);
      const id = idField(written);
      const method = stringField(written, "method");
      if (id !== null && method !== null) {
        trackRequest(method, id);
      }
    }
    const read = extractLspJson(line, "read from lsp:");
    if (!isRecord(read)) continue;
    outboundBytes += Buffer.byteLength(JSON.stringify(read), "utf8");
    const method = stringField(read, "method");
    const result = read.result;
    if (method === "textDocument/publishDiagnostics" && isRecord(read.params)) {
      const uri = stringField(read.params, "uri");
      const diagnostics = read.params.diagnostics;
      const count = Array.isArray(diagnostics) ? diagnostics.length : 0;
      const messages = Array.isArray(diagnostics)
        ? diagnostics
            .map((row) => stringField(row, "message"))
            .filter((message): message is string => message !== null)
        : [];
      const version = numberField(read.params, "version");
      if (uri !== null) {
        const doc = documents.get(uri);
        if (doc !== undefined) {
          if (!doc.open) continue;
          if (version !== null && doc.version !== null && version !== doc.version) continue;
        } else {
          const previous = diagnosticsByUri.get(uri);
          if (
            previous !== undefined &&
            version !== null &&
            previous.version !== null &&
            version < previous.version
          ) {
            continue;
          }
        }
        diagnosticsByUri.set(uri, { uri, version, count, messages });
      }
    }
    if (method === "$/verter/tsgoStarted" && isRecord(read.params)) {
      if (typeof read.params.pid === "number") providerPid = read.params.pid;
    }
    if (method === "$/verter/typeProviderStarted" && isRecord(read.params)) {
      if (typeof read.params.pid === "number") providerPid = read.params.pid;
      const kind = stringField(read.params, "kind");
      if (kind !== null) providerKind = kind;
    }
    const id = idField(read);
    if (id !== null && pendingById.has(id)) {
      const pending = pendingById.get(id)!;
      pendingById.delete(id);
      const failed = read.error !== undefined;
      if (pending.method === "textDocument/completion" && pending.seq === latestSeq.completion) {
        completion = {
          seq: pending.seq,
          pending: false,
          value: failed ? [] : completionLabelsFrom(result),
        };
      }
      if (pending.method === "textDocument/definition" && pending.seq === latestSeq.definition) {
        definition = {
          seq: pending.seq,
          pending: false,
          value: failed ? [] : definitionLocationsFrom(result),
        };
      }
    }
  }

  const diagnosticSets = [...diagnosticsByUri.values()];
  return {
    diagnosticCount: diagnosticSets.reduce((sum, set) => sum + set.count, 0),
    diagnosticUris: diagnosticSets.map((set) => set.uri),
    diagnosticsByUri: diagnosticSets,
    completionLabels: completion.value,
    completionPending: completion.pending,
    definitionLocations: definition.value,
    definitionPending: definition.pending,
    definitionUris: definition.value.map((location) => location.uri),
    outboundBytes,
    providerPid,
    providerKind,
    providerVersion: null,
  };
}

export interface SemanticOracleExpectation {
  readonly completionLabels: readonly string[];
  readonly definition: {
    readonly uriSuffix: string;
    readonly range?: LspRange;
  };
  readonly diagnosticsByUriSuffix: Readonly<Record<string, number>>;
}

export function typedAnswersFromOracle(
  oracle: CapturedSemanticOracle,
  expected: SemanticOracleExpectation = ISSUE93_ORACLE_EXPECTATION,
): TypedAnswer[] {
  const completionMatch =
    !oracle.completionPending &&
    expected.completionLabels.every((label) => oracle.completionLabels.includes(label));
  const definitionMatch =
    !oracle.definitionPending &&
    oracle.definitionLocations.some((location) => {
      if (!uriPathEndsWith(location.uri, expected.definition.uriSuffix)) return false;
      if (expected.definition.range === undefined) return true;
      return location.range !== null && rangesEqual(location.range, expected.definition.range);
    });
  const expectedDiagnosticEntries = Object.entries(expected.diagnosticsByUriSuffix);
  const observedDiagnosticCounts = expectedDiagnosticEntries.map(([suffix, count]) => {
    const set = oracle.diagnosticsByUri.find((row) => uriPathEndsWith(row.uri, suffix));
    return `${suffix}:${set === undefined ? "missing" : set.count}(expected ${count})`;
  });
  const diagnosticMatch =
    expectedDiagnosticEntries.every(([suffix, count]) => {
      const set = oracle.diagnosticsByUri.find((row) => uriPathEndsWith(row.uri, suffix));
      return set !== undefined && set.count === count;
    }) &&
    oracle.diagnosticsByUri.every((set) => {
      const expectedCount = expectedDiagnosticEntries.find(([suffix]) =>
        uriPathEndsWith(set.uri, suffix),
      );
      return expectedCount !== undefined && set.count === expectedCount[1];
    });
  return [
    {
      kind: "completion",
      expected: [...expected.completionLabels],
      observed: [...oracle.completionLabels],
      match: completionMatch,
    },
    {
      kind: "definition",
      expected: [
        expected.definition.uriSuffix,
        ...(expected.definition.range === undefined
          ? []
          : [
              `${expected.definition.range.start.line}:${expected.definition.range.start.character}-${expected.definition.range.end.line}:${expected.definition.range.end.character}`,
            ]),
      ],
      observed: oracle.definitionLocations.map((location) =>
        location.range === null
          ? location.uri
          : `${location.uri}#${location.range.start.line}:${location.range.start.character}`,
      ),
      match: definitionMatch,
    },
    {
      kind: "diagnostics",
      expected: expectedDiagnosticEntries.map(([suffix, count]) => `${suffix}:${count}`),
      observed: observedDiagnosticCounts,
      match: diagnosticMatch,
    },
  ];
}

export function paintMetricFromRun(run: {
  readonly timelines: readonly {
    readonly step?: { readonly kind: string };
    readonly inputToPaintMs: Metric;
  }[];
}): Metric {
  if (run.timelines.length === 0) {
    return unknownMetric("no measured input-to-paint on the large-project run");
  }
  const values: number[] = [];
  for (const timeline of run.timelines) {
    if (timeline.inputToPaintMs.status !== "measured") {
      const step = timeline.step?.kind ?? "interaction";
      return unknownMetric(
        `input-to-paint unavailable on ${step}: ${
          timeline.inputToPaintMs.status === "unknown"
            ? timeline.inputToPaintMs.reason
            : "not measured"
        }`,
      );
    }
    values.push(timeline.inputToPaintMs.value);
  }
  return measuredMetric(Math.max(...values), "ms");
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
