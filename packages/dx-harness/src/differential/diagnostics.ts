/**
 * Diagnostics comparator.
 *
 * Compares verter's diagnostics against a baseline provider's on the same emitted
 * TSX by code, projected range, and severity/category, keeping the classes DISTINCT
 * and never collapsing them: `verterOnly` (verter emitted it, the baseline did not),
 * `baselineOnly` (the baseline emitted it, verter did not), `defaultRange` (verter
 * matched the diagnostic by code but collapsed its range to the impossible `(0,0)`
 * default while the true span is elsewhere), `rangeMismatch` (a genuine two-real-ranges
 * disagreement), and `severityMismatch` (matched by code AND range but the
 * severity/category differs — an Error reported as a Warning, or vice versa). The
 * shared normalizer's `isImpossibleDefaultDiagnostic` predicate is reused for the
 * default-range decision.
 *
 * The emitted TSX is the differential's premise, so the baseline byte→position
 * converter ({@link GeneratedDocument}) is a REQUIRED input — there is no silent
 * `(0,0)` fallback when it is absent.
 */

import type { NormalizedDiagnostic } from "../baseline/bridgeClient.js";
import type { CanonicalDiagnostic, Range } from "../normalize/index.js";
import { isImpossibleDefaultDiagnostic, normalizeEol, rangesEqual } from "../normalize/index.js";
import type { Divergence } from "./outcome.js";
import type { GeneratedDocument } from "./projection.js";

/** Options driving the diagnostics comparison. */
export interface DiagnosticsCompareOptions {
  /** Code → the diagnostic's known true source span, overriding the baseline-as-oracle default. */
  readonly knownSourceSpans?: Readonly<Record<string, Range>>;
}

/** A baseline diagnostic lowered into the verter `Canonical*` shape (generated positions). */
interface BaselineCanonicalDiagnostic {
  readonly range: Range;
  readonly severity: string;
  readonly code?: string;
  readonly message: string;
}

function baselineToCanonical(
  diag: NormalizedDiagnostic,
  document: GeneratedDocument,
): BaselineCanonicalDiagnostic {
  return {
    range: document.byteRangeToPosition(diag.start, diag.end),
    severity: diag.severity,
    message: diag.message,
    ...(diag.code !== undefined ? { code: diag.code } : {}),
  };
}

/** The identity key for matching: the diagnostic code if present, else the message. */
export function diagnosticIdentityKey(diag: { code?: string; message: string }): string {
  return diag.code !== undefined ? `code:${diag.code}` : `msg:${normalizeEol(diag.message)}`;
}

/**
 * Fold a provider's severity/category vocabulary to a canonical bucket so casing and
 * the `Information`/`info` spelling never read as a category divergence, while a real
 * `Error` vs `Warning` difference does. Verter names severities `Error`/`Warning`/
 * `Information`/`Hint`; the baseline bridge names them `error`/`warning`/`info`/`hint`.
 */
export function severityCategory(severity: string): string {
  const s = severity.trim().toLowerCase();
  if (s === "information" || s === "info") return "info";
  if (s === "warn") return "warning";
  return s;
}

/**
 * Compare verter diagnostics against a baseline provider's. Returns a flat
 * divergence list (empty = agreement). Total over empty inputs.
 */
export function compareDiagnostics(
  verter: readonly CanonicalDiagnostic[],
  baseline: readonly NormalizedDiagnostic[],
  document: GeneratedDocument,
  options: DiagnosticsCompareOptions = {},
): Divergence[] {
  const divergences: Divergence[] = [];
  const baselineCanonical = baseline.map((d) => baselineToCanonical(d, document));
  const matched = new Set<number>();

  for (const v of verter) {
    const key = diagnosticIdentityKey(v);
    // Prefer a baseline candidate sharing identity AND range AND category (a full
    // match), then identity+range (a category difference), then identity alone (a
    // range difference). The first qualifying candidate is consumed.
    let idx = baselineCanonical.findIndex(
      (b, i) =>
        !matched.has(i) &&
        diagnosticIdentityKey(b) === key &&
        rangesEqual(b.range, v.range) &&
        severityCategory(b.severity) === severityCategory(v.severity),
    );
    if (idx === -1) {
      idx = baselineCanonical.findIndex(
        (b, i) =>
          !matched.has(i) && diagnosticIdentityKey(b) === key && rangesEqual(b.range, v.range),
      );
    }
    if (idx === -1) {
      idx = baselineCanonical.findIndex(
        (b, i) => !matched.has(i) && diagnosticIdentityKey(b) === key,
      );
    }
    if (idx === -1) {
      // No baseline match: verter-only.
      divergences.push({
        class: "verterOnly",
        detail: `verter emitted a diagnostic the baseline did not (${v.code ?? v.message})`,
        verterValue: v,
      });
      continue;
    }
    matched.add(idx);
    const b = baselineCanonical[idx];

    // Range divergence dominates: a default-range collapse or a real range mismatch.
    if (!rangesEqual(b.range, v.range)) {
      const known =
        (v.code !== undefined ? options.knownSourceSpans?.[v.code] : undefined) ?? b.range;
      if (isImpossibleDefaultDiagnostic(v, known)) {
        divergences.push({
          class: "defaultRange",
          detail: `diagnostic ${v.code ?? v.message} collapsed to the (0,0) default range`,
          verterValue: v.range,
          baselineValue: b.range,
        });
      } else {
        divergences.push({
          class: "rangeMismatch",
          detail: `diagnostic ${v.code ?? v.message} differs in range`,
          verterValue: v.range,
          baselineValue: b.range,
        });
      }
      continue;
    }

    // Same identity and range: a severity/category difference is its own divergence.
    if (severityCategory(v.severity) !== severityCategory(b.severity)) {
      divergences.push({
        class: "severityMismatch",
        detail: `diagnostic ${v.code ?? v.message} differs in severity/category`,
        verterValue: v.severity,
        baselineValue: b.severity,
      });
    }
  }

  // Unmatched baseline diagnostics: baseline-only.
  baselineCanonical.forEach((b, i) => {
    if (matched.has(i)) return;
    divergences.push({
      class: "baselineOnly",
      detail: `the baseline emitted a diagnostic verter did not (${b.code ?? b.message})`,
      baselineValue: b,
    });
  });

  return divergences;
}
