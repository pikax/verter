/**
 * The artifact-parity differential: the comparison engine that classifies
 * agreement vs divergence between verter's normalized LSP output and the
 * `verter_dx_baseline` provider's normalized output for the same probe on the
 * same emitted TSX.
 *
 * It is pure logic plus a coordinate/source-map projection layer — no LSP
 * spawning. The four comparators ({@link compareCompletion} / {@link
 * compareHover} / {@link compareDefinition} / {@link compareDiagnostics}) each
 * fold to a flat {@link Divergence} list; the {@link classifyProbe} orchestrator
 * selects providers, handles baseline disagreement, and surfaces map-absent /
 * stale refusals as their own outcomes rather than verter failures.
 */

export {
  DIVERGENCE_PRIORITY,
  agreement,
  baselineArtifactStale,
  baselineDisagreement,
  foldOutcome,
  mapAbsent,
  probeIdentity,
  rankingSignal,
  skipped,
  type AgreementOutcome,
  type BaselineArtifactStaleInput,
  type BaselineArtifactStaleOutcome,
  type BaselineDisagreementOutcome,
  type DifferentialOutcome,
  type Divergence,
  type DivergenceClass,
  type DivergenceOutcome,
  type MapAbsentInput,
  type MapAbsentOutcome,
  type ProbeIdentity,
  type ProbeLike,
  type RankingSignalOutcome,
  type SkippedInput,
  type SkippedOutcome,
} from "./outcome.js";

export {
  baselineComparable,
  compareBaselineRanking,
  compareCompletion,
  completionFieldDivergences,
  verterComparable,
  type BaselineCompletion,
  type ComparableCompletionItem,
  type CompletionCompareOptions,
} from "./completion.js";

export { compareHover, stripUnstableDocs, type HoverCompareOptions } from "./hover.js";

export {
  compareDefinition,
  type BaselineLocations,
  type DefinitionCompareOptions,
} from "./definition.js";

export {
  compareDiagnostics,
  diagnosticIdentityKey,
  severityCategory,
  type DiagnosticsCompareOptions,
} from "./diagnostics.js";

export {
  MapAbsentGateError,
  assertKnownGoodSourceMap,
  classifyProbe,
  completionDisagrees,
  definitionDisagrees,
  diagnosticsDisagrees,
  hoverDisagrees,
  type ClassifyProbeInput,
  type ProviderInputs,
  type ProviderResult,
} from "./disagreement.js";

export {
  GeneratedDocument,
  baselineByteToPosition,
  baselineRangeToPosition,
  decodeVlqMappings,
  parseSourceMap,
  projectGeneratedPosition,
  projectGeneratedRange,
  type GeneratedProjection,
  type MappingSegment,
  type OriginalPosition,
  type OriginalRange,
  type ParsedSourceMap,
  type SegmentSource,
} from "./projection.js";
