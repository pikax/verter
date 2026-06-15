/**
 * Hover comparator.
 *
 * Compares the normalized TYPE LABEL of verter's hover against a baseline
 * provider's, plus any required snippets, with unstable documentation stripped
 * first so prose/JSDoc churn never reads as a type divergence. "No hover" and
 * "empty hover" are both contentless and are handled identically — a contentless
 * hover on one side only (against real content on the other) is a presence
 * mismatch, but contentless-vs-contentless is never a false failure (a hover over
 * a synthetic/empty region is legitimately empty).
 */

import type { NormalizedHover } from "../baseline/bridgeClient.js";
import type { CanonicalHover, Range } from "../normalize/index.js";
import { normalizeEol, rangesEqual } from "../normalize/index.js";
import type { GeneratedDocument } from "./projection.js";
import type { Divergence } from "./outcome.js";

/** Extract the inner content of the FIRST fenced code block, or `null` if none is closed. */
function firstFencedBlock(lines: readonly string[]): string | null {
  let open = -1;
  for (let i = 0; i < lines.length; i++) {
    if (/^\s*```/.test(lines[i])) {
      if (open === -1) open = i;
      else return lines.slice(open + 1, i).join("\n");
    }
  }
  return null;
}

/** The leading run of non-blank lines up to the first blank-after-content / `@`-tag line. */
function leadingParagraph(lines: readonly string[]): string[] {
  const region: string[] = [];
  let seenContent = false;
  for (const line of lines) {
    const trimmed = line.trim();
    if (trimmed === "") {
      if (seenContent) break;
      continue;
    }
    if (trimmed.startsWith("@")) {
      if (seenContent) break;
      continue;
    }
    region.push(trimmed);
    seenContent = true;
  }
  return region;
}

/**
 * Reduce a hover body to its STABLE type label: the first fenced code block's
 * content (the TS signature convention) or, absent a fence, the leading signature
 * paragraph — with `@`-tag lines dropped and all whitespace collapsed to single
 * spaces. Trailing documentation prose, which churns between providers and
 * versions, is removed.
 */
export function stripUnstableDocs(contents: string): string {
  const lines = normalizeEol(contents).split("\n");
  const fenced = firstFencedBlock(lines);
  const region = fenced !== null ? leadingParagraph(fenced.split("\n")) : leadingParagraph(lines);
  return region.join(" ").replace(/\s+/g, " ").trim();
}

/** Whether a hover has a non-empty stable type label (a contentless hover does not). */
function hoverLabel(hover: { contents: string } | null): string | null {
  if (hover === null) return null;
  const label = stripUnstableDocs(hover.contents);
  return label === "" ? null : label;
}

/** Options driving the hover comparison. */
export interface HoverCompareOptions {
  /** Substrings that MUST appear in verter's stripped type label. */
  readonly requiredSnippets?: readonly string[];
  /**
   * The prepared emitted-TSX converter — enables generated-space range parity when
   * both hovers carry a range. Range parity is optional: type-label parity stands on
   * its own, so this is omitted when the artifact converter is unavailable.
   */
  readonly document?: GeneratedDocument;
}

/** The baseline byte range of a hover, as an LSP {@link Range}, when both ends are present. */
function baselineHoverRange(hover: NormalizedHover, document: GeneratedDocument): Range | null {
  if (hover.rangeStart === undefined || hover.rangeEnd === undefined) return null;
  return document.byteRangeToPosition(hover.rangeStart, hover.rangeEnd);
}

/**
 * Compare a verter hover against a baseline hover. Returns a flat divergence list
 * (empty = agreement). Total over `null` on either side.
 */
export function compareHover(
  verter: CanonicalHover | null,
  baseline: NormalizedHover | null,
  options: HoverCompareOptions = {},
): Divergence[] {
  const divergences: Divergence[] = [];
  const vLabel = hoverLabel(verter);
  const bLabel = hoverLabel(baseline);

  // Contentless on both sides is never a failure.
  if (vLabel === null && bLabel === null) return divergences;

  // Content on exactly one side: a presence mismatch.
  if (vLabel === null || bLabel === null) {
    divergences.push({
      class: "hoverPresenceMismatch",
      detail:
        vLabel === null ? "verter produced no hover content" : "baseline produced no hover content",
      verterValue: vLabel,
      baselineValue: bLabel,
    });
    return divergences;
  }

  // Both have a type label: compare.
  if (vLabel !== bLabel) {
    divergences.push({
      class: "typeLabelMismatch",
      detail: "hover type label differs",
      verterValue: vLabel,
      baselineValue: bLabel,
    });
  }
  for (const snippet of options.requiredSnippets ?? []) {
    if (!vLabel.includes(snippet)) {
      divergences.push({
        class: "missingSnippet",
        detail: `required hover snippet "${snippet}" is absent from verter's type label`,
        verterValue: vLabel,
      });
    }
  }

  // Optional generated-space range parity (only when both ranges + the converter exist).
  if (options.document !== undefined && verter?.range !== undefined && baseline !== null) {
    const bRange = baselineHoverRange(baseline, options.document);
    if (bRange !== null && !rangesEqual(verter.range, bRange)) {
      divergences.push({
        class: "rangeMismatch",
        detail: "hover range differs in generated space",
        verterValue: verter.range,
        baselineValue: bRange,
      });
    }
  }

  return divergences;
}
