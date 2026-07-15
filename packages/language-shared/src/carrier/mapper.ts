import { TraceMap, decodedMappings, type SourceMapInput } from "@jridgewell/trace-mapping";
import { normalizePath } from "./naming";

// The STRICT fail-closed V3 position mapper — ONE mapper shared by every
// instantiation (the Node tsserver plugin and the WASM in-context
// LanguageService), so carrier position mapping never forks per host. This
// module is BROWSER-SAFE: no Node builtin imports.
//
// The strictness contract (the anti-snap rules):
//
// - Positions are **UTF-16 code units** (the V3 sourcemap column contract and
//   the native JS string index), never bytes and never code points.
// - A generated offset resolves through a GREATEST-LOWER-BOUND segment lookup
//   on its own generated line — NEVER a forward / least-upper-bound snap, and
//   NEVER a nearest-segment heuristic across lines.
// - A query in UNMAPPED generated space fails closed (`null`): a line with no
//   segments, a position before the line's first segment, a sourceless
//   segment's extent, or a position past the line's content extent
//   (anti-extrapolation — the mapper never invents a source position past the
//   mapped token).
// - A span maps only when BOTH endpoints map into the SAME source without
//   inverting; otherwise the whole span drops. A workspace edit maps only when
//   EVERY span maps; otherwise the whole edit is suppressed. No partial spans,
//   no partially-mapped edits.

/** A generated-side span in offset form, end-exclusive. */
export interface GeneratedSpan {
  start: number;
  end: number;
}

/** A strictly-mapped source position (offset + 1-based line, 0-based column). */
export interface MappedSourcePosition {
  /** The source name exactly as the map's `sources` entry spells it. */
  source: string;
  /** UTF-16 code-unit offset into the source text. */
  offset: number;
  /** 1-based source line. */
  line: number;
  /** 0-based UTF-16 source column. */
  column: number;
}

/** A strictly-mapped source span (same source for both endpoints, end-exclusive). */
export interface MappedSourceSpan {
  source: string;
  start: number;
  end: number;
}

/** A strictly-mapped generated position (offset + 1-based line, 0-based column). */
export interface MappedGeneratedPosition {
  /** UTF-16 code-unit offset into the generated carrier text. */
  offset: number;
  /** 1-based generated line. */
  line: number;
  /** 0-based UTF-16 generated column. */
  column: number;
}

/** One file's spans inside a cross-file workspace edit. */
export interface WorkspaceEditFileSpans {
  carrierPath: string;
  spans: readonly GeneratedSpan[];
}

/** One file's strictly-mapped spans inside a mapped workspace edit. */
export interface MappedWorkspaceEditFileSpans {
  carrierPath: string;
  spans: MappedSourceSpan[];
}

export interface CarrierMapperOptions {
  /**
   * The carrier's V3 source map: a pre-built `TraceMap`, the JSON string, or
   * the parsed map object. Invalid input throws — callers reading maps from an
   * untrusted channel parse/validate first (the store remap layer caches a
   * `TraceMap | null` per content-addressed `map_hash`).
   */
  map: TraceMap | SourceMapInput | unknown;
  /** The generated carrier text the offsets index (the published blob). */
  generatedText: string;
  /**
   * Read a source file's text by the map's source name (for the inverse
   * line/column-to-offset conversion). Consulted FIRST; when it yields nothing
   * the map's own `sourcesContent` (the exact bytes the mappings were produced
   * against) is the fallback. With neither, mapping fails closed.
   */
  readSourceText?: (source: string) => string | undefined;
}

/** Offsets of each line start (UTF-16 code units), lines split on `\n`. */
function buildLineStarts(text: string): number[] {
  const starts = [0];
  for (let i = 0; i < text.length; i += 1) {
    if (text.charCodeAt(i) === 10 /* \n */) {
      starts.push(i + 1);
    }
  }
  return starts;
}

/** The largest index `i` with `starts[i] <= offset` (offset is in range). */
function lineForOffset(starts: number[], offset: number): number {
  let lo = 0;
  let hi = starts.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (starts[mid] <= offset) {
      lo = mid;
    } else {
      hi = mid - 1;
    }
  }
  return lo;
}

/**
 * The line's CONTENT length in UTF-16 code units — excluding its `\n` / `\r\n`
 * terminator. Column `contentLength` is the line's exclusive one-past-last-char
 * boundary (a valid span endpoint); any column beyond it is terminator space.
 */
function lineContentLength(text: string, starts: number[], line: number): number {
  const start = starts[line];
  let end = line + 1 < starts.length ? starts[line + 1] : text.length;
  if (end > start && text.charCodeAt(end - 1) === 10 /* \n */) {
    end -= 1;
  }
  if (end > start && text.charCodeAt(end - 1) === 13 /* \r */) {
    end -= 1;
  }
  return end - start;
}

/**
 * GREATEST-LOWER-BOUND segment lookup: the largest index `i` with
 * `segments[i][0] <= genCol`, or `-1` when every segment starts after
 * `genCol`. Duplicate generated columns resolve to the LAST duplicate.
 */
function glbSegmentIndex(segments: ReadonlyArray<ReadonlyArray<number>>, genCol: number): number {
  let lo = 0;
  let hi = segments.length - 1;
  if (hi < 0 || segments[0][0] > genCol) {
    return -1;
  }
  while (lo < hi) {
    const mid = (lo + hi + 1) >> 1;
    if (segments[mid][0] <= genCol) {
      lo = mid;
    } else {
      hi = mid - 1;
    }
  }
  return lo;
}

/** One forward-index row: a mapped segment keyed from its SOURCE position. */
interface ForwardSegment {
  srcCol: number;
  genLine: number;
  genCol: number;
  /** The segment's generated extent (to the next segment / line boundary). */
  genExtent: number;
}

/**
 * The strict fail-closed mapper for ONE carrier: its V3 map + its generated
 * text. See the module doc for the strictness contract. Construction
 * precomputes the generated line-start table; source line tables build lazily
 * per source index on first demand.
 */
export class CarrierMapper {
  private readonly map: TraceMap;
  private readonly generatedText: string;
  private readonly generatedLineStarts: number[];
  private readonly readSourceText: ((source: string) => string | undefined) | undefined;
  /** Per-source-index resolved text + line table (`null` = known-unavailable). */
  private readonly sourceTables = new Map<number, { text: string; starts: number[] } | null>();
  /**
   * Lazy forward index: per source index, per source line, the mapped
   * segments as `{ srcCol, genLine, genCol, genExtent }` sorted by
   * `srcCol` then generated position. Built once on first forward demand.
   */
  private forwardIndex: Map<number, Map<number, ForwardSegment[]>> | null = null;

  constructor(options: CarrierMapperOptions) {
    this.map =
      options.map instanceof TraceMap ? options.map : new TraceMap(options.map as SourceMapInput);
    this.generatedText = options.generatedText;
    this.generatedLineStarts = buildLineStarts(options.generatedText);
    this.readSourceText = options.readSourceText;
  }

  /**
   * Map a generated UTF-16 offset to its strict source position, or `null`
   * when the position is unmapped (fail closed — NEVER a snap):
   *
   * 1. `offset` converts to `(genLine, genCol)` via the line-start table.
   * 2. The greatest-lower-bound segment `s` on `genLine` is looked up: the one
   *    with the largest `s.genCol <= genCol`. No segment on the line, or no
   *    segment at-or-before `genCol` (a least-upper-bound forward snap is
   *    FORBIDDEN) — `null`.
   * 3. `s` without source fields (a sourceless extent closer) — `null`.
   * 4. Anti-extrapolation extent bound: with `nextGenCol` = the next segment's
   *    generated column on the SAME line, or one past the line's content
   *    boundary — `genCol >= nextGenCol` means the query lies beyond `s`'s own
   *    generated extent (unmapped space after the token) — `null`. The line's
   *    exclusive one-past-last-char boundary itself maps (it is a legitimate
   *    end-exclusive span endpoint over mapped characters); positions in the
   *    terminator or beyond never do.
   * 5. Otherwise `delta = genCol - s.genCol` and the source position is
   *    `(s.srcLine, s.srcCol + delta)`, converted to a source offset against
   *    the source text — which must exist and contain that position, else
   *    `null`.
   */
  mapGeneratedOffsetToSource(offset: number): MappedSourcePosition | null {
    if (!Number.isInteger(offset) || offset < 0 || offset > this.generatedText.length) {
      return null;
    }
    const genLine = lineForOffset(this.generatedLineStarts, offset);
    const genCol = offset - this.generatedLineStarts[genLine];

    const decoded = decodedMappings(this.map);
    const segments = decoded[genLine];
    if (segments === undefined || segments.length === 0) {
      return null;
    }
    const idx = glbSegmentIndex(segments, genCol);
    if (idx < 0) {
      return null;
    }
    const segment = segments[idx];
    if (segment.length === 1) {
      // A sourceless segment: the preceding token's extent is closed and this
      // region has no source origin.
      return null;
    }

    const lineEnd = lineContentLength(this.generatedText, this.generatedLineStarts, genLine);
    if (genCol > lineEnd) {
      // The query sits on the line terminator (or beyond a malformed
      // segment) — never extrapolate past the line's content.
      return null;
    }
    const nextGenCol = idx + 1 < segments.length ? segments[idx + 1][0] : lineEnd + 1;
    if (genCol >= nextGenCol) {
      return null;
    }

    const delta = genCol - segment[0];
    const sourceIndex = segment[1];
    const srcLine = segment[2];
    const srcCol = segment[3] + delta;

    const source = this.map.sources[sourceIndex];
    if (source === null || source === undefined) {
      return null;
    }
    const table = this.sourceTableFor(sourceIndex, source);
    if (table === null) {
      return null;
    }
    if (srcLine >= table.starts.length) {
      return null;
    }
    const srcLineEnd = lineContentLength(table.text, table.starts, srcLine);
    if (srcCol > srcLineEnd) {
      // The map claims a position past the source line's content — a
      // map/text mismatch. Fail closed rather than emit a corrupt offset.
      return null;
    }
    return {
      source,
      offset: table.starts[srcLine] + srcCol,
      line: srcLine + 1,
      column: srcCol,
    };
  }

  /**
   * Map an end-exclusive generated span. BOTH endpoints must map, into the
   * SAME source, without inverting — otherwise the whole span drops (`null`).
   * No partial spans.
   */
  mapGeneratedSpanToSource(start: number, end: number): MappedSourceSpan | null {
    if (end < start) {
      return null;
    }
    const mappedStart = this.mapGeneratedOffsetToSource(start);
    if (mappedStart === null) {
      return null;
    }
    const mappedEnd = start === end ? mappedStart : this.mapGeneratedOffsetToSource(end);
    if (mappedEnd === null) {
      return null;
    }
    if (mappedStart.source !== mappedEnd.source || mappedEnd.offset < mappedStart.offset) {
      return null;
    }
    return { source: mappedStart.source, start: mappedStart.offset, end: mappedEnd.offset };
  }

  /**
   * Map EVERY span of a rename / code-action edit within this carrier. If ANY
   * span fails to map, the WHOLE edit is suppressed (`null`) — a
   * partially-mapped edit is never returned.
   */
  mapWorkspaceEditToSource(spans: readonly GeneratedSpan[]): MappedSourceSpan[] | null {
    const out: MappedSourceSpan[] = [];
    for (const span of spans) {
      const mapped = this.mapGeneratedSpanToSource(span.start, span.end);
      if (mapped === null) {
        return null;
      }
      out.push(mapped);
    }
    return out;
  }

  /**
   * Map a source UTF-16 offset to its strict GENERATED position, or `null`
   * when the position is unmapped (fail closed — NEVER a snap). The forward
   * twin of [`mapGeneratedOffsetToSource`] with the same strictness contract:
   *
   * 1. `source` selects the map source by exact name; with the parameter
   *    omitted, a single-source map uses that source and a multi-source map
   *    fails closed (`null`) — never a guess.
   * 2. `offset` converts to `(srcLine, srcCol)` against the source text
   *    (which must exist and contain the position, else `null`).
   * 3. The greatest-lower-bound segment ON THAT SOURCE LINE is looked up:
   *    the largest `s.srcCol <= srcCol`. No segment on the line, or none
   *    at-or-before the column (a forward snap is FORBIDDEN) — `null`.
   * 4. `delta = srcCol - s.srcCol` must stay inside the segment's OWN
   *    generated extent (`delta < s.genExtent`) — no delta extrapolation
   *    past the mapped token. Every GLB duplicate (a source token emitted
   *    at several generated positions) is tried; failures drop.
   * 5. Among the valid candidates, the EARLIEST generated position wins —
   *    deterministic, not distance-based.
   */
  mapSourceOffsetToGenerated(offset: number, source?: string): MappedGeneratedPosition | null {
    const sourceIndex = this.resolveSourceIndex(source);
    if (sourceIndex === null) {
      return null;
    }
    const sourceName = this.map.sources[sourceIndex];
    if (sourceName === null || sourceName === undefined) {
      return null;
    }
    const table = this.sourceTableFor(sourceIndex, sourceName);
    if (table === null) {
      return null;
    }
    if (!Number.isInteger(offset) || offset < 0 || offset > table.text.length) {
      return null;
    }
    const srcLine = lineForOffset(table.starts, offset);
    const srcCol = offset - table.starts[srcLine];
    if (srcCol > lineContentLength(table.text, table.starts, srcLine)) {
      // The query sits on the source line terminator — unmapped space.
      return null;
    }

    const lineSegments = this.forwardIndexFor(sourceIndex).get(srcLine);
    if (lineSegments === undefined || lineSegments.length === 0) {
      return null;
    }
    // Greatest lower bound on the source column (the list is sorted by
    // srcCol): the largest srcCol <= query.
    let glbCol = -1;
    for (const segment of lineSegments) {
      if (segment.srcCol <= srcCol) {
        glbCol = Math.max(glbCol, segment.srcCol);
      } else {
        break;
      }
    }
    if (glbCol < 0) {
      return null;
    }
    let best: MappedGeneratedPosition | null = null;
    for (const segment of lineSegments) {
      if (segment.srcCol !== glbCol) continue;
      const delta = srcCol - segment.srcCol;
      if (delta >= segment.genExtent) {
        // Past the mapped token's generated extent — never extrapolate.
        continue;
      }
      const genColumn = segment.genCol + delta;
      const genOffset = this.generatedLineStarts[segment.genLine] + genColumn;
      if (
        best === null ||
        segment.genLine + 1 < best.line ||
        (segment.genLine + 1 === best.line && genColumn < best.column)
      ) {
        best = { offset: genOffset, line: segment.genLine + 1, column: genColumn };
      }
    }
    return best;
  }

  /** The map source index a forward query targets (see step 1 above). */
  private resolveSourceIndex(source: string | undefined): number | null {
    const sources = this.map.sources;
    if (source === undefined) {
      return sources.length === 1 ? 0 : null;
    }
    const index = sources.indexOf(source);
    return index === -1 ? null : index;
  }

  /** Build (once) and return the forward segment index for a source. */
  private forwardIndexFor(sourceIndex: number): Map<number, ForwardSegment[]> {
    if (this.forwardIndex === null) {
      const index = new Map<number, Map<number, ForwardSegment[]>>();
      const decoded = decodedMappings(this.map);
      for (let genLine = 0; genLine < decoded.length; genLine += 1) {
        const segments = decoded[genLine];
        const lineEnd = lineContentLength(this.generatedText, this.generatedLineStarts, genLine);
        for (let i = 0; i < segments.length; i += 1) {
          const segment = segments[i];
          if (segment.length === 1) continue; // sourceless closer
          const genCol = segment[0];
          const nextGenCol = i + 1 < segments.length ? segments[i + 1][0] : lineEnd + 1;
          const genExtent = nextGenCol - genCol;
          if (genExtent <= 0) continue;
          const srcIdx = segment[1];
          const srcLine = segment[2];
          const srcCol = segment[3];
          let perSource = index.get(srcIdx);
          if (perSource === undefined) {
            perSource = new Map();
            index.set(srcIdx, perSource);
          }
          let perLine = perSource.get(srcLine);
          if (perLine === undefined) {
            perLine = [];
            perSource.set(srcLine, perLine);
          }
          perLine.push({ srcCol, genLine, genCol, genExtent });
        }
      }
      for (const perSource of index.values()) {
        for (const perLine of perSource.values()) {
          perLine.sort(
            (a, b) => a.srcCol - b.srcCol || a.genLine - b.genLine || a.genCol - b.genCol,
          );
        }
      }
      this.forwardIndex = index;
    }
    return this.forwardIndex.get(sourceIndex) ?? new Map();
  }

  private sourceTableFor(
    sourceIndex: number,
    source: string,
  ): { text: string; starts: number[] } | null {
    const cached = this.sourceTables.get(sourceIndex);
    if (cached !== undefined) {
      return cached;
    }
    let text = this.readSourceText?.(source);
    if (text === undefined) {
      const embedded = this.map.sourcesContent?.[sourceIndex];
      text = typeof embedded === "string" ? embedded : undefined;
    }
    const table = text === undefined ? null : { text, starts: buildLineStarts(text) };
    this.sourceTables.set(sourceIndex, table);
    return table;
  }
}

/**
 * The per-carrier mapper registry: cross-file provider results map through
 * their OWN file's mapper (keyed by the carrier companion path, normalized),
 * never a single "active file" mapper.
 */
export class CarrierMapperSet {
  private readonly mappers = new Map<string, CarrierMapper>();

  set(carrierPath: string, mapper: CarrierMapper): void {
    this.mappers.set(normalizePath(carrierPath), mapper);
  }

  delete(carrierPath: string): void {
    this.mappers.delete(normalizePath(carrierPath));
  }

  clear(): void {
    this.mappers.clear();
  }

  /**
   * The mapper for a carrier companion path, or `undefined` for an unknown
   * carrier — the caller then DROPS the result (fail closed), never maps it
   * through another file's mapper.
   */
  forCarrier(carrierPath: string): CarrierMapper | undefined {
    return this.mappers.get(normalizePath(carrierPath));
  }

  /**
   * Map a cross-file workspace edit atomically: every file's every span must
   * map through that file's own mapper. An unknown carrier file or ANY
   * unmappable span suppresses the WHOLE edit (`null`).
   */
  mapWorkspaceEditToSource(
    edit: readonly WorkspaceEditFileSpans[],
  ): MappedWorkspaceEditFileSpans[] | null {
    const out: MappedWorkspaceEditFileSpans[] = [];
    for (const file of edit) {
      const mapper = this.forCarrier(file.carrierPath);
      if (mapper === undefined) {
        return null;
      }
      const spans = mapper.mapWorkspaceEditToSource(file.spans);
      if (spans === null) {
        return null;
      }
      out.push({ carrierPath: file.carrierPath, spans });
    }
    return out;
  }
}
