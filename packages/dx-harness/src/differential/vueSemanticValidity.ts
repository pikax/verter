/**
 * The curated semantic-oracle differential: the `vueSemanticValidity` dimension.
 *
 * Where the artifact-parity differential ({@link ./completion} et al.) compares
 * verter-on-`.vue` against tsgo/tsserver on verter's OWN emitted `.vue.tsx`, this
 * diff compares verter-on-`.vue` against tsgo/tsserver on a small hand-authored
 * `.ts` ORACLE that mirrors the intended Vue semantics. The `.ts` anchor is the
 * gold standard, so a self-consistent-but-wrong TSX lowering — which the
 * artifact-parity spine cannot catch, because verter and tsgo-on-the-`.vue.tsx`
 * agree while both are wrong — surfaces here as a divergence against the oracle.
 *
 * It is NOT a second comparator. Every comparison reuses the artifact-parity
 * engine: the per-method field comparators ({@link compareCompletion} /
 * {@link compareHover} / {@link compareDefinition}), the baseline-vs-baseline
 * disagreement helpers, and the {@link classifyProbe} orchestrator (which already
 * owns baseline disagreement, the authoritative-provider rule, and refusal
 * handling). Each method here is a thin closure that wires verter's normalized
 * fact and the oracle's normalized fact into those shared primitives.
 *
 * Two cross-file differences from artifact parity, both forced by the oracle and
 * verter being DIFFERENT files rather than two views of one artifact:
 *  - Hover/definition compare semantic VALUE (type label, expected identity), never
 *    a shared-coordinate range — there is no common byte space to project into.
 *  - Diagnostics cannot reuse {@link compareDiagnostics} (it projects baseline byte
 *    offsets through a shared {@link GeneratedDocument}); {@link compareOracleDiagnostics}
 *    instead composes the SAME shared primitives — {@link diagnosticIdentityKey},
 *    {@link severityCategory}, {@link isImpossibleDefaultDiagnostic} — to classify by
 *    identity/category, flag verter's own default-range collapse, and check verter's
 *    `.vue` range against the authored expected span (the raw `.ts` range is never
 *    compared across files).
 */

import type {
  NormalizedDiagnostic,
  NormalizedHover,
  NormalizedLocation,
  ProviderName,
} from "../baseline/bridgeClient.js";
import type {
  CanonicalCompletionList,
  CanonicalDefinitionTarget,
  CanonicalDiagnostic,
  CanonicalHover,
  ExpectedDefinition,
  Range,
} from "../normalize/index.js";
import { isImpossibleDefaultDiagnostic, rangesEqual } from "../normalize/index.js";
import type { Probe } from "../scenario/index.js";
import { compareCompletion, type BaselineCompletion } from "./completion.js";
import { compareDefinition } from "./definition.js";
import { diagnosticIdentityKey, severityCategory } from "./diagnostics.js";
import {
  classifyProbe,
  completionDisagrees,
  definitionDisagrees,
  diagnosticsDisagrees,
  hoverDisagrees,
  type ProviderInputs,
} from "./disagreement.js";
import { compareHover } from "./hover.js";
import type { DifferentialOutcome, Divergence } from "./outcome.js";

/**
 * The oracle path never maps through verter's emitted artifact — verter answers
 * the `.vue` probe in native source space and the oracle answers the `.ts` probe
 * in its own. So the map-absent gate inside {@link classifyProbe} is structurally
 * inapplicable: oracle probes are `mappingPolicy: none` / `requiresSourceMap:
 * false`, and this constant says "the artifact map is not part of this rail".
 */
const ORACLE_SOURCE_MAP_PRESENT = true;

// ── completion ───────────────────────────────────────────────────────────────

/** Inputs to {@link classifyOracleCompletion}. */
export interface OracleCompletionInput {
  readonly probe: Probe;
  /** Verter's normalized completion set for the `.vue` anchor. */
  readonly verter: CanonicalCompletionList;
  /** The oracle providers' normalized completion sets for the `.ts` anchor. */
  readonly providers: ProviderInputs<BaselineCompletion>;
  /** Labels the intended Vue semantics require verter to surface. */
  readonly requiredLabels?: readonly string[];
  /** When set, verter is compared only against this provider (disagreement ignored). */
  readonly authoritativeProvider?: ProviderName;
}

/** Classify a completion oracle probe via the shared completion comparator. */
export function classifyOracleCompletion(input: OracleCompletionInput): DifferentialOutcome[] {
  return classifyProbe<BaselineCompletion>({
    probe: input.probe,
    sourceMapPresent: ORACLE_SOURCE_MAP_PRESENT,
    providers: input.providers,
    ...(input.authoritativeProvider !== undefined
      ? { authoritativeProvider: input.authoritativeProvider }
      : {}),
    compareVerter: (_provider, baseline) =>
      compareCompletion(
        input.verter,
        baseline,
        input.requiredLabels !== undefined ? { requiredLabels: input.requiredLabels } : {},
      ),
    baselineDisagree: (tsgo, tsserver) => completionDisagrees(tsgo, tsserver),
  });
}

// ── hover ────────────────────────────────────────────────────────────────────

/** Inputs to {@link classifyOracleHover}. */
export interface OracleHoverInput {
  readonly probe: Probe;
  /** Verter's normalized hover for the `.vue` anchor (`null` = no hover). */
  readonly verter: CanonicalHover | null;
  /** The oracle providers' normalized hover for the `.ts` anchor. */
  readonly providers: ProviderInputs<NormalizedHover | null>;
  /** Type tokens the intended Vue semantics require in verter's stripped label. */
  readonly requiredSnippets?: readonly string[];
  readonly authoritativeProvider?: ProviderName;
}

/**
 * Classify a hover oracle probe via the shared hover comparator. No `document` is
 * passed: the oracle and verter are different files, so generated-space range
 * parity is meaningless — only the stripped type label and required snippets
 * compare.
 */
export function classifyOracleHover(input: OracleHoverInput): DifferentialOutcome[] {
  return classifyProbe<NormalizedHover | null>({
    probe: input.probe,
    sourceMapPresent: ORACLE_SOURCE_MAP_PRESENT,
    providers: input.providers,
    ...(input.authoritativeProvider !== undefined
      ? { authoritativeProvider: input.authoritativeProvider }
      : {}),
    compareVerter: (_provider, baseline) =>
      compareHover(
        input.verter,
        baseline,
        input.requiredSnippets !== undefined ? { requiredSnippets: input.requiredSnippets } : {},
      ),
    baselineDisagree: (tsgo, tsserver) => hoverDisagrees(tsgo, tsserver),
  });
}

// ── definition ───────────────────────────────────────────────────────────────

/** Inputs to {@link classifyOracleDefinition}. */
export interface OracleDefinitionInput {
  readonly probe: Probe;
  /** Verter's normalized definition targets for the `.vue` anchor. */
  readonly verter: readonly CanonicalDefinitionTarget[];
  /** The oracle providers' resolved `.ts` locations (used for baseline disagreement). */
  readonly providers: ProviderInputs<readonly NormalizedLocation[]>;
  /**
   * The expected authored Vue identity verter must resolve to. REQUIRED: the oracle
   * `.ts` resolves into a different file, so it cannot pin verter's `.vue` target
   * directly — without an authored identity any `.vue` target would pass as a false
   * agreement. A definition oracle must always carry this authored symbol identity.
   */
  readonly expected: ExpectedDefinition;
  readonly authoritativeProvider?: ProviderName;
}

/**
 * Classify a definition oracle probe. Verter is compared against the REQUIRED
 * expected authored Vue identity (the oracle `.ts` resolves into a different file,
 * so its locations cannot be compared to verter's directly — they instead drive the
 * tsgo-vs-tsserver disagreement check, confirming the oracle symbol is itself
 * resolvable). Failure is by SYMBOL IDENTITY and the generated-only-unmapped case,
 * never by `line === 0`.
 *
 * The expected-identity check runs inside `classifyProbe`'s `compareVerter`, so it is
 * provider-gated BY DESIGN — it fires only once a ready provider survives the refusal /
 * baseline-disagreement / authoritative-provider gates, reusing the one orchestrator
 * rather than forking a second comparator path.
 */
export function classifyOracleDefinition(input: OracleDefinitionInput): DifferentialOutcome[] {
  return classifyProbe<readonly NormalizedLocation[]>({
    probe: input.probe,
    sourceMapPresent: ORACLE_SOURCE_MAP_PRESENT,
    providers: input.providers,
    ...(input.authoritativeProvider !== undefined
      ? { authoritativeProvider: input.authoritativeProvider }
      : {}),
    compareVerter: (_provider, _baseline) =>
      compareDefinition(input.verter, { expected: input.expected }),
    baselineDisagree: (tsgo, tsserver) => definitionDisagrees(tsgo, tsserver),
  });
}

// ── diagnostics ──────────────────────────────────────────────────────────────

/** Options driving {@link compareOracleDiagnostics}. */
export interface OracleDiagnosticsCompareOptions {
  /** Code → the diagnostic's known true source span, for the default-range collapse check. */
  readonly knownSourceSpans?: Readonly<Record<string, Range>>;
}

/**
 * Compare verter diagnostics against the oracle's by IDENTITY and CATEGORY, plus
 * verter's `.vue` range against the authored expected span. This is the cross-file
 * sibling of {@link compareDiagnostics}: that comparator projects baseline byte
 * offsets through a shared {@link GeneratedDocument} to compare ranges in one space,
 * which the oracle (a different file from the `.vue`) has no equivalent of. So the
 * raw `.ts` baseline range is NEVER compared to the `.vue` range; instead the SAME
 * shared primitives classify the sets:
 *  - {@link diagnosticIdentityKey} + {@link severityCategory} pair verter and oracle
 *    diagnostics; an unmatched verter diagnostic is a `verterOnly` false-red, an
 *    unmatched oracle diagnostic is `baselineOnly`;
 *  - given an authored `.vue` span (`knownSourceSpans[code]`, the EXPECTED range, the
 *    diagnostics analogue of definition's `expected`): {@link isImpossibleDefaultDiagnostic}
 *    flags a `(0,0)` default-range collapse as `defaultRange`, and a real, non-default
 *    verter range that is not the expected span is a `rangeMismatch` (range dominates);
 *  - a matched pair (at the expected range, or with no authored span to check against)
 *    whose categories differ is `severityMismatch`.
 *
 * One finding per matched pair: the first qualifying divergence (range before severity)
 * is reported — sufficient to surface the pair, not an exhaustive per-pair enumeration.
 */
export function compareOracleDiagnostics(
  verter: readonly CanonicalDiagnostic[],
  baseline: readonly NormalizedDiagnostic[],
  options: OracleDiagnosticsCompareOptions = {},
): Divergence[] {
  const divergences: Divergence[] = [];
  const matched = new Set<number>();

  for (const v of verter) {
    const key = diagnosticIdentityKey(v);
    const idx = baseline.findIndex((b, i) => !matched.has(i) && diagnosticIdentityKey(b) === key);
    if (idx === -1) {
      divergences.push({
        class: "verterOnly",
        detail: `verter emitted a diagnostic the oracle did not (${v.code ?? v.message})`,
        verterValue: v,
      });
      continue;
    }
    matched.add(idx);
    const b = baseline[idx];

    // The oracle byte span is in the `.ts` file, not the `.vue` — it cannot pin
    // verter's range. The known source span (when the scenario supplies one) is the
    // authored `.vue` span, so it is BOTH the oracle for the default-range collapse
    // AND the EXPECTED range verter's own `.vue` range must match. With no authored
    // span the cross-file `.ts` range has no shared coordinate, so range is not checked.
    const known = v.code !== undefined ? options.knownSourceSpans?.[v.code] : undefined;
    if (known !== undefined) {
      if (isImpossibleDefaultDiagnostic(v, known)) {
        divergences.push({
          class: "defaultRange",
          detail: `diagnostic ${v.code ?? v.message} collapsed to the (0,0) default range`,
          verterValue: v.range,
        });
        continue;
      }
      if (!rangesEqual(v.range, known)) {
        // A real, non-default verter range that is not the authored expected span —
        // a range divergence; range dominates the severity check (as in compareDiagnostics).
        divergences.push({
          class: "rangeMismatch",
          detail: `diagnostic ${v.code ?? v.message} resolved to a .vue range other than its authored span`,
          verterValue: v.range,
          baselineValue: known,
        });
        continue;
      }
    }

    if (severityCategory(v.severity) !== severityCategory(b.severity)) {
      divergences.push({
        class: "severityMismatch",
        detail: `diagnostic ${v.code ?? v.message} differs in severity/category`,
        verterValue: v.severity,
        baselineValue: b.severity,
      });
    }
  }

  baseline.forEach((b, i) => {
    if (matched.has(i)) return;
    divergences.push({
      class: "baselineOnly",
      detail: `the oracle emitted a diagnostic verter did not (${b.code ?? b.message})`,
      baselineValue: b,
    });
  });

  return divergences;
}

/** Inputs to {@link classifyOracleDiagnostics}. */
export interface OracleDiagnosticsInput {
  readonly probe: Probe;
  /** Verter's normalized diagnostics for the `.vue` document. */
  readonly verter: readonly CanonicalDiagnostic[];
  /** The oracle providers' normalized diagnostics for the `.ts` document. */
  readonly providers: ProviderInputs<readonly NormalizedDiagnostic[]>;
  readonly knownSourceSpans?: Readonly<Record<string, Range>>;
  readonly authoritativeProvider?: ProviderName;
}

/** Classify a diagnostics oracle probe via {@link compareOracleDiagnostics}. */
export function classifyOracleDiagnostics(input: OracleDiagnosticsInput): DifferentialOutcome[] {
  return classifyProbe<readonly NormalizedDiagnostic[]>({
    probe: input.probe,
    sourceMapPresent: ORACLE_SOURCE_MAP_PRESENT,
    providers: input.providers,
    ...(input.authoritativeProvider !== undefined
      ? { authoritativeProvider: input.authoritativeProvider }
      : {}),
    compareVerter: (_provider, baseline) =>
      compareOracleDiagnostics(
        input.verter,
        baseline,
        input.knownSourceSpans !== undefined ? { knownSourceSpans: input.knownSourceSpans } : {},
      ),
    baselineDisagree: (tsgo, tsserver) => diagnosticsDisagrees(tsgo, tsserver),
  });
}

// ── dispatch ─────────────────────────────────────────────────────────────────

/** The per-method oracle classify input, discriminated by `method`. */
export type OracleClassifyInput =
  | ({ readonly method: "completion" } & OracleCompletionInput)
  | ({ readonly method: "hover" } & OracleHoverInput)
  | ({ readonly method: "definition" } & OracleDefinitionInput)
  | ({ readonly method: "diagnostics" } & OracleDiagnosticsInput);

/** Dispatch an oracle probe to its per-method classifier. */
export function classifyOracleProbe(input: OracleClassifyInput): DifferentialOutcome[] {
  switch (input.method) {
    case "completion":
      return classifyOracleCompletion(input);
    case "hover":
      return classifyOracleHover(input);
    case "definition":
      return classifyOracleDefinition(input);
    case "diagnostics":
      return classifyOracleDiagnostics(input);
  }
}
