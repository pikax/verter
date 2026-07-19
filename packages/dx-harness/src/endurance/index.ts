/**
 * Endurance/soak harness for the Verter LSP.
 *
 * Drives the REAL `verter-lsp` binary over stdio via the neutral
 * `@verter/lsp-test-client` (no editor dependency) through long-session IDE
 * workloads — keystroke-level component building, heavy edit→query loops,
 * hover/definition storms during typing, and sustained mixed soaks — and
 * asserts stability: provider alive, every request answered or properly
 * cancelled (never silently dropped), bounded latency (incl. a no-degradation
 * trend over time windows), bounded RSS, correct feature answers after load,
 * and a JSON attestation receipt per run proving non-vacuity.
 */
export {
  ENDURANCE_PROVIDER_ROUTES,
  ENDURANCE_LANES,
  DEFAULT_ENDURANCE_LANE,
  type EnduranceConfig,
  type EnduranceFramework,
  type EnduranceLane,
  type EnduranceLanguageMode,
  type EnduranceProviderRoute,
  type EnduranceReceipt,
  type FrameworkReceiptSection,
  type PercentileSummary,
  type RequestClassification,
  type WindowSummary,
  type ProviderRuntimeAttestation,
} from "./types.js";

export { loadEnduranceConfig } from "./config.js";

export {
  classifyRequestError,
  ConcurrencyPool,
  LatencyRecorder,
  parseHandlerExitCostsMs,
  percentileOf,
  RequestTracker,
  sleep,
  summarize,
  TypeQualityRecorder,
  type LatencySample,
  type TypeQualitySnapshot,
} from "./metrics.js";

export { RssSampler, readProcessRssBytes, type RssSample } from "./rss.js";

export {
  ENDURANCE_TSCONFIG,
  buildCarrierSet,
  carrierPath,
  carrierContent,
  childConsumerContent,
  disposeWorkspace,
  heavyUpdateChildContent,
  laneDirectory,
  materializeWorkspace,
  type CarrierSet,
  type WorkspaceFiles,
} from "./workspace.js";

export {
  REPO_ROOT,
  spawnEnduranceLsp,
  type EnduranceLspHandle,
  type SpawnEnduranceLspOptions,
  parseProviderRuntimeAttestation,
} from "./spawn.js";

export {
  camelToKebab,
  completionLabels,
  definitionTargets,
  EnduranceSession,
  hoverText,
  languageIdForPath,
  type CompletionProbe,
  type DefinitionProbe,
  type DefinitionTarget,
  type EnduranceSessionOptions,
  type HoverProbe,
  type OpenedDocument,
  type EnduranceProbe,
  type ProbeOutcome,
  type SettledOutcome,
} from "./session.js";

export {
  buildReceipt,
  convergeProbe,
  FailureBag,
  replaceOnce,
  runCheckpoint,
  typeFromScratch,
  typeInsertion,
  type ScenarioContext,
  type TypingCheckpoint,
} from "./scenarios/common.js";

export { receiptCoreFailures, receiptDestination, writeReceipt } from "./attestation.js";

export {
  BUILD_COMPONENT_FILES,
  buildComponentEventSiteProbes,
  buildComponentIntegrationProbes,
  buildComponentFixture,
  runBuildComponentScenario,
  type BuildComponentFixture,
} from "./scenarios/buildComponent.js";

export {
  HEAVY_UPDATE_FILES,
  heavyUpdateFixture,
  runHeavyUpdateScenario,
  runRenameCycles,
  type HeavyUpdateFixture,
} from "./scenarios/heavyUpdate.js";

export {
  carrierStormProbes,
  runStormScenario,
  STORM_CARRIER_COUNT,
  stormWorkspace,
  type StormParams,
} from "./scenarios/storm.js";

export {
  runSoakScenario,
  SOAK_SCRATCH_PATH,
  SOAK_TYPED_DOC,
  soakProbes,
  soakWorkspace,
  type SoakParams,
  type SoakWorkspace,
} from "./scenarios/soak.js";

export {
  collectCorpusCarrierFiles,
  deriveCorpusProbes,
  type CorpusLaneSection,
  type CorpusProbeDerivation,
} from "./scale.js";
