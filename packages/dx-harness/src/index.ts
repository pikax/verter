/**
 * `@verter/dx-harness` — the immutable {@link MaterializedWorkspace} scaffold plus
 * the TypeScript orchestration that drives the `verter_dx_baseline` differential
 * baseline.
 *
 * The dx-harness scaffold owns the Verter-facing layout (fixture copy + test-anchor
 * strip, deterministic tool roots, the committed vendored Vue shims, workspace
 * settings) and the bridge/materialize clients; the `crates/verter_dx_baseline` bridge
 * owns ALL baseline materialization. The scaffold CALLS the baseline bridge and reads
 * its emitted source maps as authoritative — it never duplicates the bridge's
 * compilation, twin generation, specifier rewriting, or source-map shifting.
 */

export { canonicalizePath, joinCanonical, offsetToLineChar, type LineChar } from "./paths.js";

// The local-analysis-input config loader + the `DX_HARNESS_EXTERNAL_CORPUS` hook.
// Mirrors the Rust `verter_analysis_inputs` `verter.analysis-projects.v1` schema.
export {
  ANALYSIS_CORPUS_ENV,
  ANALYSIS_PROJECTS_SCHEMA,
  AnalysisConfigError,
  loadAnalysisConfig,
  parseAnalysisConfig,
  resolveCorpusSource,
  type AnalysisProject,
  type AnalysisProjects,
  type CorpusSource,
  type ProjectKind,
  type Workstream,
} from "./analysisConfig.js";

// The single producer-side redactor + the typed redacted-emitter wrappers.
export {
  Redactor,
  redactSourceMap,
  serializeRedactedJson,
  serializeRedactedJsonl,
} from "./redaction.js";

export {
  AnchorError,
  addFileAnchors,
  requireAnchor,
  stripAnchors,
  type Anchor,
  type AnchorEncoding,
  type AnchorMap,
  type AnchorPosition,
  type StripResult,
} from "./anchors.js";

export {
  readTypescriptVersionFromDisk,
  resolveToolRoots,
  type ResolveToolRootsOptions,
  type ToolRoots,
} from "./toolRoots.js";

export {
  DX_HARNESS_WORKSPACE_ENV,
  writeWorkspaceSettings,
  type WorkspaceSettings,
  type WriteWorkspaceSettingsOptions,
} from "./workspaceSettings.js";

export {
  VENDORED_VUE_VERSION,
  buildVendorManifest,
  collectVuePackageVersions,
  computeExpectedVueVersion,
  sha256Hex,
  vendorShimsDir,
  type VendorFile,
  type VendorManifest,
  type VuePackageVersion,
} from "./vendorManifest.js";

export { runOneShot, type OneShotOptions, type OneShotResult } from "./baseline/childProcess.js";

export {
  buildMaterializeRequest,
  parseMaterializeResult,
  runMaterialize,
  type MaterializeArtifact,
  type MaterializeCompileError,
  type MaterializeRequestInput,
  type MaterializeResult,
  type MaterializeWireRequest,
  type RunMaterializeOptions,
  type VueVersionWarning,
} from "./baseline/materializeClient.js";

export {
  BridgeClient,
  NewlineFramer,
  decodeResponse,
  diagnosticsFrame,
  encodeRequest,
  helloFrame,
  openFrame,
  queryFrame,
  shutdownFrame,
  syncArtifactsFrame,
  type AppliedSync,
  type BaselineFile,
  type BridgeClientOptions,
  type BridgeRequest,
  type BridgeResponse,
  type ChangedTwin,
  type DiagnosticsInput,
  type DiagnosticsRequest,
  type DiagnosticsResponse,
  type ErrorKind,
  type ErrorResponse,
  type FileRole,
  type HelloRequest,
  type HelloResponse,
  type NormalizedCompletionItem,
  type NormalizedDiagnostic,
  type NormalizedHover,
  type NormalizedLocation,
  type OpenRequest,
  type OpenResponse,
  type ProviderCapabilities,
  type ProviderName,
  type QueryInput,
  type QueryMethod,
  type QueryRequest,
  type QueryResponse,
  type QueryResult,
  type ShutdownRequest,
  type ShutdownResponse,
  type SyncAction,
  type SyncArtifactsInput,
  type SyncArtifactsRequest,
  type SyncArtifactsResponse,
  type ToolRootWire,
} from "./baseline/bridgeClient.js";

export {
  createMaterializedWorkspace,
  disposeMaterializedWorkspace,
  type CreateMaterializedWorkspaceOptions,
  type MaterializedWorkspace,
  type MaterializeRunner,
  type TsconfigSet,
  type VendorReference,
} from "./materializedWorkspace.js";

export {
  GenerationGate,
  evaluateGenerationGate,
  type GenerationChannel,
  type GenerationEvent,
  type GenerationGateDecision,
} from "./core/generationGate.js";

export {
  QUIESCENCE_COUNTER_KEYS,
  QUIESCENCE_WARN_KEYWORDS,
  REQUIRED_STABLE_INTERVALS,
  countersEqual,
  decideQuiescence,
  extractQuiescenceCounters,
  isQuiescenceWarnLine,
  pollUntilQuiesced,
  type PollUntilQuiescedOptions,
  type QuiescenceCounters,
  type QuiescenceDecision,
  type QuiescenceObservation,
  type QuiescenceResult,
} from "./core/quiescence.js";

export {
  ExtensionStartupGate,
  TYPE_PROVIDER_SYNC_COMPLETE_LOG_PATTERN,
  VERTER_READY_LOG_PATTERN,
  parseExtensionStartupLog,
  parseStartupLogLine,
} from "./core/extensionStartup.js";

export {
  GET_STATISTICS_METHOD,
  TYPE_PROVIDER_SYNC_COMPLETE_METHOD,
  VERTER_READY_METHOD,
  awaitRawLspStartup,
  type AwaitRawLspStartupOptions,
  type RawLspStartupResult,
  type StartupLspClient,
} from "./core/startupGate.js";

export {
  RawEditorNeutralLspDriver,
  type RawEditorNeutralLspDriverOptions,
} from "./editor-neutral/rawLspDriver.js";

// The authored scenario model + trust-boundary validator. The sub-barrel is
// curated; re-exported wholesale so external consumers reach the scenario surface
// without a deep import.
export * from "./scenario/index.js";

// The pure LSP-response normalizers. These verter-side `Canonical*` forms are
// DISTINCT from the bridge's byte-offset `Normalized*` wire shapes exported above.
// The normalize barrel intentionally withholds the raw-LSP response-input unions
// (`CompletionResponse`/`HoverResponse`/`DefinitionResponse`/`DiagnosticsResponse`),
// so the only `DiagnosticsResponse` reachable from this root is the bridge's object
// envelope above — the public normalize surface is the `Canonical*` outputs plus the
// `normalize*` functions.
export * from "./normalize/index.js";

// The artifact-parity differential: the comparison engine over verter's
// `Canonical*` forms and the bridge's `Normalized*` outputs, plus the
// coordinate/source-map projection that bridges their two coordinate spaces.
// (Also re-exports the curated semantic-oracle `vueSemanticValidity` diff.)
export * from "./differential/index.js";

// The curated semantic-oracle runner: the `.ts` gold-standard descriptor, the pure
// fact extractors, anchor → byte-offset resolution, and the orchestration that
// drives verter-on-`.vue` against tsgo/tsserver-on-`.ts` for the vueSemanticValidity
// dimension. Consumes the diff re-exported above.
export * from "./semantic-oracle/index.js";

// The raw-LSP signal collectors: the shared event/severity/JSONL substrate, the
// per-edit sampling loop, and the nine collectors (completion, hover, definition,
// auto-import, diagnostics, churn, latency, logs, recovery). Each pure classifier
// folds normalized inputs into severity-tagged events; the live drivers spawn the
// real `verter-lsp` binary and are exercised by the env-gated integration suite. The
// collectors DRIVE the differential comparators / normalizers / quiescence gates —
// they do not re-implement comparison.
export * from "./collectors/index.js";

// The report generator: the terminal stage that consumes the collector event stream,
// the differential outcomes, the scenario model, and the normalizers, and emits the
// run's durable artifacts — the `dx-events.jsonl` stream, the deduped finding set with
// its content-addressed fingerprint and S0–S4 ladder (`dx-summary.json` /
// `DX-FINDINGS.md` / `baseline-manifest.json`), the `junit.xml`, and the
// `BUG-REPORT.md` reconciliation. It owns no comparison/normalization of its own.
export * from "./report/index.js";
