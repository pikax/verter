/**
 * Shared, pure helpers for the LSP response normalizers: EOL normalization,
 * position/range equality, deterministic stringification (for order-insensitive
 * dedup/sort keys), the LSP numeric-enum name maps, and generated-artifact URI
 * classification.
 */

import type { Position, Range } from "./lspTypes.js";

/**
 * Fold `\r\n` and lone `\r` to `\n` so a value produced on Windows compares equal
 * to its POSIX twin. Text comparisons in the differential are EOL-normalized, never
 * raw-byte (the cross-platform rule).
 */
export function normalizeEol(text: string): string {
  return text.replace(/\r\n?/g, "\n");
}

/** Coerce an untyped value to a finite line/character offset, defaulting to 0. */
function coerceOffset(value: unknown): number {
  return typeof value === "number" && Number.isFinite(value) ? value : 0;
}

/** Coerce an untyped value to a canonical {@link Position}, defaulting missing coordinates to 0. */
function coercePosition(value: unknown): Position {
  if (value !== null && typeof value === "object") {
    const p = value as { line?: unknown; character?: unknown };
    return { line: coerceOffset(p.line), character: coerceOffset(p.character) };
  }
  return { line: 0, character: 0 };
}

/**
 * Coerce an untyped value to a canonical {@link Range}, defaulting to the
 * `(0,0)-(0,0)` zero-width sentinel. Used by the normalizers to fold a raw client
 * `any` (a missing or malformed `range`/`start`/`end`/coordinate) into a valid range
 * instead of throwing or leaking a broken shape into the canonical output.
 */
export function coerceRange(value: unknown): Range {
  if (value !== null && typeof value === "object") {
    const r = value as { start?: unknown; end?: unknown };
    return { start: coercePosition(r.start), end: coercePosition(r.end) };
  }
  return { start: { line: 0, character: 0 }, end: { line: 0, character: 0 } };
}

/** Whether two LSP positions are identical. */
export function positionsEqual(a: Position, b: Position): boolean {
  return a.line === b.line && a.character === b.character;
}

/** Whether two LSP ranges are identical. */
export function rangesEqual(a: Range, b: Range): boolean {
  return positionsEqual(a.start, b.start) && positionsEqual(a.end, b.end);
}

/**
 * Deterministically stringify a value with object keys sorted recursively, so a
 * structurally-equal value yields a byte-identical key regardless of source key
 * order. Used only to build stable dedup/sort keys — never as stored output.
 */
export function stableStringify(value: unknown): string {
  if (value === null || typeof value !== "object") return JSON.stringify(value) ?? "null";
  if (Array.isArray(value)) return `[${value.map(stableStringify).join(",")}]`;
  const record = value as Record<string, unknown>;
  const keys = Object.keys(record).sort();
  return `{${keys.map((k) => `${JSON.stringify(k)}:${stableStringify(record[k])}`).join(",")}}`;
}

/** LSP `CompletionItemKind` (3.17) numeric → name. */
const COMPLETION_ITEM_KIND_NAMES: Record<number, string> = {
  1: "Text",
  2: "Method",
  3: "Function",
  4: "Constructor",
  5: "Field",
  6: "Variable",
  7: "Class",
  8: "Interface",
  9: "Module",
  10: "Property",
  11: "Unit",
  12: "Value",
  13: "Enum",
  14: "Keyword",
  15: "Snippet",
  16: "Color",
  17: "File",
  18: "Reference",
  19: "Folder",
  20: "EnumMember",
  21: "Constant",
  22: "Struct",
  23: "Event",
  24: "Operator",
  25: "TypeParameter",
};

/**
 * Map a numeric `CompletionItemKind` to its stable name; an out-of-range number
 * is stringified (never dropped), and `undefined` stays `undefined`.
 */
export function completionItemKindName(kind: number | undefined): string | undefined {
  if (kind === undefined) return undefined;
  return COMPLETION_ITEM_KIND_NAMES[kind] ?? String(kind);
}

/** LSP `DiagnosticSeverity` (3.17) numeric → name. */
const DIAGNOSTIC_SEVERITY_NAMES: Record<number, string> = {
  1: "Error",
  2: "Warning",
  3: "Information",
  4: "Hint",
};

/**
 * Map a numeric `DiagnosticSeverity` to its name; an out-of-range number is
 * stringified, and an omitted severity normalizes to the literal `"unknown"` (LSP
 * leaves an absent severity client-defined — the harness records it rather than guessing).
 */
export function diagnosticSeverityName(severity: number | undefined): string {
  if (severity === undefined) return "unknown";
  return DIAGNOSTIC_SEVERITY_NAMES[severity] ?? String(severity);
}

/** The generated-artifact path suffixes verter emits. */
const GENERATED_SUFFIXES = [".vue.tsx", ".vue.jsx", ".vue.ts"] as const;

/**
 * Whether a URI points at a verter-generated artifact (`.vue.tsx`/`.vue.jsx`/
 * `.vue.ts`) rather than an authored source. The query/fragment is stripped and
 * the comparison is case-insensitive, so the check is portable across schemes and OSes.
 */
export function isGeneratedUri(uri: string): boolean {
  const path = uri.split(/[?#]/, 1)[0].toLowerCase();
  return GENERATED_SUFFIXES.some((suffix) => path.endsWith(suffix));
}
