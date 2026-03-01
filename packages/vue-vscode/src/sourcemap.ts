/**
 * Source map VLQ decoder and bidirectional lookup utilities.
 * Extracted from packages/playground/src/core/sourcemap.ts for use in the VS Code extension.
 */

const VLQ_CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const VLQ_LOOKUP = new Map<string, number>();
for (let i = 0; i < VLQ_CHARS.length; i++) VLQ_LOOKUP.set(VLQ_CHARS[i]!, i);

/** Decode a single VLQ value from `str` starting at `index`. Returns [value, newIndex]. */
function decodeVLQValue(str: string, index: number): [number, number] {
  let result = 0;
  let shift = 0;
  let i = index;
  // eslint-disable-next-line no-constant-condition
  while (true) {
    const ch = str[i];
    const digit = VLQ_LOOKUP.get(ch!);
    if (digit === undefined) throw new Error(`Invalid VLQ character: ${ch}`);
    i++;
    result += (digit & 31) << shift;
    shift += 5;
    if ((digit & 32) === 0) break;
  }
  const negate = (result & 1) !== 0;
  result >>= 1;
  return [negate ? -result : result, i];
}

/**
 * A source map segment: [genCol, sourceIdx, srcLine, srcCol, nameIdx?]
 */
type Segment =
  | [number, number, number, number]
  | [number, number, number, number, number];

/** Parse a VLQ mappings string into an array of lines, each containing segments. */
function parseMappings(mappings: string): Segment[][] {
  if (!mappings) return [];

  const lines: Segment[][] = [];
  const groups = mappings.split(";");

  let srcLine = 0;
  let srcCol = 0;
  let sourceIdx = 0;
  let nameIdx = 0;

  for (const group of groups) {
    const segments: Segment[] = [];
    let genCol = 0;

    if (group.length > 0) {
      const parts = group.split(",");
      for (const part of parts) {
        if (part.length === 0) continue;
        let i = 0;
        const values: number[] = [];
        while (i < part.length) {
          const [val, next] = decodeVLQValue(part, i);
          values.push(val);
          i = next;
        }

        if (values.length >= 4) {
          genCol += values[0]!;
          sourceIdx += values[1]!;
          srcLine += values[2]!;
          srcCol += values[3]!;
          if (values.length >= 5) {
            nameIdx += values[4]!;
            segments.push([genCol, sourceIdx, srcLine, srcCol, nameIdx]);
          } else {
            segments.push([genCol, sourceIdx, srcLine, srcCol]);
          }
        } else if (values.length === 1) {
          genCol += values[0]!;
        }
      }
    }

    lines.push(segments);
  }
  return lines;
}

interface SourceMapJson {
  version: number;
  sources: string[];
  names: string[];
  mappings: string;
}

function tryParseMap(json: string): SourceMapJson | null {
  if (!json) return null;
  try {
    return JSON.parse(json) as SourceMapJson;
  } catch {
    return null;
  }
}

export interface MappedPosition {
  line: number; // 0-based
  col: number; // 0-based
}

/** Forward lookup: given a source position, find the generated position. */
export function lookupGenerated(
  mapJson: string,
  srcLine: number,
  srcCol: number,
): MappedPosition | null {
  const parsed = tryParseMap(mapJson);
  if (!parsed) return null;

  const segments = parseMappings(parsed.mappings);
  let best: { line: number; col: number; dist: number } | null = null;

  for (let genLine = 0; genLine < segments.length; genLine++) {
    for (const seg of segments[genLine]!) {
      if (seg[2] === srcLine) {
        const dist = Math.abs(seg[3] - srcCol);
        if (
          !best ||
          dist < best.dist ||
          (dist === best.dist && genLine < best.line)
        ) {
          best = { line: genLine, col: seg[0], dist };
        }
      }
    }
  }

  return best ? { line: best.line, col: best.col } : null;
}

/** Reverse lookup: given a generated position, find the source position. */
export function lookupSource(
  mapJson: string,
  genLine: number,
  genCol: number,
): MappedPosition | null {
  const parsed = tryParseMap(mapJson);
  if (!parsed) return null;

  const segments = parseMappings(parsed.mappings);
  if (genLine >= segments.length) return null;

  const lineSegs = segments[genLine]!;
  if (lineSegs.length === 0) return null;

  // Find the segment whose genCol is <= the requested genCol
  let lo = 0;
  let hi = lineSegs.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >>> 1;
    if (lineSegs[mid]![0] <= genCol) lo = mid;
    else hi = mid - 1;
  }

  const seg = lineSegs[lo]!;
  if (seg[0] > genCol) return null;

  return { line: seg[2], col: seg[3] };
}
