/**
 * Probe classification: the orchestration that turns one probe's verter output
 * plus per-provider baseline outputs into differential outcomes.
 *
 * Two cross-provider concerns live here. Baseline DISAGREEMENT: when both
 * baseline providers ran and disagree with each other and no provider is named
 * authoritative, the result is a first-class `baselineDisagreement` — verter is
 * neither compared nor failed. When a provider IS named authoritative, verter is
 * compared only against that one. MAP-ABSENT / STALE: a `requiresSourceMap` probe
 * with no source map, or a bridge `compiled_code_map_absent` /
 * `baseline_artifact_stale` refusal, becomes its own recorded outcome, never a
 * verter failure. The hard known-good gate ({@link assertKnownGoodSourceMap}) is
 * the sole place a map-absence throws.
 */

import type {
  ErrorResponse,
  NormalizedDiagnostic,
  NormalizedHover,
  NormalizedLocation,
  ProviderName,
} from "../baseline/bridgeClient.js";
import type { Probe } from "../scenario/index.js";
import {
  baselineComparable,
  completionFieldDivergences,
  type BaselineCompletion,
} from "./completion.js";
import { diagnosticIdentityKey, severityCategory } from "./diagnostics.js";
import { stripUnstableDocs } from "./hover.js";
import {
  baselineArtifactStale,
  baselineDisagreement,
  foldOutcome,
  mapAbsent,
  skipped,
  type DifferentialOutcome,
  type Divergence,
} from "./outcome.js";

/** A provider's baseline output, or its typed refusal. */
export type ProviderResult<B> =
  | { readonly ok: true; readonly output: B }
  | { readonly ok: false; readonly error: ErrorResponse };

/** Per-provider baseline results for one probe. */
export interface ProviderInputs<B> {
  readonly tsgo?: ProviderResult<B>;
  readonly tsserver?: ProviderResult<B>;
}

/** Inputs to {@link classifyProbe}, generic over the per-method baseline output `B`. */
export interface ClassifyProbeInput<B> {
  readonly probe: Probe;
  /** Whether verter produced a source map for this probe's artifact. */
  readonly sourceMapPresent: boolean;
  readonly providers: ProviderInputs<B>;
  /** When set, verter is compared only against this provider (disagreement is ignored). */
  readonly authoritativeProvider?: ProviderName;
  /** Compare verter against one provider's baseline output → flat divergences. */
  readonly compareVerter: (provider: ProviderName, baseline: B) => readonly Divergence[];
  /** Compare the two providers' baseline outputs with each other → flat divergences. */
  readonly baselineDisagree: (tsgo: B, tsserver: B) => readonly Divergence[];
}

/** Providers in deterministic order; `tsgo` is the primary when both agree. */
const PROVIDER_ORDER: readonly ProviderName[] = ["tsgo", "tsserver"];

/** Map a bridge refusal to its outcome — map-absent / stale / a non-fatal skip. */
function errorToOutcome(
  probe: Probe,
  provider: ProviderName,
  error: ErrorResponse,
): DifferentialOutcome {
  if (error.kind === "compiled_code_map_absent") {
    return mapAbsent(probe, provider, {
      detail: error.message,
      ...(error.requestedVersion !== undefined ? { requestedVersion: error.requestedVersion } : {}),
    });
  }
  if (error.kind === "baseline_artifact_stale") {
    return baselineArtifactStale(probe, provider, {
      detail: error.message,
      ...(error.requestedVersion !== undefined ? { requestedVersion: error.requestedVersion } : {}),
      ...(error.haveVersion !== undefined ? { haveVersion: error.haveVersion } : {}),
    });
  }
  return skipped(probe, { provider, reason: `${error.kind}: ${error.message}` });
}

/**
 * Classify one probe into differential outcomes. Returns DATA (one or more
 * outcomes), never throws — the only throwing map-absence path is the hard gate.
 */
export function classifyProbe<B>(input: ClassifyProbeInput<B>): DifferentialOutcome[] {
  const { probe, providers } = input;
  const presentProviders = PROVIDER_ORDER.filter((p) => providers[p] !== undefined);

  // Verter-side map-absent: a probe that needs the map but has none cannot be
  // compared against any provider — record it (per present provider), do not crash.
  if (probe.requiresSourceMap && !input.sourceMapPresent) {
    const detail = `requiresSourceMap probe ${probe.id} has no source map`;
    if (presentProviders.length === 0) return [mapAbsent(probe, undefined, { detail })];
    return presentProviders.map((p) => mapAbsent(probe, p, { detail }));
  }

  // Partition providers into ready outputs and refusals.
  const ready = new Map<ProviderName, B>();
  const refusals = new Map<ProviderName, ErrorResponse>();
  for (const p of PROVIDER_ORDER) {
    const result = providers[p];
    if (result === undefined) continue;
    if (result.ok) ready.set(p, result.output);
    else refusals.set(p, result.error);
  }

  // A named authoritative provider governs: compare only against it.
  if (input.authoritativeProvider !== undefined) {
    const authoritative = input.authoritativeProvider;
    const output = ready.get(authoritative);
    if (output !== undefined) {
      return [foldOutcome(probe, authoritative, input.compareVerter(authoritative, output))];
    }
    const refusal = refusals.get(authoritative);
    if (refusal !== undefined) return [errorToOutcome(probe, authoritative, refusal)];
    return [
      skipped(probe, {
        provider: authoritative,
        reason: `authoritative provider ${authoritative} did not run`,
      }),
    ];
  }

  const outcomes: DifferentialOutcome[] = [];
  for (const p of PROVIDER_ORDER) {
    const refusal = refusals.get(p);
    if (refusal !== undefined) outcomes.push(errorToOutcome(probe, p, refusal));
  }

  const tsgo = ready.get("tsgo");
  const tsserver = ready.get("tsserver");
  if (tsgo !== undefined && tsserver !== undefined) {
    const disagreement = input.baselineDisagree(tsgo, tsserver);
    if (disagreement.length > 0) {
      outcomes.push(baselineDisagreement(probe, ["tsgo", "tsserver"], disagreement));
      return outcomes;
    }
    // Both providers agree — compare verter against the primary.
    outcomes.push(foldOutcome(probe, "tsgo", input.compareVerter("tsgo", tsgo)));
    return outcomes;
  }

  const single = PROVIDER_ORDER.find((p) => ready.has(p));
  if (single !== undefined) {
    outcomes.push(foldOutcome(probe, single, input.compareVerter(single, ready.get(single) as B)));
    return outcomes;
  }

  if (outcomes.length === 0)
    outcomes.push(skipped(probe, { reason: "no baseline provider available" }));
  return outcomes;
}

// ── hard known-good gate ─────────────────────────────────────────────────────

/** The hard-gate failure when a known-good artifact has no source map. */
export class MapAbsentGateError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "MapAbsentGateError";
  }
}

/**
 * The hard known-good gate: a missing source map is a fatal fixture/codegen
 * failure, not a probe outcome — fail immediately. Normal probes use {@link
 * classifyProbe}, which records map-absence as data instead.
 *
 * @throws {MapAbsentGateError} when `sourceMapPresent` is `false`.
 */
export function assertKnownGoodSourceMap(sourceMapPresent: boolean, context: string): void {
  if (!sourceMapPresent) {
    throw new MapAbsentGateError(`known-good gate: source map absent for ${context}`);
  }
}

// ── per-method baseline-vs-baseline disagreement helpers ─────────────────────

function setsEqual(a: ReadonlySet<string>, b: ReadonlySet<string>): boolean {
  if (a.size !== b.size) return false;
  for (const value of a) if (!b.has(value)) return false;
  return true;
}

/**
 * Whether two providers disagree on completions, using the SAME field set as the
 * forward verter-vs-baseline comparator — label set (both directions, order-insensitive)
 * plus per-shared-label kind and insert/edit shape — via the shared
 * {@link completionFieldDivergences}, so the two cannot drift apart.
 */
export function completionDisagrees(a: BaselineCompletion, b: BaselineCompletion): Divergence[] {
  return completionFieldDivergences(
    a.items.map(baselineComparable),
    b.items.map(baselineComparable),
  );
}

/** Whether two providers disagree on the hover type label (docs stripped). */
export function hoverDisagrees(a: NormalizedHover | null, b: NormalizedHover | null): Divergence[] {
  const al = a === null ? null : stripUnstableDocs(a.contents) || null;
  const bl = b === null ? null : stripUnstableDocs(b.contents) || null;
  if (al === bl) return [];
  return [
    {
      class: "typeLabelMismatch",
      detail: "baseline providers differ on the hover type label",
      verterValue: al,
      baselineValue: bl,
    },
  ];
}

/** Whether two providers disagree on the definition location set (native byte offsets). */
export function definitionDisagrees(
  a: readonly NormalizedLocation[],
  b: readonly NormalizedLocation[],
): Divergence[] {
  const key = (l: NormalizedLocation): string => `${l.path}:${l.start}:${l.end}`;
  const ak = new Set(a.map(key));
  const bk = new Set(b.map(key));
  if (setsEqual(ak, bk)) return [];
  return [
    {
      class: "rangeMismatch",
      detail: "baseline providers differ on definition locations",
      verterValue: a,
      baselineValue: b,
    },
  ];
}

/**
 * Whether two providers disagree on diagnostics, using the SAME field set as the
 * forward comparator — code/message identity, severity/category, and byte range (both
 * baselines share the emitted-TSX byte space, so their offsets compare directly). The
 * identity reuses the forward comparator's {@link diagnosticIdentityKey} (which
 * namespaces a code apart from a message and `normalizeEol`s a no-code message) and the
 * shared {@link severityCategory} folds the severity vocabularies, so the baseline-vs-
 * baseline equivalence cannot drift from the verter comparison in either direction.
 */
export function diagnosticsDisagrees(
  a: readonly NormalizedDiagnostic[],
  b: readonly NormalizedDiagnostic[],
): Divergence[] {
  const key = (d: NormalizedDiagnostic): string =>
    `${diagnosticIdentityKey(d)}:${severityCategory(d.severity)}:${d.start}:${d.end}`;
  const ak = new Set(a.map(key));
  const bk = new Set(b.map(key));
  if (setsEqual(ak, bk)) return [];
  return [
    {
      class: "baselineOnly",
      detail: "baseline providers differ on diagnostics",
      verterValue: a,
      baselineValue: b,
    },
  ];
}
