// Coordinate model, segment representation, and the accepted lookup.
//
// Implements LAYER 1 §2.1 (coordinate model), §2.2 (segment), §2.3
// (`resolveAt`), §2.6 (`payloadAt`), and §5.2 (position conversion and the
// boundary lemma) of
// `spec/assembled-map-composition-layer1.md`.
//
// OFFSETS ARE UTF-16 CODE-UNIT INDICES HERE, WHERE §5.2 WRITES BYTE OFFSETS.
// §5.2 defines `pos(o)` / `off(line, column)` over UTF-8 byte offsets because
// the producer it cites is Rust. Every offset this algebra ever forms is a
// character boundary — §5.2's boundary lemma: chunk starts/ends are 0, len, or
// an edit boundary of a pure-ASCII pattern (§2.5), and input-segment offsets
// come from `off(line, column)` after `U7.3` rejected the one wire column that
// addresses no character boundary. On that domain the UTF-8-byte-offset and
// UTF-16-code-unit-index orderings are the same order and induce the same
// `(line, column)`: a line is delimited by LF either way, and §2.1 already
// defines the column as a count of UTF-16 code units. So the observable
// coordinate model is identical, and JavaScript's native index space is used
// directly rather than round-tripped through a byte encoding.

/**
 * §2.1 — the line table. Splits on `U+000A` only and RETAINS any preceding
 * `U+000D` inside the line's text, so a text ending in LF has a final, empty
 * line. This is the accepted harness semantics (`src/mapping-oracle.mjs`
 * `lineTable`).
 */
export function lineTable(text) {
  return text.split("\n");
}

/**
 * Precomputed geometry of one text: its line table plus each line's start
 * offset, so `pos` and `off` are O(log n) / O(1) rather than O(n).
 */
export class TextGeometry {
  constructor(text) {
    this.text = text;
    this.lines = lineTable(text);
    this.lineStart = new Array(this.lines.length);
    let offset = 0;
    for (let line = 0; line < this.lines.length; line += 1) {
      this.lineStart[line] = offset;
      offset += this.lines[line].length + 1; // + the LF separator
    }
  }

  /** §2.1 — `0 ≤ line < lineTable.length ∧ 0 ≤ column ≤ lineTable[line].length`. */
  isInBounds(line, column) {
    if (!Number.isInteger(line) || !Number.isInteger(column)) return false;
    if (line < 0 || line >= this.lines.length) return false;
    return column >= 0 && column <= this.lines[line].length;
  }

  /** §5.2 — `off(line, column)`. The position must be in-bounds. */
  offsetOf(line, column) {
    return this.lineStart[line] + column;
  }

  /** §5.2 — `pos(o)`. */
  positionOf(offset) {
    // Binary search for the last line whose start is ≤ offset.
    let low = 0;
    let high = this.lineStart.length - 1;
    while (low < high) {
      const mid = (low + high + 1) >> 1;
      if (this.lineStart[mid] <= offset) low = mid;
      else high = mid - 1;
    }
    return { line: low, column: offset - this.lineStart[low] };
  }

  /** The offset one past the last code unit — rule (d)'s position (§5.3). */
  get endOffset() {
    return this.text.length;
  }
}

/** §2.2 — a sourceless segment: all four authored fields null. */
export function sourcelessPayload() {
  return { srcIdx: null, srcLine: null, srcCol: null, nameIdx: null };
}

/** §2.2 — a segment's payload `(srcIdx, srcLine, srcCol, nameIdx)`. */
export function payloadOf(segment) {
  return {
    srcIdx: segment.srcIdx,
    srcLine: segment.srcLine,
    srcCol: segment.srcCol,
    nameIdx: segment.nameIdx,
  };
}

/** §2.2 — a segment whose `srcIdx` is null is sourceless. */
export function isSourceless(segment) {
  return segment.srcIdx === null;
}

/**
 * §2.3 — the accepted lookup.
 *
 * > Among the segments of `S` on `line`, ordered by `genCol` ascending and, at
 * > equal `genCol`, by their order in `S`, take the LAST whose
 * > `genCol ≤ column`. If there is none, the result is absent.
 *
 * It is line-scoped (no fall-through to a previous line) and NOT
 * sourceless-transparent (a sourceless segment is a legitimate result — the
 * sourceless barrier of §5.4).
 *
 * `index` (a `SegmentIndex`) is the per-line bucketing; passing the raw
 * sequence is also accepted.
 */
export function resolveAt(index, line, column) {
  const onLine = index instanceof SegmentIndex ? index.onLine(line) : bucketLine(index, line);
  if (onLine === undefined) return null;
  let found = null;
  for (const segment of onLine) {
    if (segment.genCol <= column) found = segment;
    else break;
  }
  return found;
}

/**
 * §2.6 — the observable. The authored payload at a generated position: the
 * resolved segment's payload when it is SOURCE-BEARING, and the single
 * distinguished value `Unmapped` in BOTH other cases (absent, and a present
 * sourceless segment).
 */
export const UNMAPPED = Symbol("Unmapped");

export function payloadAt(index, line, column) {
  const resolved = resolveAt(index, line, column);
  if (resolved === null || isSourceless(resolved)) return UNMAPPED;
  return payloadOf(resolved);
}

/** Per-line bucketing for `resolveAt`, built once per sequence. */
export class SegmentIndex {
  constructor(segments) {
    this.byLine = new Map();
    for (const segment of segments) {
      let bucket = this.byLine.get(segment.genLine);
      if (bucket === undefined) {
        bucket = [];
        this.byLine.set(segment.genLine, bucket);
      }
      bucket.push(segment);
    }
    // §2.3: "ordered by `genCol` ascending and, at equal `genCol`, by their
    // order in `S`". Array.prototype.sort is stable, so sequence order survives
    // a tie. On every sequence this algebra forms the sort is a no-op (`U3.6`
    // and §5.5 make each line non-decreasing); it is applied anyway so the
    // lookup is literally §2.3 rather than §2.3-given-an-assumption.
    for (const bucket of this.byLine.values()) bucket.sort((a, b) => a.genCol - b.genCol);
  }

  onLine(line) {
    return this.byLine.get(line);
  }
}

function bucketLine(segments, line) {
  const onLine = segments.filter((segment) => segment.genLine === line);
  if (onLine.length === 0) return undefined;
  return onLine.sort((a, b) => a.genCol - b.genCol);
}

/**
 * §6.3 / §5.3 — `advance_generated_position`: LF increments the line and resets
 * the column; every other code unit advances the column by its UTF-16 length
 * (which is 1 per code unit).
 */
export function advance(position, text) {
  let { line, column } = position;
  for (let i = 0; i < text.length; i += 1) {
    if (text.charCodeAt(i) === 10) {
      line += 1;
      column = 0;
    } else {
      column += 1;
    }
  }
  return { line, column };
}
