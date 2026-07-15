/**
 * Hover response normalizer.
 *
 * Folds a raw `Hover` (any of the LSP `contents` shapes) into a CLEAN comparable
 * label string, so a downstream Vue-surface invariant can assert `@click` (not
 * `onClick`), event modifiers like `@touchmove.stop`, `ref`, and handler-arg
 * types. "No hover" (a null/absent response) is represented DISTINCTLY from an
 * "empty hover" (a real `Hover` with empty contents): the former returns `null`,
 * the latter `{ contents: "" }`. A hover over a synthetic/empty region is not
 * automatically a failure, so this never throws.
 */

import type { HoverResponse, Range } from "./lspTypes.js";
import { normalizeEol } from "./shared.js";

/** A canonical hover: a clean comparable contents string plus the optional range. */
export interface CanonicalHover {
  readonly contents: string;
  readonly range?: Range;
}

/** The text value of a `MarkedString` — its string, or the `value` of a code block. */
function markedStringValue(part: unknown): string {
  if (typeof part === "string") return part;
  if (part !== null && typeof part === "object") {
    const value = (part as { value?: unknown }).value;
    if (typeof value === "string") return value;
  }
  return "";
}

/** Extract the raw text from any LSP `contents` shape, dropping language fences. */
function extractContents(contents: unknown): string {
  if (typeof contents === "string") return contents;
  if (Array.isArray(contents)) return contents.map(markedStringValue).join("\n");
  // A single object: `MarkupContent { kind, value }` or `MarkedString { language, value }`.
  if (contents !== null && typeof contents === "object") {
    const value = (contents as { value?: unknown }).value;
    if (typeof value === "string") return value;
  }
  return "";
}

/**
 * Normalize a raw hover response. Returns `null` for an absent hover, or a
 * `CanonicalHover` whose `contents` is EOL-normalized (cross-platform) and trimmed
 * — including the empty string for an empty-contents hover.
 */
export function normalizeHover(raw: HoverResponse): CanonicalHover | null {
  if (raw === null || raw === undefined) return null;
  const rawContents: unknown = (raw as { contents?: unknown }).contents;
  // A null/absent `contents` body is no-hover — distinct from an empty-contents hover.
  if (rawContents === null || rawContents === undefined) return null;
  const contents = normalizeEol(extractContents(rawContents)).trim();
  return raw.range !== undefined ? { contents, range: raw.range } : { contents };
}
