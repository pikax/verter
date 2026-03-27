/**
 * Source map VLQ codec and combination utilities for the playground.
 *
 * Merges separate script and template source maps into a single map
 * covering the assembled JS output shown in the JS tab.
 */

// ── VLQ codec ──────────────────────────────────────────────────

const VLQ_CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const VLQ_LOOKUP = new Map<string, number>();
for (let i = 0; i < VLQ_CHARS.length; i++) VLQ_LOOKUP.set(VLQ_CHARS[i], i);

/** Decode a single VLQ value from `str` starting at `index`. Returns [value, newIndex]. */
function decodeVLQValue(str: string, index: number): [number, number] {
  let result = 0;
  let shift = 0;
  let i = index;
  // eslint-disable-next-line no-constant-condition
  while (true) {
    const ch = str[i];
    const digit = VLQ_LOOKUP.get(ch);
    if (digit === undefined) throw new Error(`Invalid VLQ character: ${ch}`);
    i++;
    result += (digit & 31) << shift;
    shift += 5;
    if ((digit & 32) === 0) break;
  }
  // The lowest bit is the sign
  const negate = (result & 1) !== 0;
  result >>= 1;
  return [negate ? -result : result, i];
}

/** Encode a single integer as a VLQ string. */
export function encodeVLQValue(value: number): string {
  let vlq = value < 0 ? (-value << 1) | 1 : value << 1;
  let result = "";
  do {
    let digit = vlq & 31;
    vlq >>>= 5;
    if (vlq > 0) digit |= 32;
    result += VLQ_CHARS[digit];
  } while (vlq > 0);
  return result;
}

// ── Segment types ──────────────────────────────────────────────

/**
 * A source map segment: [genCol, sourceIdx, srcLine, srcCol, nameIdx?]
 * - genCol: column in the generated line (absolute within line)
 * - sourceIdx: index into the sources array
 * - srcLine: source line (0-based)
 * - srcCol: source column (0-based)
 * - nameIdx: optional index into names array
 */
export type Segment = [number, number, number, number] | [number, number, number, number, number];

/** Parse a VLQ mappings string into an array of lines, each containing segments. */
export function parseMappings(mappings: string): Segment[][] {
  if (!mappings) return [];

  const lines: Segment[][] = [];
  const groups = mappings.split(";");

  let srcLine = 0;
  let srcCol = 0;
  let sourceIdx = 0;
  let nameIdx = 0;

  for (const group of groups) {
    const segments: Segment[] = [];
    let genCol = 0; // genCol resets per line

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
          genCol += values[0];
          sourceIdx += values[1];
          srcLine += values[2];
          srcCol += values[3];
          if (values.length >= 5) {
            nameIdx += values[4];
            segments.push([genCol, sourceIdx, srcLine, srcCol, nameIdx]);
          } else {
            segments.push([genCol, sourceIdx, srcLine, srcCol]);
          }
        } else if (values.length === 1) {
          // Column-only segment (no source mapping)
          genCol += values[0];
        }
      }
    }

    lines.push(segments);
  }
  return lines;
}

/** Encode segments back to a VLQ mappings string. */
export function encodeMappings(lines: Segment[][]): string {
  let prevSrcLine = 0;
  let prevSrcCol = 0;
  let prevSourceIdx = 0;
  let prevNameIdx = 0;

  const groups: string[] = [];
  for (const segments of lines) {
    let prevGenCol = 0; // resets per line
    const parts: string[] = [];
    for (const seg of segments) {
      let vlq = encodeVLQValue(seg[0] - prevGenCol);
      prevGenCol = seg[0];

      vlq += encodeVLQValue(seg[1] - prevSourceIdx);
      prevSourceIdx = seg[1];

      vlq += encodeVLQValue(seg[2] - prevSrcLine);
      prevSrcLine = seg[2];

      vlq += encodeVLQValue(seg[3] - prevSrcCol);
      prevSrcCol = seg[3];

      if (seg.length === 5) {
        vlq += encodeVLQValue(seg[4] - prevNameIdx);
        prevNameIdx = seg[4];
      }

      parts.push(vlq);
    }
    groups.push(parts.join(","));
  }
  return groups.join(";");
}

// ── Source map combination ─────────────────────────────────────

interface SourceMapJson {
  version: number;
  file?: string;
  sourceRoot?: string;
  sources: string[];
  sourcesContent?: (string | null)[];
  names: string[];
  mappings: string;
}

export interface CombineOptions {
  /** Raw JSON string of script source map (may be empty). */
  scriptMap: string;
  /** Script virtual file code (used to compute line count). May be empty. */
  scriptCode: string;
  /** Raw JSON string of template source map (may be empty). */
  templateMap: string;
  /** Template virtual file code as returned by the host (may include prepended import). */
  templateCode: string;
  /** Original Vue SFC source. */
  vueSource: string;
  /** Final JS after mergeRenderIntoComponent. */
  finalJs: string;
}

/**
 * Combine script and template source maps into a single map that covers
 * the final assembled JS output (`file.compiled.js`).
 *
 * The template source map has generated lines relative to the full SFC
 * CodeTransform (i.e., line numbers include lines before `<template>`).
 * The host also prepends an `import { ... } from "vue"\n` line to the
 * template code, shifting it by +1 line that the source map doesn't account for.
 *
 * `mergeRenderIntoComponent` may prepend `const __sfc__ = {};\n` for
 * template-only SFCs, shifting all lines by +1.
 */
export function combineSourceMaps(opts: CombineOptions): string {
  const { scriptMap, scriptCode, templateMap, templateCode, vueSource, finalJs } = opts;

  const scriptParsed = tryParseMap(scriptMap);
  const templateParsed = tryParseMap(templateMap);

  if (!scriptParsed && !templateParsed) return "";

  // Script line count in the assembled output.
  // When both script and template exist, assembledJs = scriptCode + "\n" + templateCode,
  // so script occupies scriptCode.split("\n").length lines (the "\n" separator is on the
  // last line of scriptCode, starting templateCode on the next line).
  const scriptLineCount = scriptCode ? scriptCode.split("\n").length : 0;

  // Determine line offset from mergeRenderIntoComponent.
  // It may prepend "const __sfc__ = {};\n" for template-only components.
  const mergeLineOffset = computeMergeLineOffset(scriptCode, finalJs);

  // Determine how many SFC prefix lines (before <template>) are in the template source map.
  const tplIdx = vueSource.indexOf("<template");
  const sfcPrefixLines = tplIdx !== -1 ? vueSource.slice(0, tplIdx).split("\n").length - 1 : 0;

  // Determine if the host prepended an import line to the template code.
  // The host prepends "import { ... } from \"vue\"\n" when the template needs vue imports.
  const hostImportOffset = templateCode.startsWith("import ") ? 1 : 0;

  // Build the combined source map
  const source = (scriptParsed ?? templateParsed)!;
  const combined: SourceMapJson = {
    version: 3,
    sources: source.sources.length > 0 ? [...source.sources] : [],
    sourcesContent: source.sourcesContent ? [...source.sourcesContent] : undefined,
    names: [],
    mappings: "",
  };

  // Collect all name entries from both maps
  const nameMap = new Map<string, number>(); // old name → new index
  const allNames: string[] = [];

  function getNameIdx(name: string): number {
    let idx = nameMap.get(name);
    if (idx === undefined) {
      idx = allNames.length;
      allNames.push(name);
      nameMap.set(name, idx);
    }
    return idx;
  }

  // Parse segments from both maps
  const scriptSegments = scriptParsed ? parseMappings(scriptParsed.mappings) : [];
  const templateSegments = templateParsed ? parseMappings(templateParsed.mappings) : [];

  // Determine total line count from the final JS
  const finalLineCount = finalJs.split("\n").length;
  const result: Segment[][] = Array.from({ length: finalLineCount }, () => []);

  // Copy script segments (shifted by mergeLineOffset)
  if (scriptParsed) {
    for (let genLine = 0; genLine < scriptSegments.length; genLine++) {
      const adjustedLine = genLine + mergeLineOffset;
      if (adjustedLine < 0 || adjustedLine >= finalLineCount) continue;
      for (const seg of scriptSegments[genLine]) {
        const newSeg: Segment = [seg[0], 0, seg[2], seg[3]];
        if (seg.length === 5) {
          const name = scriptParsed.names[seg[4]];
          if (name !== undefined) {
            (newSeg as number[]).push(getNameIdx(name));
          }
        }
        result[adjustedLine].push(newSeg);
      }
    }
  }

  // Copy template segments with offset adjustments:
  // Generated line in template map → subtract sfcPrefixLines → add scriptLineCount + hostImportOffset + mergeLineOffset
  if (templateParsed) {
    for (let genLine = 0; genLine < templateSegments.length; genLine++) {
      // Skip lines before the template region in the full-SFC source map
      if (genLine < sfcPrefixLines) continue;

      const adjustedLine =
        genLine - sfcPrefixLines + scriptLineCount + hostImportOffset + mergeLineOffset;
      if (adjustedLine < 0 || adjustedLine >= finalLineCount) continue;
      for (const seg of templateSegments[genLine]) {
        const newSeg: Segment = [seg[0], 0, seg[2], seg[3]];
        if (seg.length === 5) {
          const name = templateParsed.names[seg[4]];
          if (name !== undefined) {
            (newSeg as number[]).push(getNameIdx(name));
          }
        }
        result[adjustedLine].push(newSeg);
      }
    }
  }

  // Sort segments within each line by generated column
  for (const line of result) {
    line.sort((a, b) => a[0] - b[0]);
  }

  combined.names = allNames;
  combined.mappings = encodeMappings(result);

  return JSON.stringify(combined);
}

// ── Lookup utilities ───────────────────────────────────────────

interface MappedPosition {
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
    for (const seg of segments[genLine]) {
      if (seg[2] === srcLine) {
        const dist = Math.abs(seg[3] - srcCol);
        if (!best || dist < best.dist || (dist === best.dist && genLine < best.line)) {
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

  const lineSegs = segments[genLine];
  if (lineSegs.length === 0) return null;

  // Find the segment whose genCol is <= the requested genCol (binary search)
  let lo = 0;
  let hi = lineSegs.length - 1;
  while (lo < hi) {
    const mid = (lo + hi + 1) >>> 1;
    if (lineSegs[mid][0] <= genCol) lo = mid;
    else hi = mid - 1;
  }

  const seg = lineSegs[lo];
  if (seg[0] > genCol) return null;

  return { line: seg[2], col: seg[3] };
}

// ── Helpers ────────────────────────────────────────────────────

function tryParseMap(json: string): SourceMapJson | null {
  if (!json) return null;
  try {
    return JSON.parse(json) as SourceMapJson;
  } catch {
    return null;
  }
}

/**
 * Compute how many lines `mergeRenderIntoComponent` prepended to the output.
 * It prepends `const __sfc__ = {};\n` for template-only SFCs (no `export default`
 * and no `const __sfc__` in the input).
 */
function computeMergeLineOffset(scriptCode: string, finalJs: string): number {
  // If there's a script block producing `export default` or `const __sfc__`,
  // mergeRenderIntoComponent doesn't prepend any lines.
  if (
    scriptCode &&
    (/^export default /m.test(scriptCode) || /^const __sfc__ = /m.test(scriptCode))
  ) {
    return 0;
  }
  // Template-only: mergeRenderIntoComponent prepends "const __sfc__ = {};\n"
  if (finalJs.startsWith("const __sfc__ = {};")) {
    return 1;
  }
  return 0;
}
