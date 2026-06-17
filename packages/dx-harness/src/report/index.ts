/**
 * The Verter DX report generator.
 *
 * The terminal stage of the harness: it CONSUMES the landed substrate — the
 * collector event stream, the differential outcomes, the scenario model, and the
 * pure normalizers — and produces the run's durable artifacts. It owns no
 * comparison, normalization, or severity logic of its own beyond the report-shaped
 * projections (the S0–S4 impact ladder, the content-addressed fingerprint, the
 * dedupe fold, the JUnit/markdown/JSON emitters).
 *
 * Four modules:
 *  - {@link ./events} — the `dx-events.jsonl` serializer/deserializer + file sink;
 *  - {@link ./findings} — the finding model + reducer + `dx-summary.json` /
 *    `DX-FINDINGS.md` / `baseline-manifest.json` emitters + the benign allowlist;
 *  - {@link ./junit} — the `junit.xml` emitter;
 *  - {@link ./bugReportReconcile} — reconciliation against a `BUG-REPORT.md`.
 */

export {
  DX_EVENTS_FILENAME,
  JsonlEventSink,
  ReportEventsError,
  parseEvents,
  readEventsJsonl,
  serializeEvents,
  writeEventsJsonl,
  type SerializeEventsOptions,
} from "./events.js";

export {
  BASELINE_MANIFEST_FILENAME,
  BENIGN_DIVERGENCES_V1_FILENAME,
  DX_FINDINGS_FILENAME,
  DX_SUMMARY_FILENAME,
  FINDING_SEVERITIES,
  FINDING_SEVERITY_RANK,
  FindingsError,
  REPORT_OUTCOME_SIGNALS,
  buildBaselineManifest,
  buildSummary,
  classifyFindingSeverity,
  computeFindingFingerprint,
  isFailingSeverity,
  loadBenignAllowlist,
  reduceFindings,
  renderFindingsMarkdown,
  serializeBaselineManifest,
  serializeSummary,
  validateBenignAllowlist,
  worstFindingSeverity,
  writeBaselineManifest,
  writeFindingsMarkdown,
  writeSummary,
  type AllowlistHit,
  type AllowlistHitRecord,
  type AllowlistMatchKey,
  type BaselineManifest,
  type BaselineProviderManifest,
  type BaselineRanSummary,
  type BenignDivergenceAllowlist,
  type BenignDivergenceEntry,
  type BugReportReconciliationSummary,
  type BuildSummaryInput,
  type DxFinding,
  type DxSummary,
  type EventObservation,
  type FindingEvents,
  type FindingKind,
  type FindingSeverity,
  type FindingSeverityContext,
  type FindingSignal,
  type FindingFingerprintInput,
  type ProbeMeta,
  type ReduceFindingsInput,
  type ReduceFindingsResult,
  type ReportOutcomeSignal,
  type ScenarioIndex,
  type ScenarioMeta,
  type SituatedOutcome,
} from "./findings.js";

export { JUNIT_FILENAME, renderJunitXml, writeJunitXml, type JunitOptions } from "./junit.js";

export {
  bugReportPathForWorktree,
  reconcileBugReport,
  reconciliationSummary,
  type BugReportReconciliation,
  type BugReportReconciliationStatus,
  type FindingReconciliation,
  type FindingReconciliationStatus,
  type ReconcileBugReportInput,
} from "./bugReportReconcile.js";
