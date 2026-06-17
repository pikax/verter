/**
 * The Verter DX scenario MODEL — the authored, serializable description of one
 * editing-DX scenario the harness drives.
 *
 * A scenario is DATA: it is authored (as YAML/JSON), validated at the trust
 * boundary ({@link ./validate}), then consumed by the differential, oracle, and
 * collector layers. Every enumeration here is a string-literal union backed by an
 * `as const` array plus an exported type guard, so the same closed set drives the
 * compile-time type, the runtime validator, and consumer narrowing.
 *
 * Driver capability metadata is FIRST-CLASS: the relationship between a probe's
 * {@link MappingPolicy} and its {@link Confidence} is encoded as the structural
 * helpers {@link mappingPolicyConfidenceCeiling} / {@link effectiveConfidence}, not
 * a free-text annotation — a `nearestTokenLowConfidence` probe is STRUCTURALLY
 * low-confidence. Likewise the map-absent relationship is
 * {@link mappingPolicyRequiresSourceMap} and the operational-method dimension
 * constraint is {@link methodSupportsDimension}.
 *
 * Importing this module does no I/O and mutates no globals.
 */

// ── probe method ─────────────────────────────────────────────────────────────

/**
 * The probe query methods: the four semantic queries
 * (`completion`/`hover`/`definition`/`diagnostics`), the editor actions
 * (`codeAction`/`autoImport`), and the operational signals
 * (`churn`/`latency`/`log`/`recovery`).
 */
export const PROBE_METHODS = [
  "completion",
  "hover",
  "definition",
  "diagnostics",
  "codeAction",
  "autoImport",
  "churn",
  "latency",
  "log",
  "recovery",
] as const;
export type ProbeMethod = (typeof PROBE_METHODS)[number];
export function isProbeMethod(value: unknown): value is ProbeMethod {
  return typeof value === "string" && (PROBE_METHODS as readonly string[]).includes(value);
}

/**
 * The operational signal methods — measured/observed, not type-query results.
 * They carry a host compile-counter delta (`churn`), request latency (`latency`),
 * server log lines (`log`), or post-burst recovery (`recovery`). They have no
 * curated-`.ts`-oracle or direct-Vue-surface semantic assertion, so they cannot be
 * {@link SemanticDimension} `vueSemanticValidity`.
 */
export const OPERATIONAL_METHODS = ["churn", "latency", "log", "recovery"] as const;
export type OperationalMethod = (typeof OPERATIONAL_METHODS)[number];
export function isOperationalMethod(method: ProbeMethod): method is OperationalMethod {
  return (OPERATIONAL_METHODS as readonly string[]).includes(method);
}

// ── mapping policy ───────────────────────────────────────────────────────────

/**
 * How a probe maps an authored position into the emitted-TSX artifact:
 *  - `strict` — exact source-map mapping;
 *  - `memberBoundaryFallback` — fell back to the enclosing member boundary;
 *  - `nearestTokenLowConfidence` — nearest-token mapping, STRUCTURALLY low-confidence;
 *  - `none` — a direct Vue-surface probe that does not map through the artifact.
 */
export const MAPPING_POLICIES = [
  "strict",
  "memberBoundaryFallback",
  "nearestTokenLowConfidence",
  "none",
] as const;
export type MappingPolicy = (typeof MAPPING_POLICIES)[number];
export function isMappingPolicy(value: unknown): value is MappingPolicy {
  return typeof value === "string" && (MAPPING_POLICIES as readonly string[]).includes(value);
}

// ── confidence ───────────────────────────────────────────────────────────────

export const CONFIDENCE_LEVELS = ["high", "medium", "low"] as const;
export type Confidence = (typeof CONFIDENCE_LEVELS)[number];
export function isConfidence(value: unknown): value is Confidence {
  return typeof value === "string" && (CONFIDENCE_LEVELS as readonly string[]).includes(value);
}

// ── semantic dimension ───────────────────────────────────────────────────────

/**
 * The epistemic dimension a probe asserts on:
 *  - `artifactParity` — emitted-`.vue.tsx` mapping/projection parity vs the TS
 *    baseline (and verter-internal operational signals);
 *  - `vueSemanticValidity` — Vue semantic truth via curated `.ts` oracle, Volar,
 *    or direct Vue-surface invariants.
 */
export const SEMANTIC_DIMENSIONS = ["artifactParity", "vueSemanticValidity"] as const;
export type SemanticDimension = (typeof SEMANTIC_DIMENSIONS)[number];
export function isSemanticDimension(value: unknown): value is SemanticDimension {
  return typeof value === "string" && (SEMANTIC_DIMENSIONS as readonly string[]).includes(value);
}

// ── required drivers ─────────────────────────────────────────────────────────

/** The drivers/providers a probe requires, as stable camelCase identifiers. */
export const REQUIRED_DRIVERS = [
  "rawLsp",
  "extensionHost",
  "rustComplement",
  "tsgo",
  "tsserver",
  "volar",
] as const;
/**
 * A driver/provider identifier. Each canonical id maps to a human-facing label,
 * held as first-class data in {@link REQUIRED_DRIVER_LABELS}: `rawLsp` → "raw-LSP",
 * `extensionHost` → "extension-host", `rustComplement` → "Rust complement",
 * `tsgo` → "tsgo", `tsserver` → "tsserver", `volar` → "Volar".
 */
export type RequiredDriver = (typeof REQUIRED_DRIVERS)[number];
export function isRequiredDriver(value: unknown): value is RequiredDriver {
  return typeof value === "string" && (REQUIRED_DRIVERS as readonly string[]).includes(value);
}

/** The human-facing label for each stable {@link RequiredDriver} identifier. */
export const REQUIRED_DRIVER_LABELS: Record<RequiredDriver, string> = {
  rawLsp: "raw-LSP",
  extensionHost: "extension-host",
  rustComplement: "Rust complement",
  tsgo: "tsgo",
  tsserver: "tsserver",
  volar: "Volar",
};

// ── capability requirements (extensible) ─────────────────────────────────────

/**
 * The known driver capability requirements. The union is EXTENSIBLE: a
 * forward-compatible capability string is admitted ({@link CapabilityRequirement}),
 * so the validator accepts an unknown non-empty capability rather than rejecting it.
 */
export const KNOWN_CAPABILITY_REQUIREMENTS = [
  "acceptPath",
  "debugLogs",
  "diagnosticsPush",
  "positionEncoding",
] as const;
export type KnownCapabilityRequirement = (typeof KNOWN_CAPABILITY_REQUIREMENTS)[number];
/** A known capability (with autocomplete) OR a forward-compatible custom string. */
export type CapabilityRequirement = KnownCapabilityRequirement | (string & {});
export function isKnownCapabilityRequirement(value: unknown): value is KnownCapabilityRequirement {
  return (
    typeof value === "string" &&
    (KNOWN_CAPABILITY_REQUIREMENTS as readonly string[]).includes(value)
  );
}

// ── baseline requirement ─────────────────────────────────────────────────────

/** How strictly a baseline provider is required for a scenario. */
export const BASELINE_REQUIREMENTS = ["required", "requiredForCi", "optional", "disabled"] as const;
export type BaselineRequirement = (typeof BASELINE_REQUIREMENTS)[number];
export function isBaselineRequirement(value: unknown): value is BaselineRequirement {
  return typeof value === "string" && (BASELINE_REQUIREMENTS as readonly string[]).includes(value);
}

// ── edit-step kind ───────────────────────────────────────────────────────────

/** The edit operations a {@link Scenario} script/setup step performs. */
export const EDIT_STEP_KINDS = ["insert", "replace", "delete"] as const;
export type EditStepKind = (typeof EDIT_STEP_KINDS)[number];
export function isEditStepKind(value: unknown): value is EditStepKind {
  return typeof value === "string" && (EDIT_STEP_KINDS as readonly string[]).includes(value);
}

// ── invariant assertion ──────────────────────────────────────────────────────

/** The assertion an {@link Invariant} makes about a verter surface string. */
export const INVARIANT_ASSERTIONS = ["contains", "excludes", "equals"] as const;
export type InvariantAssertion = (typeof INVARIANT_ASSERTIONS)[number];
export function isInvariantAssertion(value: unknown): value is InvariantAssertion {
  return typeof value === "string" && (INVARIANT_ASSERTIONS as readonly string[]).includes(value);
}

// ── structural confidence relationship ───────────────────────────────────────

/** Total order over {@link Confidence}: `low` < `medium` < `high`. */
export const CONFIDENCE_RANK: Record<Confidence, number> = { low: 0, medium: 1, high: 2 };

/**
 * The highest {@link Confidence} a {@link MappingPolicy} structurally permits. A
 * `nearestTokenLowConfidence` probe is STRUCTURALLY low; a member-boundary
 * fallback is at most medium; a strict mapping (or a direct `none` Vue-surface
 * probe, whose confidence is not bounded by mapping precision) permits high.
 */
export const MAPPING_POLICY_CONFIDENCE_CEILING: Record<MappingPolicy, Confidence> = {
  strict: "high",
  memberBoundaryFallback: "medium",
  nearestTokenLowConfidence: "low",
  none: "high",
};

/** The {@link Confidence} ceiling {@link MappingPolicy} `policy` structurally permits. */
export function mappingPolicyConfidenceCeiling(policy: MappingPolicy): Confidence {
  return MAPPING_POLICY_CONFIDENCE_CEILING[policy];
}

/** Whether `confidence` is at or below the ceiling `policy` structurally permits. */
export function confidenceWithinCeiling(confidence: Confidence, policy: MappingPolicy): boolean {
  return CONFIDENCE_RANK[confidence] <= CONFIDENCE_RANK[mappingPolicyConfidenceCeiling(policy)];
}

/**
 * The DERIVED effective confidence of a probe: its declared confidence clamped
 * DOWN to the ceiling its mapping policy structurally permits. A high-confidence
 * claim on a `nearestTokenLowConfidence` probe yields `low`.
 */
export function effectiveConfidence(
  probe: Pick<Probe, "confidence" | "mappingPolicy">,
): Confidence {
  const ceiling = mappingPolicyConfidenceCeiling(probe.mappingPolicy);
  return CONFIDENCE_RANK[probe.confidence] <= CONFIDENCE_RANK[ceiling] ? probe.confidence : ceiling;
}

// ── requiresSourceMap relationship ───────────────────────────────────────────

/**
 * Whether a {@link MappingPolicy} structurally needs a source map. Every mapping
 * policy maps through the emitted artifact and so requires the map; a `none`
 * direct Vue-surface probe continues without one. This is the biconditional the
 * validator enforces: `requiresSourceMap === (policy !== "none")`.
 */
export function mappingPolicyRequiresSourceMap(policy: MappingPolicy): boolean {
  return policy !== "none";
}

// ── method↔dimension relationship ────────────────────────────────────────────

/**
 * Whether `method` can structurally carry `dimension`. The operational signal
 * methods ({@link OPERATIONAL_METHODS}) cannot be `vueSemanticValidity`: a
 * compile-counter delta, a latency measurement, a log line, or a recovery check
 * carries no curated-oracle / Vue-surface semantic assertion. Every method can be
 * `artifactParity`.
 */
export function methodSupportsDimension(
  method: ProbeMethod,
  dimension: SemanticDimension,
): boolean {
  if (dimension === "vueSemanticValidity" && isOperationalMethod(method)) return false;
  return true;
}

// ── method↔invariant relationship ────────────────────────────────────────────

/**
 * Whether `method` can back an {@link Invariant}. An invariant is a direct
 * Vue-surface assertion, so it requires a method that produces a surface string
 * to assert `contains`/`excludes`/`equals` against — the semantic-query methods
 * (`completion`/`hover`/`definition`/`diagnostics`/`codeAction`/`autoImport`).
 * The operational signals ({@link OPERATIONAL_METHODS}) carry a measurement, not a
 * surface, so they cannot back an invariant. This mirrors {@link methodSupportsDimension}.
 */
export function methodSupportsInvariant(method: ProbeMethod): boolean {
  return !isOperationalMethod(method);
}

// ── DTOs ─────────────────────────────────────────────────────────────────────

/** One query definition within a {@link Scenario}. */
export interface Probe {
  /** Stable, scenario-unique probe id. */
  readonly id: string;
  readonly method: ProbeMethod;
  /** A named source anchor declared in {@link Scenario.anchors}. */
  readonly anchor: string;
  readonly mappingPolicy: MappingPolicy;
  readonly confidence: Confidence;
  readonly dimension: SemanticDimension;
  /** Whether this probe needs verter's emitted source map. */
  readonly requiresSourceMap: boolean;
  readonly requiredDrivers: readonly RequiredDriver[];
  readonly capabilityRequirements: readonly CapabilityRequirement[];
}

/** Insert `text` at `anchor`; `burst` types it one character at a time. */
export interface InsertStep {
  readonly kind: "insert";
  readonly anchor: string;
  readonly text: string;
  /** Per-character burst: one `didChange` per inserted character. */
  readonly burst?: boolean;
}

/** Remove `removeUnits` UTF-16 units at `anchor` then insert `text`. */
export interface ReplaceStep {
  readonly kind: "replace";
  readonly anchor: string;
  readonly text: string;
  readonly removeUnits: number;
  readonly burst?: boolean;
}

/** Remove `removeUnits` UTF-16 units at `anchor`. */
export interface DeleteStep {
  readonly kind: "delete";
  readonly anchor: string;
  readonly removeUnits: number;
}

/** One edit applied to a driver-local document buffer (NOT the materialized fixture). */
export type EditStep = InsertStep | ReplaceStep | DeleteStep;

/**
 * A direct Vue-surface assertion: at `anchor`, the `method` surface of verter's
 * output must `contains`/`excludes`/`equals` `value` (e.g. a hover label
 * `contains` `@click` and `excludes` `onClick`).
 */
export interface Invariant {
  readonly id: string;
  readonly anchor: string;
  readonly method: ProbeMethod;
  readonly assertion: InvariantAssertion;
  readonly value: string;
  readonly description?: string;
}

/** Per-provider baseline requirement. */
export interface ScenarioBaselines {
  readonly tsgo: BaselineRequirement;
  readonly tsserver: BaselineRequirement;
  readonly volar: BaselineRequirement;
}

/** The default requirement levels: tsgo required, tsserver CI-required, Volar optional. */
export const DEFAULT_BASELINES: ScenarioBaselines = {
  tsgo: "required",
  tsserver: "requiredForCi",
  volar: "optional",
};

/** Latency ceilings in milliseconds. */
export interface LatencyThresholds {
  readonly p50Ms?: number;
  readonly p95Ms?: number;
  readonly p99Ms?: number;
}

/** Recovery-after-burst thresholds. */
export interface RecoveryThresholds {
  readonly maxRecoveryMs?: number;
  readonly stableIntervals?: number;
}

/** Scenario thresholds: latency, steady-state compile delta, recovery, flake windows. */
export interface ScenarioThresholds {
  readonly latency?: LatencyThresholds;
  /** Max host compile-counter delta tolerated per quiesced steady-state edit. */
  readonly steadyStateCompileDelta?: number;
  readonly recovery?: RecoveryThresholds;
  /** Allowed flake windows before a probe is treated as flaky. */
  readonly flakeWindows?: number;
}

/** The authored scenario model. Immutable config — every field is `readonly`. */
export interface Scenario {
  readonly id: string;
  readonly fixture: string;
  /** The primary `.vue` entry file. */
  readonly entryFile: string;
  /** The named source anchors this scenario declares; probes/edits/invariants reference these. */
  readonly anchors: readonly string[];
  /** Optional edits applied before measurement. */
  readonly setup?: readonly EditStep[];
  /** The measured edit script, including per-character burst steps. */
  readonly script: readonly EditStep[];
  readonly probes: readonly Probe[];
  readonly invariants: readonly Invariant[];
  readonly baselines: ScenarioBaselines;
  readonly thresholds: ScenarioThresholds;
}
