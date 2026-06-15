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

  /**
   * The exclusive-end `character` of a generated line: the count of UTF-16 code units of
   * CONTENT on `line`, excluding its line terminator. Returns `null` when `line` is out of
   * range. Reuses the underlying {@link DocumentPositions} line-end clamp (which strips a
   * trailing `\n` / `\r` / `\r\n`) rather than a second walker — the span from the line
   * start to the over-long-character clamp is exactly the line's content length. The
   * column-0 range reconstruction uses this to bound a content line by its REAL generated
   * length instead of fabricating an authored span across the line break.
   */
  lineEndCharacter(line: number): number | null {
    if (!Number.isInteger(line) || line < 0 || line >= this.positions.lineCount) return null;
    const start = this.positions.positionToUtf16({ line, character: 0 }, "utf-16");
    const end = this.positions.positionToUtf16(
      { line, character: Number.MAX_SAFE_INTEGER },
      "utf-16",
    );
    return end - start;
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
 * One-shot convenience: fold a baseline `[start, end)` UTF-8 byte range over the
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
 * Project the CONTENT of a generated range `[startGen, endGen)` to its authored EXCLUSIVE-end
 * position, confirming the whole content is ONE contiguous same-source 1:1 mapping. Returns `null`
 * when any content position is source-less, maps to a different source than `startGen`, breaks
 * within-line authored-column contiguity, or breaks authored-LINE continuity across a generated
 * line break — any of which means no single authored range faithfully represents the generated
 * content, so the span is reported as unprojectable rather than fabricated across the hole.
 *
 * The content positions are `[startGen .. endGen - 1]` flattened across generated lines: on the
 * start line from `startGen.character`, every fully-included intermediate line in whole (its
 * trailing segment extends to the line end), and the final content line up to its last content
 * column. When `endGen.character` is greater than 0 the final content line is `endGen.line` up to
 * `endGen.character - 1`. When `endGen.character` is 0 the content stops at the end of the previous
 * generated line, whose last content column is bounded by that line's REAL generated length (from
 * the {@link GeneratedDocument} the projector owns) — so no column of `endGen.line` is content. In
 * both cases the authored exclusive end is reconstructed one unit past the authored position of that
 * last included column. A column-0 end additionally validates that the boundary at `endGen` is a
 * faithful same-source line break (it must continue the authored line at column 0), but that
 * boundary is only a gate and never supplies the returned end.
 *
 * Coverage is verified by walking the segments of each generated content line: a segment opening a
 * new sub-run must keep the authored line and resume at the authored column the previous segment
 * interpolates to at that boundary, and the first segment of every content line after the start
 * must open on the next authored line at authored column 0 — a faithful copy advances exactly one
 * authored line per generated line break and restarts at that line's start, so a break onto a
 * non-adjacent authored line, or onto a column past the line start, is rejected rather than
 * fabricating the authored columns the generated content never covers.
 *
 * Limitation: this projector owns the generated artifact text and the source map, but not the
 * authored Vue text. It can bound fully included generated lines by their real generated line
 * length, but it cannot prove that a non-final generated line copied through the end of its authored
 * line. A generated break from authored (A,C) to authored (A+1,0) may therefore be a true authored
 * line break or a break after copying only a strict prefix of authored line A. This is recorded as
 * an authored-reporting limitation; the authored-text-aware generated-to-Vue projection layer must
 * close it by checking authored line lengths.
 */
function projectContentSpan(
  map: ParsedSourceMap,
  document: GeneratedDocument,
  startGen: Position,
  endGen: Position,
): OriginalPosition | null {
  const startSegment =
    startGen.line >= 0 && startGen.line < map.lines.length
      ? coveringSegment(map.lines[startGen.line], startGen.character)
      : null;
  if (startSegment === null || startSegment.source === null) return null;
  const sourceIndex = startSegment.source.index;

  const endsAtColumnZero = endGen.character === 0;
  // A column-0 exclusive end's content runs through the end of the previous generated line; a
  // real-column end's content runs through `endGen.character - 1` on the end line itself.
  const lastContentLine = endsAtColumnZero ? endGen.line - 1 : endGen.line;
  if (lastContentLine < startGen.line) return null; // empty or inverted content
  // The last content line's last content column. For a column-0 end it is bounded by that line's
  // REAL generated length (`lineEndCharacter - 1`) — NOT Infinity and NOT the authored position the
  // column-0 boundary maps to — so a generated line that copied only a prefix of its authored line
  // yields a tight authored end rather than fabricating the uncovered authored suffix. For a
  // real-column end it is the column one before the exclusive end on the end line itself.
  let lastContentColumn: number;
  if (endsAtColumnZero) {
    const lineEnd = document.lineEndCharacter(lastContentLine);
    if (lineEnd === null) return null; // the generated document has no such line — cannot bound
    lastContentColumn = lineEnd - 1;
  } else {
    lastContentColumn = endGen.character - 1;
  }

  let prevAuthoredLine = startSegment.source.line;
  for (let line = startGen.line; line <= lastContentLine; line++) {
    const segments = line >= 0 && line < map.lines.length ? map.lines[line] : [];
    const colFrom = line === startGen.line ? startGen.character : 0;
    // The final content line stops at its last content column; an earlier line runs to its end,
    // which the trailing segment already covers.
    const colTo = line === lastContentLine ? lastContentColumn : Infinity;

    const cover = coveringSegment(segments, colFrom);
    if (cover === null || cover.source === null || cover.source.index !== sourceIndex) {
      return null;
    }
    // Authored-line continuity across a generated line break: each content line after the start
    // must open one authored line past the previous content line's authored line AND at authored
    // column 0 — a faithful line break restarts the next authored line at its start, so a break
    // onto a non-adjacent authored line, or onto a column past that line's start (which would
    // fabricate the authored columns before it), is rejected.
    if (
      line > startGen.line &&
      (cover.source.line !== prevAuthoredLine + 1 || cover.source.column !== 0)
    ) {
      return null;
    }

    let prevSource: SegmentSource = cover.source;
    let prevGenColumn = cover.genColumn;
    for (const segment of segments) {
      if (segment.genColumn <= colFrom) continue; // already covered by `cover`
      if (segment.genColumn > colTo) break; // past this line's content
      if (segment.source === null || segment.source.index !== sourceIndex) return null;
      const expectedColumn = prevSource.column + (segment.genColumn - prevGenColumn);
      if (segment.source.line !== prevSource.line || segment.source.column !== expectedColumn) {
        return null;
      }
      prevSource = segment.source;
      prevGenColumn = segment.genColumn;
    }
    // Within a content line every segment shares one authored line (enforced above), so the
    // covering segment's authored line is this line's authored line.
    prevAuthoredLine = cover.source.line;
  }

  // A column-0 exclusive end additionally requires the generated line break to be a FAITHFUL
  // same-source wrap: the boundary at `endGen` must map to the start source and open the next
  // authored line (one past the last content line) at authored column 0. This rejects a
  // source-less, cross-source, line-jumping, or mid-line break rather than projecting across it.
  // The boundary is only a GATE — it does not supply the returned end (a faithful break to authored
  // (A+1,0) does not prove the generated line copied through the end of authored line A; that is
  // the authored-reporting limitation documented above).
  if (endsAtColumnZero) {
    const boundary = projectGeneratedPosition(map, endGen);
    if (
      boundary === null ||
      boundary.source !== map.sources[sourceIndex] ||
      boundary.line !== prevAuthoredLine + 1 ||
      boundary.character !== 0
    ) {
      return null;
    }
  }

  // The authored exclusive end is one past the authored position of the LAST ACTUAL generated
  // content column: `endGen.character - 1` for a real-column end, the real-length-bounded last
  // column for a column-0 end. Both reconstruct identically through the source map, so a column-0
  // end never returns the boundary's (A+1,0) position and a prefix-copied final line is bounded to
  // the columns it actually covered.
  const lastIncluded = projectGeneratedPosition(map, {
    line: lastContentLine,
    character: lastContentColumn,
  });
  if (lastIncluded === null) return null;
  return {
    source: lastIncluded.source,
    line: lastIncluded.line,
    character: lastIncluded.character + 1,
  };
}

/**
 * The paired inputs a generated → authored range projection reads: the V3 source map AND the
 * generated artifact's text. The projector owns BOTH — the map gives authored attribution per
 * generated column, and the {@link GeneratedDocument} gives each generated line's real length,
 * which bounds a content line to the columns generated content actually covers instead of
 * fabricating across a line break (see {@link projectContentSpan}). It does NOT own the authored
 * Vue text, which is the boundary of what it can prove (see the limitation on
 * {@link projectContentSpan}).
 */
export interface GeneratedProjection {
  readonly map: ParsedSourceMap;
  readonly document: GeneratedDocument;
}

/**
 * Project a generated {@link Range} back to an authored {@link OriginalRange}.
 * Returns `null` unless the range's CONTENT projects and resolves to a single SAME source — a
 * start and end in different sources, or content broken by a source-less / cross-source /
 * non-contiguous hole, is not a coherent single-source range and is reported as unprojectable
 * rather than fabricated.
 *
 * An LSP range is inclusive-start / EXCLUSIVE-end: its content is the positions `[start .. end-1]`,
 * and the exclusive end is one past the last content position. The range is therefore projected by
 * its content — EVERY content position must be covered by one contiguous same-source mapping, with
 * authored-column contiguity within a generated line and authored-line continuity across a line
 * break — NOT by projecting the exclusive end as a point. Projecting the exclusive end would both
 * reject a fully-mapped token range merely because its end abuts a generated-only (unmapped)
 * boundary (the shape verter emits for inserted content immediately after a copied token) AND
 * accept a range whose interior content hides an unmapped, cross-source, or line-jumping hole that
 * mapping resumes past before the end. One content walk ({@link projectContentSpan}) handles every
 * non-empty range — including one ending at the column-0 break of a later generated line — so there
 * is no endpoint-only path; the authored exclusive end is derived only after the full content span
 * validates as one contiguous same-source copy.
 */
export function projectGeneratedRange(
  projection: GeneratedProjection,
  generated: Range,
): OriginalRange | null {
  const { map, document } = projection;
  const start = projectGeneratedPosition(map, generated.start);
  if (start === null) return null;
  const startPos = { line: start.line, character: start.character };

  // A zero-width range is its single position: it projects exactly when the start does, and the
  // authored range is the zero-width point. It has no content positions to walk.
  if (
    generated.start.line === generated.end.line &&
    generated.start.character === generated.end.character
  ) {
    return { source: start.source, range: { start: startPos, end: startPos } };
  }

  const end = projectContentSpan(map, document, generated.start, generated.end);
  if (end === null) return null;
  return {
    source: start.source,
    range: { start: startPos, end: { line: end.line, character: end.character } },
  };
}
