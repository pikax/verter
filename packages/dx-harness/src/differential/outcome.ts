/**
 * The differential's result vocabulary.
 *
 * Every comparison the engine performs yields DATA, never a thrown failure: an
 * agreement, a classified divergence, a baseline-vs-baseline disagreement, a
 * map-absent / stale refusal, or a skip. Each outcome carries the probe identity
 * envelope (id, method, mapping policy, declared AND structurally-clamped
 * confidence, dimension) plus the provider it concerns, so the report layer can
 * prioritize without re-deriving any of it.
 *
 * A comparator produces a flat list of {@link Divergence} findings; {@link
 * foldOutcome} folds an empty list into an agreement and a non-empty list into a
 * single divergence whose primary {@link DivergenceClass} is the highest-priority
 * finding — with NO finding dropped (all are retained on `findings`).
 */

import type { ProviderName } from "../baseline/bridgeClient.js";
import type {
  Confidence,
  MappingPolicy,
  Probe,
  ProbeMethod,
  SemanticDimension,
} from "../scenario/index.js";
import { effectiveConfidence } from "../scenario/index.js";

// ── divergence vocabulary ────────────────────────────────────────────────────

/**
 * A classified way two normalized results disagree. The classes span all four
 * comparators: completion (`missingLabel` / `extraLabel` / `wrongKind` /
 * `insertEditShape` / `noSuggestionsCollapse`), hover (`typeLabelMismatch` /
 * `missingSnippet` / `hoverPresenceMismatch`), definition (`wrongTarget` /
 * `unmappedGenerated`), diagnostics (`defaultRange` / `severityMismatch`), the shared
 * set/range classes (`verterOnly` / `baselineOnly` / `rangeMismatch`), and the
 * baseline-side `rankingMismatch` (carried only by a {@link RankingSignalOutcome},
 * never folded into a verter divergence).
 */
export type DivergenceClass =
  | "noSuggestionsCollapse"
  | "verterOnly"
  | "baselineOnly"
  | "missingLabel"
  | "extraLabel"
  | "unmappedGenerated"
  | "wrongTarget"
  | "defaultRange"
  | "rangeMismatch"
  | "severityMismatch"
  | "typeLabelMismatch"
  | "hoverPresenceMismatch"
  | "missingSnippet"
  | "wrongKind"
  | "insertEditShape"
  | "rankingMismatch";

/** One classified disagreement, with the disagreeing values for the report. */
export interface Divergence {
  readonly class: DivergenceClass;
  readonly detail: string;
  readonly verterValue?: unknown;
  readonly baselineValue?: unknown;
}

/**
 * Severity order over {@link DivergenceClass} — a lower number is more severe and
 * wins the primary slot when one comparison surfaces several findings. The
 * DX-critical No-Suggestions collapse leads; presence/coverage gaps
 * (`verterOnly`/`baselineOnly`/`missingLabel`) outrank a verter-only `extraLabel` and
 * target/range/severity mismatches, which outrank the secondary kind/edit signals.
 * `rankingMismatch` is last — it is baseline-side and never the primary of a verter
 * divergence.
 */
export const DIVERGENCE_PRIORITY: Record<DivergenceClass, number> = {
  noSuggestionsCollapse: 0,
  verterOnly: 1,
  baselineOnly: 2,
  missingLabel: 3,
  extraLabel: 4,
  unmappedGenerated: 5,
  wrongTarget: 6,
  defaultRange: 7,
  rangeMismatch: 8,
  severityMismatch: 9,
  typeLabelMismatch: 10,
  hoverPresenceMismatch: 11,
  missingSnippet: 12,
  wrongKind: 13,
  insertEditShape: 14,
  rankingMismatch: 15,
};

// ── probe identity envelope ──────────────────────────────────────────────────

/** The probe fields the outcome envelope reads. */
export type ProbeLike = Pick<
  Probe,
  "id" | "method" | "mappingPolicy" | "confidence" | "dimension" | "requiresSourceMap"
>;

/** The probe-identity envelope every outcome carries. */
export interface ProbeIdentity {
  readonly probeId: string;
  readonly method: ProbeMethod;
  readonly mappingPolicy: MappingPolicy;
  /** The probe's declared confidence. */
  readonly confidence: Confidence;
  /** Declared confidence clamped down to what the mapping policy structurally permits. */
  readonly effectiveConfidence: Confidence;
  readonly dimension: SemanticDimension;
  /** Whether this probe needs verter's emitted source map (governs the map-absent path). */
  readonly requiresSourceMap: boolean;
}

/** Build the identity envelope, deriving {@link ProbeIdentity.effectiveConfidence}. */
export function probeIdentity(probe: ProbeLike): ProbeIdentity {
  return {
    probeId: probe.id,
    method: probe.method,
    mappingPolicy: probe.mappingPolicy,
    confidence: probe.confidence,
    effectiveConfidence: effectiveConfidence(probe),
    dimension: probe.dimension,
    requiresSourceMap: probe.requiresSourceMap,
  };
}

// ── outcome union ────────────────────────────────────────────────────────────

/** Verter and the baseline provider agreed for this probe. */
export interface AgreementOutcome {
  readonly kind: "agreement";
  readonly probe: ProbeIdentity;
  readonly provider: ProviderName;
  readonly detail?: string;
}

/** Verter and the baseline provider disagreed; `class` is the primary divergence. */
export interface DivergenceOutcome {
  readonly kind: "divergence";
  readonly probe: ProbeIdentity;
  readonly provider: ProviderName;
  readonly class: DivergenceClass;
  readonly detail: string;
  readonly verterValue?: unknown;
  readonly baselineValue?: unknown;
  /** Every finding, primary first; nothing is dropped. */
  readonly findings: readonly Divergence[];
}

/**
 * The two baseline providers disagreed WITH EACH OTHER and no provider was named
 * authoritative — verter is neither compared nor failed; this is first-class.
 */
export interface BaselineDisagreementOutcome {
  readonly kind: "baselineDisagreement";
  readonly probe: ProbeIdentity;
  readonly providers: readonly ProviderName[];
  readonly detail: string;
  readonly findings: readonly Divergence[];
}

/**
 * The baseline provider's observed completion ranking did not match the
 * scenario-asserted order. This is a BASELINE-SIDE signal: verter's own emission order
 * is normalized away upstream and is not compared here, so it carries no verter
 * divergence. `provider` names the baseline whose order was inspected.
 */
export interface RankingSignalOutcome {
  readonly kind: "rankingSignal";
  readonly probe: ProbeIdentity;
  readonly provider: ProviderName;
  readonly detail: string;
  readonly findings: readonly Divergence[];
}

/** A `requiresSourceMap` probe had no source map — recorded, never a verter failure. */
export interface MapAbsentOutcome {
  readonly kind: "mapAbsent";
  readonly probe: ProbeIdentity;
  readonly provider?: ProviderName;
  readonly detail: string;
  readonly requestedVersion?: number;
}

/** The baseline artifact for the probe's version was absent or older — its own outcome. */
export interface BaselineArtifactStaleOutcome {
  readonly kind: "baselineArtifactStale";
  readonly probe: ProbeIdentity;
  readonly provider?: ProviderName;
  readonly detail: string;
  readonly requestedVersion?: number;
  readonly haveVersion?: number;
}

/** The comparison did not run (e.g. no baseline provider, a non-map provider refusal). */
export interface SkippedOutcome {
  readonly kind: "skipped";
  readonly probe: ProbeIdentity;
  readonly provider?: ProviderName;
  readonly reason: string;
}

/** The closed differential-outcome union. */
export type DifferentialOutcome =
  | AgreementOutcome
  | DivergenceOutcome
  | BaselineDisagreementOutcome
  | RankingSignalOutcome
  | MapAbsentOutcome
  | BaselineArtifactStaleOutcome
  | SkippedOutcome;

// ── builders ─────────────────────────────────────────────────────────────────

/** An explicit agreement outcome. */
export function agreement(
  probe: ProbeLike,
  provider: ProviderName,
  detail?: string,
): AgreementOutcome {
  const out: AgreementOutcome = { kind: "agreement", probe: probeIdentity(probe), provider };
  if (detail !== undefined) return { ...out, detail };
  return out;
}

/**
 * Fold a comparator's findings into one outcome: empty → agreement; otherwise a
 * single divergence whose primary class is the highest-priority finding, with
 * every finding retained on `findings`.
 */
export function foldOutcome(
  probe: ProbeLike,
  provider: ProviderName,
  findings: readonly Divergence[],
): AgreementOutcome | DivergenceOutcome {
  const id = probeIdentity(probe);
  if (findings.length === 0) return { kind: "agreement", probe: id, provider };
  const sorted = [...findings].sort(
    (a, b) => DIVERGENCE_PRIORITY[a.class] - DIVERGENCE_PRIORITY[b.class],
  );
  const primary = sorted[0];
  return {
    kind: "divergence",
    probe: id,
    provider,
    class: primary.class,
    detail: primary.detail,
    findings: sorted,
    ...(primary.verterValue !== undefined ? { verterValue: primary.verterValue } : {}),
    ...(primary.baselineValue !== undefined ? { baselineValue: primary.baselineValue } : {}),
  };
}

/** Inputs to {@link mapAbsent}. */
export interface MapAbsentInput {
  readonly detail: string;
  readonly requestedVersion?: number;
}

/** A map-absent outcome for a `requiresSourceMap` probe with no source map. */
export function mapAbsent(
  probe: ProbeLike,
  provider: ProviderName | undefined,
  input: MapAbsentInput,
): MapAbsentOutcome {
  return {
    kind: "mapAbsent",
    probe: probeIdentity(probe),
    detail: input.detail,
    ...(provider !== undefined ? { provider } : {}),
    ...(input.requestedVersion !== undefined ? { requestedVersion: input.requestedVersion } : {}),
  };
}

/** Inputs to {@link baselineArtifactStale}. */
export interface BaselineArtifactStaleInput {
  readonly detail: string;
  readonly requestedVersion?: number;
  readonly haveVersion?: number;
}

/** A stale-baseline outcome for a probe whose artifact version is absent or older. */
export function baselineArtifactStale(
  probe: ProbeLike,
  provider: ProviderName | undefined,
  input: BaselineArtifactStaleInput,
): BaselineArtifactStaleOutcome {
  return {
    kind: "baselineArtifactStale",
    probe: probeIdentity(probe),
    detail: input.detail,
    ...(provider !== undefined ? { provider } : {}),
    ...(input.requestedVersion !== undefined ? { requestedVersion: input.requestedVersion } : {}),
    ...(input.haveVersion !== undefined ? { haveVersion: input.haveVersion } : {}),
  };
}

/** A baseline-disagreement outcome; `detail` defaults to a finding summary. */
export function baselineDisagreement(
  probe: ProbeLike,
  providers: readonly ProviderName[],
  findings: readonly Divergence[],
  detail?: string,
): BaselineDisagreementOutcome {
  return {
    kind: "baselineDisagreement",
    probe: probeIdentity(probe),
    providers,
    findings,
    detail: detail ?? findings.map((f) => `${f.class}: ${f.detail}`).join("; "),
  };
}

/**
 * A ranking-signal outcome: the baseline provider's order did not match the asserted
 * ranking. Routed here rather than into a verter divergence — verter's own order is
 * unavailable (normalized away upstream), so this is never attributed to verter.
 */
export function rankingSignal(
  probe: ProbeLike,
  provider: ProviderName,
  findings: readonly Divergence[],
  detail?: string,
): RankingSignalOutcome {
  return {
    kind: "rankingSignal",
    probe: probeIdentity(probe),
    provider,
    findings,
    detail: detail ?? findings.map((f) => `${f.class}: ${f.detail}`).join("; "),
  };
}

/** Inputs to {@link skipped}. */
export interface SkippedInput {
  readonly reason: string;
  readonly provider?: ProviderName;
}

/** A skipped outcome — the comparison did not run. */
export function skipped(probe: ProbeLike, input: SkippedInput): SkippedOutcome {
  return {
    kind: "skipped",
    probe: probeIdentity(probe),
    reason: input.reason,
    ...(input.provider !== undefined ? { provider: input.provider } : {}),
  };
}
