/**
 * The coordinate seam between the two differential inputs.
 *
 * The verter side speaks LSP `{ line, character }` in UTF-16 code units; the
 * `verter_dx_baseline` bridge speaks UTF-8 byte offsets over the SAME emitted
 * TSX. To compare them, both are brought into one space — generated
 * `{ line, character }` (UTF-16), where the verter `Canonical*` forms already
 * live and verter's own emitted source map is keyed (the compiler emits 0-based
 * UTF-16 columns). This module owns the two conversions:
 *
 *  - {@link baselineByteToPosition} / {@link baselineRangeToPosition} fold a
 *    baseline UTF-8 byte offset into a generated `{ line, character }`, reusing
 *    the encoding-aware `@verter/lsp-test-client` converter (surrogate-pair and
 *    CRLF correct) rather than a second offset walker.
 *  - {@link projectGeneratedPosition} / {@link projectGeneratedRange} project a
 *    generated `{ line, character }` back to the authored Vue position through a
 *    V3 source map, for Vue-space reporting and the definition expected-source
 *    checks. Both sides of the differential otherwise compare in generated space.
 */

import { DocumentPositions } from "@verter/lsp-test-client";

import type { Position, Range } from "../normalize/index.js";

// ── baseline byte offset -> generated LSP position ───────────────────────────

/**
 * A byte→position converter prepared ONCE over an emitted-TSX artifact and queried
 * many times. The runner builds one per artifact and reuses it across every probe on
 * that artifact, so the document's line index is scanned once — not rebuilt per offset
 * conversion (the per-call cost of the one-shot {@link baselineByteToPosition}). It
 * wraps the encoding-aware {@link DocumentPositions} seam from `@verter/lsp-test-client`
 * (surrogate-pair and CRLF correct), never a second offset walker.
 */
export class GeneratedDocument {
  private readonly positions: DocumentPositions;

  constructor(emittedTsx: string) {
    this.positions = new DocumentPositions(emittedTsx);
  }

  /**
   * Fold a UTF-8 BYTE offset into a generated LSP `{ line, character }` whose
   * `character` is a UTF-16 code-unit column — the space the verter `Canonical*`
   * forms and the emitted source map both use. The offset is clamped into range; a
   * mid-character offset clamps to that character's start.
   */
  byteToPosition(byteOffset: number): Position {
    const pos = this.positions.byteToPosition(byteOffset, "utf-16");
    return { line: pos.line, character: pos.character };
  }

  /** Fold a `[start, end)` UTF-8 byte range into a generated LSP {@link Range}. */
  byteRangeToPosition(start: number, end: number): Range {
    return { start: this.byteToPosition(start), end: this.byteToPosition(end) };
  }
}

/**
 * One-shot convenience: fold a baseline UTF-8 BYTE offset into the emitted TSX into a
 * generated LSP `{ line, character }` (UTF-16 column). Builds a throwaway converter;
 * for many offsets over one document prefer {@link GeneratedDocument}.
 */
export function baselineByteToPosition(emittedTsx: string, byteOffset: number): Position {
  return new GeneratedDocument(emittedTsx).byteToPosition(byteOffset);
}

/**
 * One-shot convenience: fold a baseline `[start, end)` byte range over the
 * emitted TSX into a generated LSP {@link Range} (UTF-16 columns).
 */
export function baselineRangeToPosition(emittedTsx: string, start: number, end: number): Range {
  return new GeneratedDocument(emittedTsx).byteRangeToPosition(start, end);
}

// ── V3 source map: parse + VLQ decode ────────────────────────────────────────

/** The authored source coordinates a mapped segment points at. */
export interface SegmentSource {
  readonly index: number;
  readonly line: number;
  readonly column: number;
}

/**
 * A decoded V3 mapping segment for one generated column. A MAPPED segment carries
 * authored source coordinates. An UNMAPPED segment — a 1-field V3 segment, the shape
 * verter's compiler emits for inserted/generated content (a source-less token,
 * `code_transform/source_map.rs`) — has `source: null` and marks the generated run
 * from its column until the next segment as having no authored source.
 */
export interface MappingSegment {
  readonly genColumn: number;
  readonly source: SegmentSource | null;
}

/** A parsed V3 source map: the source list plus decoded segments per generated line. */
export interface ParsedSourceMap {
  readonly sources: readonly string[];
  /** Decoded segments grouped by generated line (index = 0-based generated line). */
  readonly lines: readonly (readonly MappingSegment[])[];
}

/** An authored position recovered through the source map. */
export interface OriginalPosition {
  /** The source file (a `map.sources` entry) the generated position maps back to. */
  readonly source: string;
  readonly line: number;
  readonly character: number;
}

/** An authored range recovered through the source map (start and end share a source). */
export interface OriginalRange {
  readonly source: string;
  readonly range: Range;
}

const VLQ_CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const VLQ_LOOKUP = new Int16Array(128).fill(-1);
for (let i = 0; i < VLQ_CHARS.length; i++) {
  VLQ_LOOKUP[VLQ_CHARS.charCodeAt(i)] = i;
}

/**
 * Decode a V3 `mappings` string into per-generated-line segments. A complete 4- or
 * 5-field segment is a MAPPED segment carrying source coordinates; a 1-field segment
 * is retained as an UNMAPPED boundary (`source: null`) so the run it covers is not
 * silently attributed to the previous mapped segment. Source line/column/index are
 * cumulative across the whole map; the generated column resets at each `;` (line
 * boundary), per the V3 spec.
 */
export function decodeVlqMappings(mappings: string): MappingSegment[][] {
  const lines: MappingSegment[][] = [];
  let current: MappingSegment[] = [];
  let genColumn = 0;
  let sourceIndex = 0;
  let sourceLine = 0;
  let sourceColumn = 0;

  let i = 0;
  const n = mappings.length;
  while (i < n) {
    const ch = mappings.charCodeAt(i);
    if (ch === 59 /* ; */) {
      lines.push(current);
      current = [];
      genColumn = 0;
      i++;
      continue;
    }
    if (ch === 44 /* , */) {
      i++;
      continue;
    }
    const fields: number[] = [];
    while (i < n) {
      const c = mappings.charCodeAt(i);
      if (c === 59 || c === 44) break;
      let value = 0;
      let shift = 0;
      let cont = true;
      while (cont) {
        if (i >= n) throw new Error("source map mappings ended mid-VLQ segment");
        const digit = VLQ_LOOKUP[mappings.charCodeAt(i)];
        if (digit < 0) throw new Error("invalid base64 VLQ character in source map mappings");
        i++;
        cont = (digit & 32) !== 0;
        value += (digit & 31) << shift;
        shift += 5;
      }
      fields.push(value & 1 ? -(value >>> 1) : value >>> 1);
    }
    if (fields.length >= 4) {
      genColumn += fields[0];
      sourceIndex += fields[1];
      sourceLine += fields[2];
      sourceColumn += fields[3];
      current.push({
        genColumn,
        source: { index: sourceIndex, line: sourceLine, column: sourceColumn },
      });
    } else if (fields.length >= 1) {
      // A generated-column-only segment: retain it as an unmapped boundary so a later
      // position in this run resolves to "no source" rather than the prior mapped hop.
      genColumn += fields[0];
      current.push({ genColumn, source: null });
    }
  }
  lines.push(current);
  return lines;
}

/** The V3 source map fields this layer reads. */
interface SourceMapV3 {
  version: number;
  sources: unknown;
  mappings: unknown;
}

/**
 * Parse a V3 source map JSON string into decoded {@link ParsedSourceMap} state.
 *
 * @throws when the JSON is malformed, the `version` is not 3, or `sources` /
 *   `mappings` are not the expected shapes — a structural fault is surfaced, not
 *   silently treated as "no mapping".
 */
export function parseSourceMap(json: string): ParsedSourceMap {
  const raw: unknown = JSON.parse(json);
  if (raw === null || typeof raw !== "object") {
    throw new Error("source map is not an object");
  }
  const map = raw as SourceMapV3;
  if (map.version !== 3) {
    throw new Error(`unsupported source map version: ${String(map.version)} (expected 3)`);
  }
  if (!Array.isArray(map.sources) || !map.sources.every((s) => typeof s === "string")) {
    throw new Error("source map `sources` must be an array of strings");
  }
  if (typeof map.mappings !== "string") {
    throw new Error("source map `mappings` must be a string");
  }
  return { sources: map.sources as string[], lines: decodeVlqMappings(map.mappings) };
}

/**
 * The covering segment for a generated column on a generated line: the segment
 * with the greatest `genColumn <= character`. The segments on a line are emitted
 * in ascending generated-column order, so a linear scan to the last qualifying
 * segment is correct.
 */
function coveringSegment(
  segments: readonly MappingSegment[],
  character: number,
): MappingSegment | null {
  let best: MappingSegment | null = null;
  for (const segment of segments) {
    if (segment.genColumn <= character) best = segment;
    else break;
  }
  return best;
}

/**
 * Project a generated `{ line, character }` (UTF-16) back to its authored Vue
 * position through the source map. Returns `null` when the generated line has no
 * segments, when no segment covers the column, or when the covering segment is an
 * unmapped (source-less) boundary — all three mean the position is in
 * inserted/generated text with no authored source.
 *
 * Within a mapped covering segment the offset is interpolated (`character - genColumn`
 * added to the authored column), which is exact for a 1:1-copied run — the case
 * for the identifier/token ranges definition and hover queries land on. It is
 * NOT a guarantee for overwritten/synthesized generated content, where the
 * segment start is the only exact anchor.
 */
export function projectGeneratedPosition(
  map: ParsedSourceMap,
  generated: Position,
): OriginalPosition | null {
  if (generated.line < 0 || generated.line >= map.lines.length) return null;
  const segment = coveringSegment(map.lines[generated.line], generated.character);
  if (segment === null || segment.source === null) return null;
  const source = map.sources[segment.source.index];
  if (source === undefined) return null;
  return {
    source,
    line: segment.source.line,
    character: segment.source.column + (generated.character - segment.genColumn),
  };
}

/**
 * Project a generated {@link Range} back to an authored {@link OriginalRange} by proving EVERY
 * generated content column the range covers, never by sampling its endpoints. Returns `null`
 * unless the whole content `[start .. end-1]` is one contiguous same-source 1:1 mapping: a start
 * and end in different sources, a source-less / cross-source / authored-column-jumping hole
 * anywhere in the interior, or a generated line break that does not restart the next authored line
 * at column 0, all make the range unprojectable rather than fabricated across the gap.
 *
 * An LSP range is inclusive-start / EXCLUSIVE-end, so its content is the positions
 * `[start .. end-1]` and the exclusive end is NOT content. A segment opening exactly at
 * `generated.end.character` therefore does not reject the range — that is what lets a fully-mapped
 * token whose end abuts a generated-only (unmapped) boundary still project, which an endpoint-only
 * check cannot tell apart from an interior hole that mapping resumes past before the end.
 *
 * The authored exclusive end is derived only after the content validates. For a real-column end it
 * is one past the authored position of the last content column (`generated.end.character - 1`). For
 * an end at column 0 of a later generated line — where no column of `generated.end.line` is content
 * and the content runs through the generated EOL of the previous line — the boundary `generated.end`
 * is itself projected: a faithful line break carries it to the START of the next authored line
 * (`{ lastAuthoredLine + 1, 0 }`), so it both gates the wrap as same-source and supplies the
 * authored exclusive end.
 */
export function projectGeneratedRange(
  map: ParsedSourceMap,
  generated: Range,
): OriginalRange | null {
  const start = projectGeneratedPosition(map, generated.start);
  if (start === null) return null;
  const startPos = { line: start.line, character: start.character };

  // A zero-width range is its single position: it projects exactly when the start does, with a
  // zero-width authored range and no content columns to walk.
  if (
    generated.start.line === generated.end.line &&
    generated.start.character === generated.end.character
  ) {
    return { source: start.source, range: { start: startPos, end: startPos } };
  }

  // An inverted range (end before start) has no content and is not a coherent span.
  if (
    generated.end.line < generated.start.line ||
    (generated.end.line === generated.start.line &&
      generated.end.character < generated.start.character)
  ) {
    return null;
  }

  // The start projected, so its covering segment is mapped; its source anchors the whole span and
  // the same authored line seeds the cross-line-break continuity check below.
  const startCover = coveringSegment(map.lines[generated.start.line], generated.start.character);
  if (startCover === null || startCover.source === null) return null;
  const sourceIndex = startCover.source.index;
  const sourceName = map.sources[sourceIndex];
  if (sourceName === undefined) return null;

  // An end at column 0 has its content run through the previous generated line's EOL; a real-column
  // end's content runs through `end.character - 1` on the end line itself.
  const endsAtColumnZero = generated.end.character === 0;
  const lastContentLine = endsAtColumnZero ? generated.end.line - 1 : generated.end.line;
  if (lastContentLine < generated.start.line) return null;

  let previousAuthoredLine = startCover.source.line;
  for (let line = generated.start.line; line <= lastContentLine; line++) {
    const segments = line >= 0 && line < map.lines.length ? map.lines[line] : [];
    const from = line === generated.start.line ? generated.start.character : 0;
    // The exclusive-end column is NOT content; only a real-column end on the end line itself bounds
    // a content line short of its generated EOL.
    const toExclusive =
      line === generated.end.line && generated.end.character > 0
        ? generated.end.character
        : Number.POSITIVE_INFINITY;

    const cover = coveringSegment(segments, from);
    if (cover === null || cover.source === null || cover.source.index !== sourceIndex) {
      return null;
    }
    // Across a generated line break a faithful 1:1 copy advances exactly one authored line and
    // restarts it at column 0 — a break onto a non-adjacent authored line, or onto a column past
    // the line start (which would fabricate the authored columns before it), is rejected.
    if (
      line > generated.start.line &&
      (cover.source.line !== previousAuthoredLine + 1 || cover.source.column !== 0)
    ) {
      return null;
    }

    // Within the line every sub-run must keep the authored line and resume at the authored column
    // the previous segment interpolates to — a source-less, cross-source, authored-line-breaking,
    // or authored-column-jumping segment is not a contiguous copy.
    let previous: MappingSegment = cover;
    for (const segment of segments) {
      if (segment.genColumn <= from) continue; // already covered by `cover`
      if (segment.genColumn >= toExclusive) break; // at/past the exclusive end — not content
      const previousSource = previous.source;
      if (
        segment.source === null ||
        previousSource === null ||
        segment.source.index !== sourceIndex ||
        segment.source.line !== previousSource.line
      ) {
        return null;
      }
      const expectedColumn = previousSource.column + (segment.genColumn - previous.genColumn);
      if (segment.source.column !== expectedColumn) return null;
      previous = segment;
    }
    previousAuthoredLine = cover.source.line;
  }

  let authoredEnd: OriginalPosition;
  if (endsAtColumnZero) {
    // The column-0 break is both gated as a faithful same-source wrap AND supplies the authored
    // exclusive end: it must map to the start source at the START of the next authored line.
    const boundary = projectGeneratedPosition(map, generated.end);
    if (
      boundary === null ||
      boundary.source !== sourceName ||
      boundary.line !== previousAuthoredLine + 1 ||
      boundary.character !== 0
    ) {
      return null;
    }
    authoredEnd = boundary;
  } else {
    // The authored exclusive end is one past the authored position of the last content column.
    const lastIncluded = projectGeneratedPosition(map, {
      line: generated.end.line,
      character: generated.end.character - 1,
    });
    if (lastIncluded === null || lastIncluded.source !== sourceName) return null;
    authoredEnd = {
      source: sourceName,
      line: lastIncluded.line,
      character: lastIncluded.character + 1,
    };
  }

  return {
    source: sourceName,
    range: { start: startPos, end: { line: authoredEnd.line, character: authoredEnd.character } },
  };
}
