/**
 * Diagnostics response normalizer.
 *
 * Folds a raw `Diagnostic[]` into a canonical, stably-sorted
 * `{ range, severity, code?, source?, message }[]` so the differential compares
 * diagnostic SETS, not server emission order.
 *
 * Default-range invariant: the component diagnostic fallback is specifically the
 * DEFAULT `(0,0)-(0,0)` range on offset-mapping failure — NOT any line-0 range.
 * {@link isImpossibleDefaultDiagnostic} takes the KNOWN source span and flags a
 * diagnostic ONLY when its range collapsed to the origin while the real source is
 * elsewhere, or when the range is the zero-width default sentinel. A precise,
 * positive-width line-0 diagnostic whose known source is at the origin is NOT flagged.
 */

import type { DiagnosticsResponse, Range } from "./lspTypes.js";
import { coerceRange, diagnosticSeverityName, normalizeEol } from "./shared.js";

/** A canonical diagnostic. `severity` is always present (an absent one normalizes to `"unknown"`). */
export interface CanonicalDiagnostic {
  readonly range: Range;
  readonly severity: string;
  readonly code?: string;
  readonly source?: string;
  readonly message: string;
  /**
   * The published LSP `DiagnosticTag` numbers (1 = Unnecessary fade, 2 =
   * Deprecated strikethrough), stably sorted. {@link normalizeDiagnostics}
   * ALWAYS populates this (empty when the diagnostic carries none) so the
   * dx-harness can guard the user-visible gray-out / strikethrough contract
   * end-to-end; optional only so hand-built fixtures need not spell `tags: []`.
   */
  readonly tags?: readonly number[];
}

function toCanonical(diag: unknown): CanonicalDiagnostic {
  // Every array entry is an `any` from the raw client: a `null`, a non-object, a
  // bare `{}`, or a junk object must all fold to a safe canonical diagnostic rather
  // than dereferencing `diag.range` / `diag.message`.
  const d = (diag !== null && typeof diag === "object" ? diag : {}) as Record<string, unknown>;
  const out: {
    range: Range;
    severity: string;
    code?: string;
    source?: string;
    message: string;
    tags: readonly number[];
  } = {
    range: coerceRange(d.range),
    severity: diagnosticSeverityName(typeof d.severity === "number" ? d.severity : undefined),
    message: typeof d.message === "string" ? normalizeEol(d.message) : "",
    // The published LSP tags (1 = Unnecessary, 2 = Deprecated). A junk entry (a
    // non-number) is dropped; the kept tags are sorted for set comparison.
    tags: Array.isArray(d.tags)
      ? d.tags.filter((t): t is number => typeof t === "number").sort((a, b) => a - b)
      : [],
  };
  // `code` is `string | number` when present; a non-scalar code is omitted.
  if (typeof d.code === "string" || typeof d.code === "number") out.code = String(d.code);
  if (typeof d.source === "string") out.source = d.source;
  return out;
}

function compareStrings(a: string, b: string): number {
  return a < b ? -1 : a > b ? 1 : 0;
}

function compare(a: CanonicalDiagnostic, b: CanonicalDiagnostic): number {
  return (
    a.range.start.line - b.range.start.line ||
    a.range.start.character - b.range.start.character ||
    a.range.end.line - b.range.end.line ||
    a.range.end.character - b.range.end.character ||
    compareStrings(a.severity, b.severity) ||
    compareStrings(a.code ?? "", b.code ?? "") ||
    compareStrings(a.source ?? "", b.source ?? "") ||
    compareStrings(a.message, b.message) ||
    // Tags are pre-sorted within each diagnostic; compare them so two otherwise-
    // identical diagnostics that differ only by tag sort deterministically.
    compareStrings((a.tags ?? []).join(","), (b.tags ?? []).join(","))
  );
}

/**
 * Normalize a raw diagnostics array. Total over `null`/`undefined`/empty; the
 * result is order-insensitive (a permuted array normalizes equal).
 */
export function normalizeDiagnostics(raw: DiagnosticsResponse): readonly CanonicalDiagnostic[] {
  // The raw client hands over an untyped value; anything that is not an array
  // (null, undefined, or a malformed body) folds to the empty set.
  if (!Array.isArray(raw)) return [];
  return raw.map(toCanonical).sort(compare);
}

/** Whether a range is the default `(0,0)-(0,0)` zero-width sentinel. */
export function isDefaultDiagnosticRange(range: Range): boolean {
  return (
    range.start.line === 0 &&
    range.start.character === 0 &&
    range.end.line === 0 &&
    range.end.character === 0
  );
}

/**
 * Whether a diagnostic is the impossible/default `(0,0)` fallback, given the KNOWN
 * source span where it should sit. Flagged when the range is the zero-width default
 * sentinel (an impossible extent for a real diagnostic), OR when the range start
 * collapsed to the origin while the known source span is elsewhere (a mapping
 * failure). A precise, positive-width line-0 diagnostic whose known source IS at
 * the origin is NOT flagged — the default-range distinction.
 */
export function isImpossibleDefaultDiagnostic(
  diag: { readonly range: Range },
  knownSourceSpan: Range,
): boolean {
  const range = diag.range;
  const startAtOrigin = range.start.line === 0 && range.start.character === 0;
  const knownStartAtOrigin =
    knownSourceSpan.start.line === 0 && knownSourceSpan.start.character === 0;
  return isDefaultDiagnosticRange(range) || (startAtOrigin && !knownStartAtOrigin);
}
