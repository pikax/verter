/**
 * The finding model + reducer: the report layer's interpretation of a collection run.
 *
 * It FOLDS two landed streams into a deduplicated finding set — the
 * {@link CollectorEvent} stream (verter-side signals, with each differential
 * comparison's divergence already carried in the event payload) and the raw
 * {@link DifferentialOutcome}s (the provider-disagreement / refusal outcomes a
 * collector records as healthy) — and emits the run's three derived artifacts:
 * `dx-summary.json`, `DX-FINDINGS.md`, and `baseline-manifest.json`.
 *
 * The module owns only report-shaped projections: the S0–S4 impact ladder
 * ({@link classifyFindingSeverity}), the content-addressed {@link computeFindingFingerprint}
 * (which EXCLUDES every volatile run datum — timestamps, latency, temp paths, driver
 * runtime ids, event ids), the dedupe fold, and the benign-divergence allowlist. It
 * reuses the substrate's comparison vocabulary ({@link DivergenceClass},
 * {@link DIVERGENCE_PRIORITY}), normalizers ({@link normalizeEol},
 * {@link stableStringify}), and digest ({@link sha256Hex}) rather than re-deriving any.
 */

import { readFileSync, writeFileSync } from "node:fs";

import type { ProviderName } from "../baseline/bridgeClient.js";
import {
  DIVERGENCE_PRIORITY,
  type DifferentialOutcome,
  type DivergenceClass,
} from "../differential/index.js";
import type { Severity } from "../collectors/index.js";
import { normalizeEol, stableStringify } from "../normalize/index.js";
import type {
  Confidence,
  MappingPolicy,
  ProbeMethod,
  RequiredDriver,
  SemanticDimension,
} from "../scenario/index.js";
import type { CollectorEvent, CollectorSignal } from "../collectors/index.js";
import { sha256Hex } from "../vendorManifest.js";

// ── finding severity (S0–S4 impact ladder) ──────────────────────────────────────

/**
 * The DX impact ladder, most severe first:
 *  - `S0` — crash / panic / server hang / data loss / unrecovered provider death;
 *  - `S1` — persistent editing blocker, false-red diagnostic, missing definition,
 *    auto-import accept failure, extension-confirmed user-visible No Suggestions;
 *  - `S2` — wrong hover, raw-LSP transient collapse, generated-only unmapped
 *    definition, impossible/default diagnostic range, steady-state compile storm;
 *  - `S3` — warning correlated with recoverable behavior, latency regression,
 *    map-absent on a non-required probe;
 *  - `S4` — informational skip, benign allowlisted divergence, tsgo-vs-tsserver
 *    baseline disagreement.
 */
export const FINDING_SEVERITIES = ["S0", "S1", "S2", "S3", "S4"] as const;
export type FindingSeverity = (typeof FINDING_SEVERITIES)[number];

/** Total order: a lower rank is MORE severe (S0 = 0). */
export const FINDING_SEVERITY_RANK: Record<FindingSeverity, number> = {
  S0: 0,
  S1: 1,
  S2: 2,
  S3: 3,
  S4: 4,
};

/** The more severe of two finding severities (the lower rank). */
export function worstFindingSeverity(a: FindingSeverity, b: FindingSeverity): FindingSeverity {
  return FINDING_SEVERITY_RANK[a] <= FINDING_SEVERITY_RANK[b] ? a : b;
}

/** The unallowlisted severities a CI gate treats as failures. */
export function isFailingSeverity(severity: FindingSeverity): boolean {
  return FINDING_SEVERITY_RANK[severity] <= FINDING_SEVERITY_RANK.S2;
}

// ── finding signal taxonomy ──────────────────────────────────────────────────────

/**
 * The differential-only signals: the provider-disagreement / refusal outcomes that
 * the collectors record as healthy `ok` events and so carry NO {@link CollectorSignal}.
 * The report surfaces them as their own low-severity findings.
 */
export const REPORT_OUTCOME_SIGNALS = [
  "baseline_provider_disagreement",
  "baseline_ranking_signal",
  "source_map_absent",
  "baseline_artifact_stale",
  "probe_skipped",
] as const;
export type ReportOutcomeSignal = (typeof REPORT_OUTCOME_SIGNALS)[number];

/** Every signal a finding can carry: a collector signal OR a differential-only signal. */
export type FindingSignal = CollectorSignal | ReportOutcomeSignal;

/** How a finding is attributed — the discriminator that keeps the three classes apart. */
export type FindingKind = "verterDefect" | "providerDisagreement" | "informational";

// ── finding model ────────────────────────────────────────────────────────────────

/** The event linkage of a (possibly collapsed) finding. */
export interface FindingEvents {
  /** The id of the first contributing observation. */
  readonly first: string;
  /** The id of the last contributing observation (equal to `first` when only one). */
  readonly last: string;
  /** How many observations collapsed into this finding. */
  readonly count: number;
}

/** An allowlist match recorded on a reclassified finding. */
export interface AllowlistHit {
  readonly entryId: string;
  readonly reason: string;
}

/**
 * One DX finding. The schema follows the report spec — the content-addressed
 * {@link DxFinding.fingerprint}, the probe/driver/provider identity, the semantic
 * dimension / mapping policy / confidence, the verter-vs-baseline behaviors, the
 * primary {@link DxFinding.divergence} class, the S0–S4 {@link DxFinding.severity},
 * the {@link DxFinding.rootCauseHint}, the {@link DxFinding.events} linkage, and the
 * baseline-execution / skip evidence — plus the report-internal {@link DxFinding.findingKind}
 * and {@link DxFinding.allowlisted} annotations.
 */
export interface DxFinding {
  readonly fingerprint: string;
  readonly scenario: string;
  readonly fixture: string;
  readonly driver: RequiredDriver;
  readonly provider: string;
  readonly signal: FindingSignal;
  readonly semanticDimension: SemanticDimension;
  readonly mappingPolicy: MappingPolicy;
  readonly confidence: Confidence;
  readonly verterBehavior: string;
  readonly baselineBehavior: string;
  readonly divergence: DivergenceClass | null;
  readonly severity: FindingSeverity;
  readonly rootCauseHint: string | null;
  readonly events: FindingEvents;
  readonly baselineRanProbeId: string | null;
  readonly findingKind: FindingKind;
  readonly skipReason?: string;
  readonly allowlisted?: AllowlistHit;
}

// ── fingerprint ──────────────────────────────────────────────────────────────────

/** The normalized tuple the fingerprint is computed over. */
export interface FindingFingerprintInput {
  readonly scenario: string;
  readonly signal: FindingSignal;
  readonly divergenceKind: DivergenceClass | null;
  readonly semanticDimension: SemanticDimension;
  /** The baseline behavior (the expected side). */
  readonly expected: string;
  /** The verter behavior (the actual side). */
  readonly actual: string;
  readonly rootCauseHint: string | null;
}

/**
 * The stable, content-addressed finding id: a `sha256` over the EOL-normalized tuple
 * `{scenario, signal, divergenceKind, semanticDimension, normalizedExpected,
 * normalizedActual, normalizedRootCauseHint}`. The tuple shape structurally excludes the
 * volatile run keys (driver runtime ids, document versions, edit-step indices, event
 * ids, latency); the remaining volatility carried INSIDE the behavior strings — per-run
 * temp-path URIs, log timestamps, measured deltas — is projected out by the reducer
 * (workspace-root relativization + structured descriptors) BEFORE the values reach here,
 * so the SAME divergence fingerprints identically across runs, drivers, and providers.
 * EOL is normalized (the cross-platform rule) and keys are sorted ({@link stableStringify})
 * so the digest is reproducible.
 */
export function computeFindingFingerprint(input: FindingFingerprintInput): string {
  const tuple = {
    scenario: input.scenario,
    signal: input.signal,
    divergenceKind: input.divergenceKind ?? "",
    semanticDimension: input.semanticDimension,
    normalizedExpected: normalizeEol(input.expected),
    normalizedActual: normalizeEol(input.actual),
    normalizedRootCauseHint: normalizeEol(input.rootCauseHint ?? ""),
  };
  return sha256Hex(Buffer.from(stableStringify(tuple), "utf8"));
}

// ── fingerprint volatile redaction ───────────────────────────────────────────────

/** The stable token a relativized materialized-workspace root collapses to. */
const WORKSPACE_PLACEHOLDER = "<workspace>";

/** A leading RFC3339 / ISO-8601 timestamp — the volatile prefix on a server log line. */
const LEADING_LOG_TIMESTAMP = /^\s*\d{4}-\d{2}-\d{2}[T ]\d{2}:\d{2}:\d{2}(?:\.\d+)?Z?\s*/;

/** Escape a string for literal use inside a {@link RegExp}. */
function escapeRegExp(value: string): string {
  return value.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
}

/**
 * Relativize every occurrence of the materialized workspace root — in bare-path OR
 * `file://` URI form, drive-case-insensitively — to {@link WORKSPACE_PLACEHOLDER}, so
 * the per-run-random `dx-ws-XXXXXX` temp segment never enters the content-addressed
 * fingerprint. A no-op when `workspaceRoot` is undefined/empty (the pure-data path).
 */
function relativizeWorkspaceRoot(text: string, workspaceRoot: string | undefined): string {
  if (workspaceRoot === undefined || workspaceRoot.length === 0) return text;
  // The canonical root path appears verbatim inside both a bare path and a `file://`
  // URI (whose `file:///` adds a leading slash before a drive). Matching an optional
  // scheme + optional slash folds every form to one token.
  const pattern = new RegExp(`(?:file://)?/?${escapeRegExp(workspaceRoot)}`, "gi");
  return text.replace(pattern, WORKSPACE_PLACEHOLDER);
}

/**
 * The volatile-free failure category of a server log line: the leading timestamp
 * stripped and any embedded workspace path relativized, so the same logical error
 * keys identically across runs regardless of when it was logged.
 */
function logFailureCategory(line: string, workspaceRoot: string | undefined): string {
  return relativizeWorkspaceRoot(line.replace(LEADING_LOG_TIMESTAMP, ""), workspaceRoot).trim();
}

/**
 * A stable fingerprint descriptor for a signal whose `detail` embeds a measured or
 * raw-text value (a churn delta, a timestamp-bearing log line, a mapping position) and
 * that carries NO structured comparable value. Derived strictly from the structured
 * `data` fields — never the free-text `detail` — so the content-addressed id keys on
 * the failure's identity, not its volatile magnitude. Returns `undefined` for every
 * other signal, whose structured value (or already-stable detail) is hashed directly.
 */
function detailFreeFingerprintActual(
  event: CollectorEvent,
  workspaceRoot: string | undefined,
): string | undefined {
  const data = event.data;
  if (data === null || typeof data !== "object") return undefined;
  const d = data as Record<string, unknown>;
  switch (event.signal) {
    case "churn_steady_state_delta":
    case "churn_burst_aggregate":
    case "churn_attribution_uncertain": {
      // Key on scope + threshold-breach (the health flag), NOT the volatile delta.
      const scope = typeof d.scope === "string" ? d.scope : "unknown";
      return `churn scope=${scope} breached=${!event.ok}`;
    }
    case "server_error":
    case "server_warn": {
      // A normalized failure category, NOT the timestamp-bearing raw line.
      const line = typeof d.line === "string" ? d.line : "";
      return `log ${logFailureCategory(line, workspaceRoot)}`;
    }
    case "mapping_root_cause_hint":
    case "mapping_failure_benign": {
      // method + workspace-relative uri; the volatile line/character are omitted.
      const method = typeof d.method === "string" ? d.method : "unknown";
      const uri = typeof d.uri === "string" ? relativizeWorkspaceRoot(d.uri, workspaceRoot) : "";
      return `mapping ${method} ${uri}`;
    }
    default:
      return undefined;
  }
}

// ── severity classification ──────────────────────────────────────────────────────

/** The four comparator families a class-bearing divergence belongs to. */
type DivergenceFamily = "completion" | "hover" | "definition" | "diagnostics";

function assertNever(value: never): never {
  throw new Error(`unhandled finding signal: ${String(value)}`);
}

/**
 * The S0–S4 of a class-bearing divergence, by comparator family. Severity is
 * family-relative: a `verterOnly` diagnostic is a false red (S1) while a `verterOnly`
 * completion item is harmless noise (S3); a `wrongTarget`/`baselineOnly` definition is a
 * missing definition (S1) while `unmappedGenerated` is the generated-only case (S2).
 */
function divergenceClassSeverity(family: DivergenceFamily, cls: DivergenceClass): FindingSeverity {
  switch (family) {
    case "completion":
      switch (cls) {
        case "missingLabel":
        case "baselineOnly":
        case "rangeMismatch":
        case "noSuggestionsCollapse":
          return "S2";
        case "verterOnly":
        case "extraLabel":
        case "wrongKind":
        case "insertEditShape":
          return "S3";
        case "rankingMismatch":
          return "S4";
        default:
          return "S2";
      }
    case "definition":
      switch (cls) {
        case "wrongTarget":
        case "baselineOnly":
          return "S1";
        case "unmappedGenerated":
        case "rangeMismatch":
          return "S2";
        case "verterOnly":
          return "S3";
        default:
          return "S2";
      }
    case "diagnostics":
      switch (cls) {
        case "verterOnly":
          return "S1";
        case "baselineOnly":
        case "defaultRange":
        case "rangeMismatch":
          return "S2";
        case "severityMismatch":
          return "S3";
        default:
          return "S2";
      }
    case "hover":
      // Every hover divergence is a wrong-hover-detail class.
      return "S2";
  }
}

/** Inputs to the finding severity classifier. */
export interface FindingSeverityContext {
  readonly signal: FindingSignal;
  readonly divergence: DivergenceClass | null;
  readonly driver: RequiredDriver;
  /** The 3-level severity of the originating event, when the finding came from one. */
  readonly eventSeverity?: Severity;
}

/**
 * The base S0–S4 IMPACT severity of a finding, from its signal, divergence class,
 * driver, and (when it came from an event) its 3-level severity. The benign-allowlist
 * reclassification is NOT applied here — it is owned by {@link reduceFindings}, which
 * applies it to the WORST severity across a collapsed group (never downgrading a
 * crash/hang). This keeps the classifier a pure impact function.
 */
export function classifyFindingSeverity(ctx: FindingSeverityContext): FindingSeverity {
  // The 3-level `critical` class is the crash / hang / unrecovered-provider-death floor.
  if (ctx.eventSeverity === "critical") return "S0";
  switch (ctx.signal) {
    case "no_suggestions_collapse":
      // A raw-LSP candidate collapse (S2) escalates once the extension host confirms it
      // reached the user (S1).
      return ctx.driver === "extensionHost" ? "S1" : "S2";
    case "completion_parity":
      return ctx.divergence !== null ? divergenceClassSeverity("completion", ctx.divergence) : "S2";
    case "completion_required_label":
      return "S2";
    case "hover_parity":
    case "hover_vue_semantic_validity":
      return ctx.divergence !== null ? divergenceClassSeverity("hover", ctx.divergence) : "S2";
    case "hover_required_snippet":
    case "hover_invariant":
      return "S2";
    case "hover_synthetic_region_tolerated":
    case "hover_contentless_observed":
    case "hover_observed":
      return "S4";
    case "definition_parity":
      return ctx.divergence !== null ? divergenceClassSeverity("definition", ctx.divergence) : "S1";
    case "auto_import_empty_edit":
    case "auto_import_wrong_text":
    case "auto_import_not_introduced":
      return "S1";
    case "auto_import_no_candidate":
      return "S2";
    case "auto_import_applied":
      return "S4";
    case "diagnostics_parity":
    case "diagnostics_vue_semantic_validity":
      return ctx.divergence !== null
        ? divergenceClassSeverity("diagnostics", ctx.divergence)
        : "S2";
    case "diagnostics_default_range":
      return "S2";
    case "churn_steady_state_delta":
      return "S2";
    case "churn_burst_aggregate":
    case "churn_attribution_uncertain":
      return "S3";
    case "latency_breach":
      return "S3";
    case "latency_summary":
      return "S4";
    case "server_error":
      return "S2";
    case "mapping_root_cause_hint":
    case "server_warn":
      return "S3";
    case "mapping_failure_benign":
      return "S4";
    case "recovery_not_restored":
      return "S1";
    case "recovery_baseline_restored":
      return "S4";
    case "baseline_provider_disagreement":
    case "baseline_ranking_signal":
      return "S4";
    case "source_map_absent":
      return "S3";
    case "baseline_artifact_stale":
      return "S4";
    case "probe_skipped":
      return "S4";
    default:
      return assertNever(ctx.signal);
  }
}

// ── scenario context ─────────────────────────────────────────────────────────────

/** The probe facts a finding needs that the event key does not carry. */
export interface ProbeMeta {
  readonly mappingPolicy: MappingPolicy;
  readonly confidence: Confidence;
  readonly dimension: SemanticDimension;
}

/** Per-scenario context: the fixture and (optionally) the probe metadata. */
export interface ScenarioMeta {
  readonly fixture: string;
  readonly probes?: Readonly<Record<string, ProbeMeta>>;
}

/** scenario id → its {@link ScenarioMeta}. */
export type ScenarioIndex = Readonly<Record<string, ScenarioMeta>>;

/** Raised when a finding observation references a scenario the index does not declare. */
export class FindingsError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "FindingsError";
  }
}

// ── allowlist ────────────────────────────────────────────────────────────────────

/** The dedupe-key fields an allowlist entry may match against. */
export interface AllowlistMatchKey {
  readonly fixture: string;
  readonly scenario: string;
  readonly signal: FindingSignal;
  readonly divergenceKind: DivergenceClass | null;
  readonly semanticDimension: SemanticDimension;
  readonly normalizedRootCauseHint: string;
}

/** One benign-divergence allowlist entry: matches by exact fingerprint and/or dedupe key. */
export interface BenignDivergenceEntry {
  /** Stable, human-meaningful entry id (recorded on the reclassified finding). */
  readonly id: string;
  /** Why the divergence is benign. */
  readonly reason: string;
  /** Match by exact finding fingerprint. */
  readonly fingerprint?: string;
  /** Match by a subset of the dedupe key (every provided field must equal the finding's). */
  readonly match?: Partial<AllowlistMatchKey>;
}

/** The versioned benign-divergence allowlist. */
export interface BenignDivergenceAllowlist {
  readonly version: 1;
  readonly entries: readonly BenignDivergenceEntry[];
  /** Optional self-documentation; ignored by matching. */
  readonly description?: string;
}

/** Whether `entry` matches the finding identified by `fingerprint` + `keyFields`. */
function allowlistEntryMatches(
  entry: BenignDivergenceEntry,
  fingerprint: string,
  keyFields: AllowlistMatchKey,
): boolean {
  if (entry.fingerprint === undefined && entry.match === undefined) return false;
  if (entry.fingerprint !== undefined && entry.fingerprint !== fingerprint) return false;
  if (entry.match !== undefined) {
    for (const [field, expected] of Object.entries(entry.match)) {
      if (keyFields[field as keyof AllowlistMatchKey] !== expected) return false;
    }
  }
  return true;
}

/** The first allowlist entry matching the finding, or `undefined`. */
function findAllowlistMatch(
  allowlist: BenignDivergenceAllowlist | undefined,
  fingerprint: string,
  keyFields: AllowlistMatchKey,
): BenignDivergenceEntry | undefined {
  if (allowlist === undefined) return undefined;
  return allowlist.entries.find((entry) => allowlistEntryMatches(entry, fingerprint, keyFields));
}

/** The canonical on-disk name of the versioned benign-divergence allowlist. */
export const BENIGN_DIVERGENCES_V1_FILENAME = "benign-divergences.v1.json";

/** Validate one parsed allowlist entry at the trust boundary. */
function validateAllowlistEntry(value: unknown, index: number): BenignDivergenceEntry {
  if (value === null || typeof value !== "object") {
    throw new FindingsError(`benign-divergence entry ${index} must be an object`);
  }
  const entry = value as Record<string, unknown>;
  if (typeof entry.id !== "string" || entry.id.length === 0) {
    throw new FindingsError(`benign-divergence entry ${index} needs a non-empty "id"`);
  }
  if (typeof entry.reason !== "string") {
    throw new FindingsError(`benign-divergence entry ${index} needs a "reason"`);
  }
  const hasFingerprint = typeof entry.fingerprint === "string";
  const hasMatch = entry.match !== null && typeof entry.match === "object";
  if (!hasFingerprint && !hasMatch) {
    throw new FindingsError(
      `benign-divergence entry ${index} needs a "fingerprint" matcher and/or a "match" matcher`,
    );
  }
  return {
    id: entry.id,
    reason: entry.reason,
    ...(hasFingerprint ? { fingerprint: entry.fingerprint as string } : {}),
    ...(hasMatch ? { match: entry.match as Partial<AllowlistMatchKey> } : {}),
  };
}

/** Validate a parsed value as a {@link BenignDivergenceAllowlist} (the v1 schema). */
export function validateBenignAllowlist(value: unknown): BenignDivergenceAllowlist {
  if (value === null || typeof value !== "object") {
    throw new FindingsError("benign-divergence allowlist must be an object");
  }
  const doc = value as Record<string, unknown>;
  if (doc.version !== 1) {
    throw new FindingsError("benign-divergence allowlist version must be 1");
  }
  if (!Array.isArray(doc.entries)) {
    throw new FindingsError("benign-divergence allowlist entries must be an array");
  }
  const entries = doc.entries.map((entry, index) => validateAllowlistEntry(entry, index));
  return {
    version: 1,
    entries,
    ...(typeof doc.description === "string" ? { description: doc.description } : {}),
  };
}

/** Read and validate a benign-divergence allowlist JSON file. */
export function loadBenignAllowlist(filePath: string): BenignDivergenceAllowlist {
  return validateBenignAllowlist(JSON.parse(readFileSync(filePath, "utf8")));
}

// ── reducer inputs ───────────────────────────────────────────────────────────────

/** A collector event with an optional correlated root-cause hint. */
export interface EventObservation {
  readonly event: CollectorEvent;
  readonly rootCauseHint?: string;
}

/** A differential outcome, situated in the scenario + driver context it lacks on its own. */
export interface SituatedOutcome {
  readonly scenario: string;
  readonly driver: RequiredDriver;
  readonly outcome: DifferentialOutcome;
  readonly rootCauseHint?: string;
}

/** Inputs to {@link reduceFindings}. */
export interface ReduceFindingsInput {
  readonly scenarios: ScenarioIndex;
  readonly events?: readonly EventObservation[];
  readonly outcomes?: readonly SituatedOutcome[];
  readonly allowlist?: BenignDivergenceAllowlist;
  /**
   * The materialized workspace root, used ONLY to relativize per-run temp-path URIs
   * out of the content-addressed fingerprint (see {@link computeFindingFingerprint}).
   * Omitted in pure-data tests where no path enters the hashed tuple.
   */
  readonly workspaceRoot?: string;
}

/** Distinct probes a baseline executed this run. */
export interface BaselineRanSummary {
  readonly probes: number;
  readonly probeIds: readonly string[];
}

/** An allowlist match recorded for the run. */
export interface AllowlistHitRecord {
  readonly entryId: string;
  readonly reason: string;
  readonly fingerprint: string;
}

/** The reducer output: the deduped findings plus run-level accounting. */
export interface ReduceFindingsResult {
  readonly findings: readonly DxFinding[];
  readonly baselineRan: BaselineRanSummary;
  readonly allowlistHits: readonly AllowlistHitRecord[];
}

// ── internal draft model ─────────────────────────────────────────────────────────

interface FindingDraft {
  readonly eventId: string;
  readonly ordinal: number;
  readonly dedupeKey: string;
  readonly fingerprint: string;
  readonly keyFields: AllowlistMatchKey;
  readonly severity: FindingSeverity;
  readonly base: Omit<DxFinding, "fingerprint" | "severity" | "events" | "allowlisted">;
}

const DIVERGENCE_CLASSES = new Set<string>(Object.keys(DIVERGENCE_PRIORITY));

function isDivergenceClass(value: unknown): value is DivergenceClass {
  return typeof value === "string" && DIVERGENCE_CLASSES.has(value);
}

/** The semantic dimension a collector signal structurally carries when no probe meta is known. */
function dimensionForSignal(signal: FindingSignal): SemanticDimension {
  return signal.endsWith("_vue_semantic_validity") ? "vueSemanticValidity" : "artifactParity";
}

/** Whether a signal's findings imply a baseline executed the probe. */
function signalImpliesBaseline(signal: FindingSignal): boolean {
  return signal.endsWith("_parity") || signal.endsWith("_vue_semantic_validity");
}

/** Render a behavior value to a stable single-line string (sorted-key JSON for structures). */
function behaviorString(value: unknown, fallback: string): string {
  if (value === undefined) return fallback;
  if (typeof value === "string") return value;
  return stableStringify(value);
}

/** Safely read a collector event's embedded divergence payload. */
function readEventDivergence(data: unknown): {
  divergence: DivergenceClass | null;
  verterValue: unknown;
  baselineValue: unknown;
} {
  if (data === null || typeof data !== "object") {
    return { divergence: null, verterValue: undefined, baselineValue: undefined };
  }
  const d = data as Record<string, unknown>;
  return {
    divergence: isDivergenceClass(d.class) ? d.class : null,
    verterValue: d.verterValue,
    baselineValue: d.baselineValue,
  };
}

function dedupeKeyOf(kind: FindingKind, keyFields: AllowlistMatchKey): string {
  // The spec primary key {fixture, scenario, signal, divergenceKind, semanticDimension,
  // normalizedRootCauseHint} PLUS the finding kind: provider disagreement and informational
  // outcomes must never collapse into a verter defect (the "keep separate" rule).
  return stableStringify({ kind, ...keyFields });
}

function draftFromEvent(
  obs: EventObservation,
  ordinal: number,
  scenarios: ScenarioIndex,
  workspaceRoot: string | undefined,
): FindingDraft {
  const { event } = obs;
  const scenario = event.key.scenario;
  const meta = scenarios[scenario];
  if (meta === undefined) {
    throw new FindingsError(
      `event references scenario "${scenario}" absent from the scenario index`,
    );
  }
  const signal = event.signal;
  const probeMeta = meta.probes?.[event.key.probe];
  const { divergence, verterValue, baselineValue } = readEventDivergence(event.data);
  const verterBehavior = behaviorString(verterValue, event.detail);
  const baselineBehavior = behaviorString(baselineValue, "");
  const rootCauseHint = obs.rootCauseHint ?? null;
  const dimension = probeMeta?.dimension ?? dimensionForSignal(signal);
  const keyFields: AllowlistMatchKey = {
    fixture: meta.fixture,
    scenario,
    signal,
    divergenceKind: divergence,
    semanticDimension: dimension,
    normalizedRootCauseHint: normalizeEol(rootCauseHint ?? ""),
  };
  const severity = classifyFindingSeverity({
    signal,
    divergence,
    driver: event.key.driver,
    eventSeverity: event.severity,
  });
  // The fingerprint hashes a VOLATILE-FREE projection of the semantic value: a
  // structured descriptor for signals whose detail embeds a measured/raw value, else
  // the behavior strings with the per-run workspace root relativized out. The DISPLAY
  // behaviors below stay verbatim (full paths aid same-run debugging).
  const fingerprintActual =
    detailFreeFingerprintActual(event, workspaceRoot) ??
    relativizeWorkspaceRoot(verterBehavior, workspaceRoot);
  return {
    eventId: `${scenario}/${event.key.probe}/${signal}#${ordinal}`,
    ordinal,
    dedupeKey: dedupeKeyOf("verterDefect", keyFields),
    fingerprint: computeFindingFingerprint({
      scenario,
      signal,
      divergenceKind: divergence,
      semanticDimension: dimension,
      expected: relativizeWorkspaceRoot(baselineBehavior, workspaceRoot),
      actual: fingerprintActual,
      rootCauseHint:
        rootCauseHint === null ? null : relativizeWorkspaceRoot(rootCauseHint, workspaceRoot),
    }),
    keyFields,
    severity,
    base: {
      scenario,
      fixture: meta.fixture,
      driver: event.key.driver,
      provider: event.key.provider,
      signal,
      semanticDimension: dimension,
      mappingPolicy: probeMeta?.mappingPolicy ?? "none",
      confidence: probeMeta?.confidence ?? "low",
      verterBehavior,
      baselineBehavior,
      divergence,
      rootCauseHint,
      baselineRanProbeId: signalImpliesBaseline(signal) ? event.key.probe : null,
      findingKind: "verterDefect",
    },
  };
}

function divergenceSignalForMethod(
  method: ProbeMethod,
  dimension: SemanticDimension,
): FindingSignal {
  const vue = dimension === "vueSemanticValidity";
  switch (method) {
    case "completion":
      return "completion_parity";
    case "hover":
      return vue ? "hover_vue_semantic_validity" : "hover_parity";
    case "definition":
      return "definition_parity";
    case "diagnostics":
      return vue ? "diagnostics_vue_semantic_validity" : "diagnostics_parity";
    default:
      // Divergence outcomes only arise from the four comparator methods above; this
      // defensive default keeps the function total over `ProbeMethod`.
      return vue ? "diagnostics_vue_semantic_validity" : "completion_parity";
  }
}

function signalForOutcome(outcome: DifferentialOutcome): FindingSignal {
  switch (outcome.kind) {
    case "divergence":
      return divergenceSignalForMethod(outcome.probe.method, outcome.probe.dimension);
    case "agreement":
      return divergenceSignalForMethod(outcome.probe.method, outcome.probe.dimension);
    case "baselineDisagreement":
      return "baseline_provider_disagreement";
    case "rankingSignal":
      return "baseline_ranking_signal";
    case "mapAbsent":
      return "source_map_absent";
    case "baselineArtifactStale":
      return "baseline_artifact_stale";
    case "skipped":
      return "probe_skipped";
  }
}

function findingKindForOutcome(kind: DifferentialOutcome["kind"]): FindingKind {
  switch (kind) {
    case "baselineDisagreement":
    case "rankingSignal":
      return "providerDisagreement";
    case "mapAbsent":
    case "baselineArtifactStale":
    case "skipped":
      return "informational";
    case "divergence":
    case "agreement":
      return "verterDefect";
  }
}

/** The provider label a finding carries for an outcome (both sides of a disagreement). */
function outcomeProvider(outcome: DifferentialOutcome): string {
  switch (outcome.kind) {
    case "divergence":
    case "agreement":
    case "rankingSignal":
      return outcome.provider;
    case "baselineDisagreement":
      return outcome.providers.join("+");
    case "mapAbsent":
    case "baselineArtifactStale":
    case "skipped":
      return outcome.provider ?? "none";
  }
}

/** The verter/baseline behaviors carried by an outcome (primary finding, when several). */
function outcomeBehaviors(outcome: DifferentialOutcome): {
  divergence: DivergenceClass | null;
  verter: string;
  baseline: string;
} {
  switch (outcome.kind) {
    case "divergence":
      return {
        divergence: outcome.class,
        verter: behaviorString(outcome.verterValue, outcome.detail),
        baseline: behaviorString(outcome.baselineValue, ""),
      };
    case "baselineDisagreement":
    case "rankingSignal": {
      const primary = outcome.findings[0];
      return {
        divergence: primary?.class ?? null,
        verter: behaviorString(primary?.verterValue, outcome.detail),
        baseline: behaviorString(primary?.baselineValue, ""),
      };
    }
    case "agreement":
      return { divergence: null, verter: outcome.detail ?? "agreement", baseline: "" };
    case "mapAbsent":
    case "baselineArtifactStale":
      return { divergence: null, verter: outcome.detail, baseline: "" };
    case "skipped":
      return { divergence: null, verter: outcome.reason, baseline: "" };
  }
}

function draftFromOutcome(
  situated: SituatedOutcome,
  ordinal: number,
  scenarios: ScenarioIndex,
  workspaceRoot: string | undefined,
): FindingDraft {
  const { outcome, scenario, driver } = situated;
  const meta = scenarios[scenario];
  if (meta === undefined) {
    throw new FindingsError(
      `outcome references scenario "${scenario}" absent from the scenario index`,
    );
  }
  const signal = signalForOutcome(outcome);
  const kind = findingKindForOutcome(outcome.kind);
  const { divergence, verter, baseline } = outcomeBehaviors(outcome);
  const rootCauseHint = situated.rootCauseHint ?? null;
  const dimension = outcome.probe.dimension;
  const keyFields: AllowlistMatchKey = {
    fixture: meta.fixture,
    scenario,
    signal,
    divergenceKind: divergence,
    semanticDimension: dimension,
    normalizedRootCauseHint: normalizeEol(rootCauseHint ?? ""),
  };
  const severity = classifyFindingSeverity({ signal, divergence, driver });
  const base: FindingDraft["base"] = {
    scenario,
    fixture: meta.fixture,
    driver,
    provider: outcomeProvider(outcome),
    signal,
    semanticDimension: dimension,
    mappingPolicy: outcome.probe.mappingPolicy,
    confidence: outcome.probe.confidence,
    verterBehavior: verter,
    baselineBehavior: baseline,
    divergence,
    rootCauseHint,
    baselineRanProbeId: outcome.kind === "skipped" ? null : outcome.probe.probeId,
    findingKind: kind,
    ...(outcome.kind === "skipped" ? { skipReason: outcome.reason } : {}),
  };
  return {
    eventId: `${scenario}/${outcome.probe.probeId}/${signal}#${ordinal}`,
    ordinal,
    dedupeKey: dedupeKeyOf(kind, keyFields),
    // The fingerprint relativizes the per-run workspace root out of the behavior
    // strings (e.g. a definition target's temp-root uri); the DISPLAY behaviors above
    // stay verbatim. Outcome signals carry no measured-detail descriptor.
    fingerprint: computeFindingFingerprint({
      scenario,
      signal,
      divergenceKind: divergence,
      semanticDimension: dimension,
      expected: relativizeWorkspaceRoot(baseline, workspaceRoot),
      actual: relativizeWorkspaceRoot(verter, workspaceRoot),
      rootCauseHint:
        rootCauseHint === null ? null : relativizeWorkspaceRoot(rootCauseHint, workspaceRoot),
    }),
    keyFields,
    severity,
    base,
  };
}

/** Whether a draft is a finding (vs a healthy observation that only feeds accounting). */
function isFindingDraft(
  eventOk: boolean | undefined,
  kind: DifferentialOutcome["kind"] | undefined,
): boolean {
  if (eventOk !== undefined) return eventOk === false;
  return kind !== undefined && kind !== "agreement";
}

// ── reducer ──────────────────────────────────────────────────────────────────────

/**
 * Fold collector events + differential outcomes into deduped {@link DxFinding}s.
 *
 * Healthy events (`ok`) and agreements are NOT findings — but a baseline that ran on
 * them still counts toward {@link ReduceFindingsResult.baselineRan}. Flagged events and
 * non-agreement outcomes become draft findings; drafts that share the dedupe key
 * `{findingKind, fixture, scenario, signal, divergenceKind, semanticDimension,
 * normalizedRootCauseHint}` collapse into one finding carrying the worst severity, the
 * first/last contributing event ids, and the collapsed count. The benign allowlist
 * reclassifies a matched finding to S4 (recorded, never silently dropped).
 */
export function reduceFindings(input: ReduceFindingsInput): ReduceFindingsResult {
  const { scenarios, allowlist, workspaceRoot } = input;
  const events = input.events ?? [];
  const outcomes = input.outcomes ?? [];

  const drafts: FindingDraft[] = [];
  const baselineProbeIds = new Set<string>();
  let ordinal = 0;

  for (const obs of events) {
    if (signalImpliesBaseline(obs.event.signal)) baselineProbeIds.add(obs.event.key.probe);
    if (isFindingDraft(obs.event.ok, undefined)) {
      drafts.push(draftFromEvent(obs, ordinal, scenarios, workspaceRoot));
    }
    ordinal++;
  }
  for (const situated of outcomes) {
    const { outcome } = situated;
    if (outcome.kind !== "skipped" && "probe" in outcome) {
      if (outcome.kind !== "mapAbsent" && outcome.kind !== "baselineArtifactStale") {
        baselineProbeIds.add(outcome.probe.probeId);
      } else if (outcome.provider !== undefined) {
        baselineProbeIds.add(outcome.probe.probeId);
      }
    }
    if (isFindingDraft(undefined, outcome.kind)) {
      drafts.push(draftFromOutcome(situated, ordinal, scenarios, workspaceRoot));
    }
    ordinal++;
  }

  const groups = new Map<string, FindingDraft[]>();
  const order: string[] = [];
  for (const draft of drafts) {
    const existing = groups.get(draft.dedupeKey);
    if (existing === undefined) {
      groups.set(draft.dedupeKey, [draft]);
      order.push(draft.dedupeKey);
    } else {
      existing.push(draft);
    }
  }

  const allowlistHits: AllowlistHitRecord[] = [];
  const findings: DxFinding[] = [];
  for (const dedupeKey of order) {
    const group = groups.get(dedupeKey) as FindingDraft[];
    const sortedByOrdinal = [...group].sort((a, b) => a.ordinal - b.ordinal);
    const first = sortedByOrdinal[0];
    const last = sortedByOrdinal[sortedByOrdinal.length - 1];
    // The representative carries the finding's scalar facts: the worst-severity draft,
    // tie-broken by the earliest ordinal (deterministic).
    const representative = [...group].sort(
      (a, b) =>
        FINDING_SEVERITY_RANK[a.severity] - FINDING_SEVERITY_RANK[b.severity] ||
        a.ordinal - b.ordinal,
    )[0];
    const worst = group.reduce<FindingSeverity>(
      (acc, d) => worstFindingSeverity(acc, d.severity),
      "S4",
    );
    // A crash/hang (S0) is never benign: any allowlist match is ignored ENTIRELY —
    // no reclassification, no annotation, no recorded hit — so an S0 always fails the
    // gate and counts under totals.failures end-to-end. A non-S0 match drops to S4.
    const match =
      worst === "S0"
        ? undefined
        : findAllowlistMatch(allowlist, representative.fingerprint, representative.keyFields);
    const severity = match !== undefined ? "S4" : worst;
    const finding: DxFinding = {
      ...representative.base,
      fingerprint: representative.fingerprint,
      severity,
      events: { first: first.eventId, last: last.eventId, count: group.length },
      ...(match !== undefined ? { allowlisted: { entryId: match.id, reason: match.reason } } : {}),
    };
    findings.push(finding);
    if (match !== undefined) {
      allowlistHits.push({
        entryId: match.id,
        reason: match.reason,
        fingerprint: representative.fingerprint,
      });
    }
  }

  findings.sort(
    (a, b) =>
      FINDING_SEVERITY_RANK[a.severity] - FINDING_SEVERITY_RANK[b.severity] ||
      (a.fingerprint < b.fingerprint ? -1 : a.fingerprint > b.fingerprint ? 1 : 0) ||
      (a.scenario < b.scenario ? -1 : a.scenario > b.scenario ? 1 : 0),
  );

  const probeIds = [...baselineProbeIds].sort();
  return {
    findings,
    baselineRan: { probes: probeIds.length, probeIds },
    allowlistHits,
  };
}

// ── baseline manifest ────────────────────────────────────────────────────────────

/** Per-provider baseline execution: the distinct probe ids that provider ran. */
export interface BaselineProviderManifest {
  readonly ranProbeIds: readonly string[];
  readonly probeCount: number;
}

/** The baseline-execution manifest emitted as `baseline-manifest.json`. */
export interface BaselineManifest {
  readonly providers: Record<ProviderName, BaselineProviderManifest>;
  /** Total (provider, probe) baseline executions across all providers. */
  readonly totalExecutions: number;
  /** The distinct probe ids a baseline executed, across all providers. */
  readonly distinctProbeIds: readonly string[];
}

const PROVIDER_NAMES: readonly ProviderName[] = ["tsgo", "tsserver"];

/** Build the per-provider baseline-execution manifest from the run's situated outcomes. */
export function buildBaselineManifest(outcomes: readonly SituatedOutcome[]): BaselineManifest {
  const ran: Record<ProviderName, Set<string>> = { tsgo: new Set(), tsserver: new Set() };
  const distinct = new Set<string>();
  const record = (provider: ProviderName, probeId: string): void => {
    ran[provider].add(probeId);
    distinct.add(probeId);
  };
  for (const { outcome } of outcomes) {
    switch (outcome.kind) {
      case "divergence":
      case "agreement":
      case "rankingSignal":
        record(outcome.provider, outcome.probe.probeId);
        break;
      case "baselineDisagreement":
        for (const provider of outcome.providers) record(provider, outcome.probe.probeId);
        break;
      case "mapAbsent":
      case "baselineArtifactStale":
        if (outcome.provider !== undefined) record(outcome.provider, outcome.probe.probeId);
        break;
      case "skipped":
        break;
    }
  }
  let totalExecutions = 0;
  const providers = {} as Record<ProviderName, BaselineProviderManifest>;
  for (const provider of PROVIDER_NAMES) {
    const ids = [...ran[provider]].sort();
    providers[provider] = { ranProbeIds: ids, probeCount: ids.length };
    totalExecutions += ids.length;
  }
  return { providers, totalExecutions, distinctProbeIds: [...distinct].sort() };
}

/** Serialize the baseline manifest deterministically (2-space JSON, trailing newline). */
export function serializeBaselineManifest(manifest: BaselineManifest): string {
  return `${JSON.stringify(manifest, null, 2)}\n`;
}

// ── summary ──────────────────────────────────────────────────────────────────────

/** The bug-report reconciliation metadata embedded in the summary. */
export interface BugReportReconciliationSummary {
  readonly status: "reconciled" | "skipped_missing_file";
  readonly bugReportPath?: string;
  readonly matchedFindings?: number;
  readonly unmatchedFindings?: number;
  readonly knownBugsWithoutFinding?: number;
}

/** Inputs to {@link buildSummary}. */
export interface BuildSummaryInput {
  readonly findings: readonly DxFinding[];
  readonly baselineRan: BaselineRanSummary;
  readonly allowlistHits: readonly AllowlistHitRecord[];
  readonly allowlistVersion: number;
  readonly bugReportReconciliation?: BugReportReconciliationSummary;
}

/** The `dx-summary.json` shape: counts by severity / dimension / signal + run metadata. */
export interface DxSummary {
  readonly totals: {
    readonly findings: number;
    readonly failures: number;
    readonly allowlisted: number;
    readonly providerDisagreements: number;
    readonly informational: number;
  };
  readonly bySeverity: Record<FindingSeverity, number>;
  readonly byDimension: Record<SemanticDimension, number>;
  readonly bySignal: Record<string, number>;
  readonly baselineRan: BaselineRanSummary;
  readonly allowlist: { readonly version: number; readonly hits: number };
  readonly bugReportReconciliation: BugReportReconciliationSummary;
}

/** Build the deterministic `dx-summary.json` aggregate. */
export function buildSummary(input: BuildSummaryInput): DxSummary {
  const bySeverity: Record<FindingSeverity, number> = { S0: 0, S1: 0, S2: 0, S3: 0, S4: 0 };
  const byDimension: Record<SemanticDimension, number> = {
    artifactParity: 0,
    vueSemanticValidity: 0,
  };
  const bySignalMap = new Map<string, number>();
  let failures = 0;
  let allowlisted = 0;
  let providerDisagreements = 0;
  let informational = 0;
  for (const finding of input.findings) {
    bySeverity[finding.severity]++;
    byDimension[finding.semanticDimension]++;
    bySignalMap.set(finding.signal, (bySignalMap.get(finding.signal) ?? 0) + 1);
    if (finding.allowlisted !== undefined) allowlisted++;
    else if (isFailingSeverity(finding.severity)) failures++;
    if (finding.findingKind === "providerDisagreement") providerDisagreements++;
    if (finding.findingKind === "informational") informational++;
  }
  const bySignal: Record<string, number> = {};
  for (const signal of [...bySignalMap.keys()].sort())
    bySignal[signal] = bySignalMap.get(signal) as number;
  return {
    totals: {
      findings: input.findings.length,
      failures,
      allowlisted,
      providerDisagreements,
      informational,
    },
    bySeverity,
    byDimension,
    bySignal,
    baselineRan: input.baselineRan,
    allowlist: { version: input.allowlistVersion, hits: input.allowlistHits.length },
    bugReportReconciliation: input.bugReportReconciliation ?? { status: "skipped_missing_file" },
  };
}

/** Serialize the summary deterministically (2-space JSON, trailing newline). */
export function serializeSummary(summary: DxSummary): string {
  return `${JSON.stringify(summary, null, 2)}\n`;
}

// ── markdown ─────────────────────────────────────────────────────────────────────

function inlineCode(value: string): string {
  const collapsed = normalizeEol(value).replace(/\n/g, " ");
  // Use a fence longer than the longest backtick run inside the value.
  const longest = (collapsed.match(/`+/g) ?? []).reduce((n, run) => Math.max(n, run.length), 0);
  const fence = "`".repeat(longest + 1);
  const pad = longest > 0 ? " " : "";
  return `${fence}${pad}${collapsed}${pad}${fence}`;
}

/** An inline code span, or a plain em-dash for an empty value (never a bare empty `` `` `` span). */
function inlineCodeOrDash(value: string): string {
  return value.length === 0 ? "—" : inlineCode(value);
}

/** Render the deterministic `DX-FINDINGS.md`. Each finding carries its stable fingerprint. */
export function renderFindingsMarkdown(findings: readonly DxFinding[]): string {
  const lines: string[] = ["# Verter DX Findings", ""];
  const bySeverity: Record<FindingSeverity, number> = { S0: 0, S1: 0, S2: 0, S3: 0, S4: 0 };
  for (const finding of findings) bySeverity[finding.severity]++;
  lines.push(`Total findings: ${findings.length}`);
  lines.push(`Severity: ${FINDING_SEVERITIES.map((s) => `${s}=${bySeverity[s]}`).join(", ")}`, "");
  if (findings.length === 0) {
    lines.push("_No findings._", "");
    return `${lines.join("\n")}\n`;
  }
  for (const finding of findings) {
    lines.push(
      `## ${finding.severity} — ${finding.scenario} / ${finding.signal}`,
      "",
      `- fingerprint: \`${finding.fingerprint}\``,
      `- fixture: ${finding.fixture}`,
      `- driver / provider: ${finding.driver} / ${finding.provider}`,
      `- dimension: ${finding.semanticDimension}`,
      `- mapping policy / confidence: ${finding.mappingPolicy} / ${finding.confidence}`,
      `- finding kind: ${finding.findingKind}`,
      `- divergence: ${finding.divergence ?? "—"}`,
      `- verter: ${inlineCodeOrDash(finding.verterBehavior)}`,
      `- baseline: ${inlineCodeOrDash(finding.baselineBehavior)}`,
      `- root cause hint: ${inlineCodeOrDash(finding.rootCauseHint ?? "")}`,
      `- events: first=${finding.events.first} last=${finding.events.last} count=${finding.events.count}`,
      `- baseline ran probe: ${finding.baselineRanProbeId ?? "—"}`,
    );
    if (finding.skipReason !== undefined) lines.push(`- skip reason: ${finding.skipReason}`);
    if (finding.allowlisted !== undefined) {
      lines.push(`- allowlisted: ${finding.allowlisted.entryId} — ${finding.allowlisted.reason}`);
    }
    lines.push("");
  }
  return `${lines.join("\n")}\n`;
}

// ── artifact filenames + writers ─────────────────────────────────────────────────

export const DX_SUMMARY_FILENAME = "dx-summary.json";
export const DX_FINDINGS_FILENAME = "DX-FINDINGS.md";
export const BASELINE_MANIFEST_FILENAME = "baseline-manifest.json";

/** Write the deterministic `dx-summary.json`. */
export function writeSummary(filePath: string, summary: DxSummary): void {
  writeFileSync(filePath, serializeSummary(summary), "utf8");
}

/** Write the deterministic `DX-FINDINGS.md`. */
export function writeFindingsMarkdown(filePath: string, findings: readonly DxFinding[]): void {
  writeFileSync(filePath, renderFindingsMarkdown(findings), "utf8");
}

/** Write the deterministic `baseline-manifest.json`. */
export function writeBaselineManifest(filePath: string, manifest: BaselineManifest): void {
  writeFileSync(filePath, serializeBaselineManifest(manifest), "utf8");
}
