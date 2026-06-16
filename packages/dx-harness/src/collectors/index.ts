/**
 * The Verter DX signal collectors — the raw-LSP side.
 *
 * Each collector drives the shared substrate (the `@verter/lsp-test-client`
 * `LspClient`, the pure LSP-response normalizers, the artifact-parity / curated-oracle
 * comparators, and the startup/quiescence gates) and folds its observations into
 * {@link CollectorEvent}s on an {@link EventSink}. The DETECTION/decision logic of
 * every collector is a pure function over already-normalized inputs (unit-tested with
 * in-memory fixtures); the LIVE driving paths spawn the real `verter-lsp` binary and
 * are env-gated in the integration suite.
 */

export {
  COLLECTOR_NAMES,
  COLLECTOR_SIGNALS,
  CollectingSink,
  SEVERITIES,
  SEVERITY_RANK,
  atLeastAsSevere,
  collectorEvent,
  isCollectorName,
  isCollectorSignal,
  isSeverity,
  serializeCollectorEvent,
  toJsonl,
  type CollectorEvent,
  type CollectorEventInput,
  type CollectorEventKey,
  type CollectorName,
  type CollectorSignal,
  type EventSink,
  type Severity,
  type SignalProvenance,
} from "./event.js";

export { EditBuffer, runEditScript, type ContentChange, type Tick } from "./editLoop.js";

export {
  closeDocument,
  offsetToPosition,
  openDocument,
  sendTickChange,
  tickCursorPosition,
  type CollectorLspClient,
} from "./client.js";

export {
  classifyCompletionSample,
  collectCompletion,
  completionTriggerCharacters,
  triggerContextForChar,
  type CollectCompletionOptions,
  type CompletionBaseline,
  type CompletionSampleInput,
  type CompletionTriggerContext,
} from "./completion.js";

export {
  classifyHoverSample,
  collectHover,
  type CollectHoverOptions,
  type HoverBaseline,
  type HoverInvariant,
  type HoverOracle,
  type HoverSampleInput,
} from "./hover.js";

export {
  classifyDefinitionSample,
  collectDefinition,
  type CollectDefinitionOptions,
  type DefinitionBaseline,
  type DefinitionSampleInput,
} from "./definition.js";

export {
  DiagnosticsAccumulator,
  classifyDiagnosticsSample,
  collectDiagnostics,
  type CollectDiagnosticsOptions,
  type DiagnosticsBaseline,
  type DiagnosticsOracle,
  type DiagnosticsSampleInput,
} from "./diagnostics.js";

export {
  applyResolvedCompletion,
  applyTextEdits,
  collectAutoImport,
  completionItemEdits,
  findCompletionItem,
  parseImportDeclarations,
  verifyAutoImport,
  type AutoImportInput,
  type CollectAutoImportOptions,
  type ExpectedImport,
  type ParsedImportDeclaration,
} from "./autoImport.js";

export {
  classifyChurn,
  collectChurn,
  steadyStateCompileDelta,
  type ChurnInput,
  type ChurnMode,
  type ChurnPreconditions,
  type CollectChurnOptions,
  type StaticChurnPreconditions,
} from "./churn.js";

export {
  classifyLatency,
  collectLatency,
  monotonicNow,
  percentile,
  summarizeLatency,
  type CollectLatencyOptions,
  type LatencyInput,
  type LatencySummary,
} from "./latency.js";

export {
  classifyLogs,
  collectLogs,
  logLevel,
  parseMappingFailure,
  scanLogLines,
  splitStderr,
  type CollectLogsOptions,
  type LogLevel,
  type LogObservation,
  type LogsInput,
  type MappingFailure,
  type SemanticFailureKey,
} from "./logs.js";

export {
  collectRecovery,
  decideRecovery,
  type CollectRecoveryOptions,
  type CorrelatedSignal,
  type ProbeSnapshot,
  type RecoveryInput,
} from "./recovery.js";
