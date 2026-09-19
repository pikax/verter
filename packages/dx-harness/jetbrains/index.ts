/**
 * `@verter/dx-harness/jetbrains` — real-IDE comparison harness and fixture
 * driver (JBT1H). Consumed by JBT9 (parity) and JBT10 (comparative qualification).
 * Does not evaluate superiority and does not add a second semantic engine.
 */

export {
  BASIS_KINDS,
  COMPARISON_WORKFLOWS,
  COMPLETENESS_STATES,
  REAL_IDE_HOST,
  REJECTED_UI_HOSTS,
  REQUIRED_METRIC_IDS,
  SESSION_STATES,
  SIDES,
  isBasisKind,
  isCompletenessState,
  isRejectedUiHost,
  isSessionState,
  isSide,
  measuredMetric,
  metricValue,
  unknownMetric,
  type BasisKind,
  type CaptureHostKind,
  type ComparisonWorkflowId,
  type CompletenessState,
  type CorrectnessRow,
  type IdeCapture,
  type Metric,
  type PaintSample,
  type ProcessTreeMember,
  type ProcessTreeSnapshot,
  type ProductReceiptBasis,
  type RejectedUiHost,
  type RequiredMetricId,
  type SessionState,
  type Side,
  type TypeProviderStatus,
  type VersionSet,
} from "./types.js";

export { assertRealIdeUiEvidence, classifyUiEvidence, type UiEvidenceVerdict } from "./evidence.js";

export {
  evaluateMemoryRow,
  memoryMetricsFromCapture,
  providerRequiredForMemory,
  type MemoryRow,
  type MemoryRowStatus,
} from "./process-tree.js";

export {
  compareSwappedPairs,
  labelOutliers,
  pairOrderFromSeed,
  parseSide,
  recordPair,
  sameReceiptBasis,
  swappedOrder,
  type ComparabilityVerdict,
  type NoiseBound,
  type PairOrder,
  type PairRecord,
  type PairedCampaign,
  type TimedSide,
} from "./paired-run.js";

export {
  FIXTURE_CORPUS,
  OFFICIAL_PENDING_REASON,
  SKELETON_UNSUPPORTED_REASON,
  defaultWorkflowRows,
  ingestCapture,
  parseIdeCapture,
  rejectRpcOnlyAsPaint,
  runPairedCampaign,
  versionsFromCaptureFields,
  workflowRow,
  type DriverRunRequest,
  type DriverRunResult,
} from "./driver.js";
