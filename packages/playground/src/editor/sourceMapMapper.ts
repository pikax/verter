/**
 * Source map position mapper for TSX ↔ Vue offset conversion.
 * Parses standard V3 source maps with VLQ-encoded mappings.
 */

interface SourceMapV3 {
  version: number;
  sources: string[];
  mappings: string;
  names?: string[];
  sourcesContent?: (string | null)[];
}

/** A single decoded mapping segment: [genCol, sourceIdx, sourceLine, sourceCol] */
type Segment = [number, number, number, number];

/** Decoded mappings: array of lines, each containing segments */
type DecodedMappings = Segment[][];

const VLQ_CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const VLQ_LOOKUP = new Uint8Array(128);
for (let i = 0; i < VLQ_CHARS.length; i++) {
  VLQ_LOOKUP[VLQ_CHARS.charCodeAt(i)] = i;
}

function decodeVlq(encoded: string): DecodedMappings {
  const lines: Segment[][] = [];
  let currentLine: Segment[] = [];

  // State for relative values
  let genCol = 0;
  let sourceIdx = 0;
  let sourceLine = 0;
  let sourceCol = 0;

  let i = 0;
  while (i < encoded.length) {
    const ch = encoded.charCodeAt(i);

    if (ch === 59) {
      // ';' — new line
      lines.push(currentLine);
      currentLine = [];
      genCol = 0;
      i++;
      continue;
    }

    if (ch === 44) {
      // ',' — next segment
      i++;
      continue;
    }

    // Decode a segment (1, 4, or 5 fields; we only care about 4-field segments)
    const fields: number[] = [];
    while (i < encoded.length) {
      const c = encoded.charCodeAt(i);
      if (c === 59 || c === 44) break;

      // Decode one VLQ integer
      let value = 0;
      let shift = 0;
      let cont = true;
      while (cont && i < encoded.length) {
        const digit = VLQ_LOOKUP[encoded.charCodeAt(i)];
        i++;
        cont = (digit & 32) !== 0;
        value += (digit & 31) << shift;
        shift += 5;
      }
      // Sign bit is in lowest bit
      fields.push(value & 1 ? -(value >> 1) : value >> 1);
    }

    if (fields.length >= 4) {
      genCol += fields[0];
      sourceIdx += fields[1];
      sourceLine += fields[2];
      sourceCol += fields[3];
      currentLine.push([genCol, sourceIdx, sourceLine, sourceCol]);
    } else if (fields.length >= 1) {
      genCol += fields[0];
      // No source mapping for this segment
    }
  }

  lines.push(currentLine);
  return lines;
}

export class SourceMapMapper {
  private mappings: DecodedMappings;
  private tsxLineOffsets: number[];
  private vueLineOffsets: number[];

  constructor(
    sourceMapJson: string,
    private tsxCode: string,
    private vueCode: string,
  ) {
    const map: SourceMapV3 = JSON.parse(sourceMapJson);
    this.mappings = decodeVlq(map.mappings);
    this.tsxLineOffsets = computeLineOffsets(tsxCode);
    this.vueLineOffsets = computeLineOffsets(vueCode);
  }

  /** Map a TSX byte offset to a Vue byte offset. Returns null if no mapping found. */
  tsxOffsetToVueOffset(tsxOffset: number): number | null {
    const { line: tsxLine, col: tsxCol } = this.offsetToLineCol(tsxOffset, this.tsxLineOffsets);

    if (tsxLine >= this.mappings.length) return null;
    const segments = this.mappings[tsxLine];
    if (segments.length === 0) return null;

    // Find the closest segment whose genCol <= tsxCol
    let best: Segment | null = null;
    for (const seg of segments) {
      if (seg[0] <= tsxCol) {
        best = seg;
      } else {
        break;
      }
    }

    if (!best) return null;

    const [genCol, , srcLine, srcCol] = best;
    const delta = tsxCol - genCol;
    const vueCol = srcCol + delta;

    return this.lineColToOffset(srcLine, vueCol, this.vueLineOffsets);
  }

  /** Map a Vue byte offset to a TSX byte offset. Returns null if no mapping found. */
  vueOffsetToTsxOffset(vueOffset: number): number | null {
    const { line: vueLine, col: vueCol } = this.offsetToLineCol(vueOffset, this.vueLineOffsets);

    // Search all mappings for one that maps from this source position
    let bestDist = Infinity;
    let bestTsxLine = -1;
    let bestTsxCol = -1;

    for (let genLine = 0; genLine < this.mappings.length; genLine++) {
      for (const seg of this.mappings[genLine]) {
        const [genCol, , srcLine, srcCol] = seg;
        if (srcLine === vueLine) {
          const dist = Math.abs(srcCol - vueCol);
          if (dist < bestDist) {
            bestDist = dist;
            bestTsxLine = genLine;
            bestTsxCol = genCol + (vueCol - srcCol);
          }
        }
      }
    }

    if (bestTsxLine === -1) return null;
    return this.lineColToOffset(bestTsxLine, bestTsxCol, this.tsxLineOffsets);
  }

  private offsetToLineCol(offset: number, lineOffsets: number[]): { line: number; col: number } {
    let line = 0;
    for (let i = 1; i < lineOffsets.length; i++) {
      if (lineOffsets[i] > offset) break;
      line = i;
    }
    return { line, col: offset - lineOffsets[line] };
  }

  private lineColToOffset(line: number, col: number, lineOffsets: number[]): number {
    if (line >= lineOffsets.length) return lineOffsets[lineOffsets.length - 1];
    return lineOffsets[line] + Math.max(0, col);
  }
}

function computeLineOffsets(text: string): number[] {
  const offsets = [0];
  for (let i = 0; i < text.length; i++) {
    if (text.charCodeAt(i) === 10) {
      offsets.push(i + 1);
    }
  }
  return offsets;
}
