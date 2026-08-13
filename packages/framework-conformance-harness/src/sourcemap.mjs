// Source-map v3 mappings codec + assembled-module map composition.
//
// WHY THIS EXISTS. The official compiler emits two INDEPENDENT maps for a
// non-inline SFC compile — compileScript's map over the script half and
// compileTemplate's map over the render fragment (chained through the
// descriptor block map so original coordinates are whole-fixture-file
// coordinates). The harness assembles those two fragments into ONE module
// (invoke-vue-oracle.mjs `assembleNonInline`), so a published golden map
// must describe THAT assembled module: each fragment map's generated
// coordinates are re-anchored by the assembly's exact geometry — the
// single-line keyword splice each fragment receives (`export default ` →
// `const _sfc_main = `; `export ` stripped) and the fragment's line offset
// within the assembled text. Nothing else is invented: every surviving
// segment is an official-compiler segment with only its generated
// position translated; segments inside a replaced keyword span (harness
// text, no longer present) are dropped.
//
// The decoder doubles as the comparator's normalizer (compare.mjs): two
// mappings fields are equivalent when their DECODED, normalized segment
// sets agree — VLQ relative-encoding spelling, in-line segment order,
// duplicate segments, and trailing empty lines are representation
// artifacts, not mapping semantics.

const BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_LOOKUP = new Map([...BASE64].map((char, index) => [char, index]));

/** Decodes one VLQ-encoded segment string into its numeric fields. */
function decodeVlqSegment(segment) {
  const values = [];
  let value = 0;
  let shift = 0;
  for (const char of segment) {
    const digit = BASE64_LOOKUP.get(char);
    if (digit === undefined) throw new Error(`invalid VLQ character: ${JSON.stringify(char)}`);
    const continuation = digit & 32;
    value += (digit & 31) << shift;
    if (continuation) {
      shift += 5;
    } else {
      const negate = value & 1;
      value >>>= 1;
      values.push(negate ? -value : value);
      value = 0;
      shift = 0;
    }
  }
  if (shift !== 0) throw new Error("truncated VLQ segment");
  return values;
}

function encodeVlqValue(value) {
  let vlq = value < 0 ? (-value << 1) | 1 : value << 1;
  let encoded = "";
  do {
    let digit = vlq & 31;
    vlq >>>= 5;
    if (vlq > 0) digit |= 32;
    encoded += BASE64[digit];
  } while (vlq > 0);
  return encoded;
}

/**
 * Decodes a v3 `mappings` string into ABSOLUTE segments:
 * `{ genLine, genCol, srcIdx, srcLine, srcCol, nameIdx }` (source/name
 * fields null for 1-length segments). Lines and columns are 0-based, as in
 * the wire format.
 */
export function decodeMappings(mappings) {
  const segments = [];
  let srcIdx = 0;
  let srcLine = 0;
  let srcCol = 0;
  let nameIdx = 0;
  const lines = mappings.split(";");
  for (let genLine = 0; genLine < lines.length; genLine += 1) {
    let genCol = 0;
    if (lines[genLine] === "") continue;
    for (const raw of lines[genLine].split(",")) {
      const fields = decodeVlqSegment(raw);
      if (fields.length !== 1 && fields.length !== 4 && fields.length !== 5) {
        throw new Error(`malformed mapping segment (${fields.length} fields)`);
      }
      genCol += fields[0];
      if (fields.length === 1) {
        segments.push({
          genLine,
          genCol,
          srcIdx: null,
          srcLine: null,
          srcCol: null,
          nameIdx: null,
        });
        continue;
      }
      srcIdx += fields[1];
      srcLine += fields[2];
      srcCol += fields[3];
      const named = fields.length === 5;
      if (named) nameIdx += fields[4];
      segments.push({ genLine, genCol, srcIdx, srcLine, srcCol, nameIdx: named ? nameIdx : null });
    }
  }
  return segments;
}

/** Encodes absolute segments (any order) back into a v3 `mappings` string. */
export function encodeMappings(segments) {
  const sorted = [...segments].sort((a, b) => a.genLine - b.genLine || a.genCol - b.genCol);
  const lastLine = sorted.length === 0 ? -1 : sorted[sorted.length - 1].genLine;
  const lines = [];
  let cursor = 0;
  let prevSrcIdx = 0;
  let prevSrcLine = 0;
  let prevSrcCol = 0;
  let prevNameIdx = 0;
  for (let genLine = 0; genLine <= lastLine; genLine += 1) {
    const encoded = [];
    let prevGenCol = 0;
    while (cursor < sorted.length && sorted[cursor].genLine === genLine) {
      const seg = sorted[cursor];
      cursor += 1;
      let out = encodeVlqValue(seg.genCol - prevGenCol);
      prevGenCol = seg.genCol;
      if (seg.srcIdx !== null) {
        out += encodeVlqValue(seg.srcIdx - prevSrcIdx);
        out += encodeVlqValue(seg.srcLine - prevSrcLine);
        out += encodeVlqValue(seg.srcCol - prevSrcCol);
        prevSrcIdx = seg.srcIdx;
        prevSrcLine = seg.srcLine;
        prevSrcCol = seg.srcCol;
        if (seg.nameIdx !== null) {
          out += encodeVlqValue(seg.nameIdx - prevNameIdx);
          prevNameIdx = seg.nameIdx;
        }
      }
      encoded.push(out);
    }
    lines.push(encoded.join(","));
  }
  return lines.join(";");
}

/**
 * Normal form for mappings EQUIVALENCE: absolute segments, sorted, with
 * exact duplicates collapsed. Two `mappings` strings whose normal forms
 * agree address identical (generated → original) correspondences; they may
 * still differ in VLQ spelling, in-line ordering, duplicates, or trailing
 * empty lines.
 */
export function normalizedMappingSegments(mappings) {
  const key = (s) => `${s.genLine},${s.genCol},${s.srcIdx},${s.srcLine},${s.srcCol},${s.nameIdx}`;
  const byKey = new Map();
  for (const segment of decodeMappings(mappings)) byKey.set(key(segment), segment);
  return [...byKey.values()].sort(
    (a, b) =>
      a.genLine - b.genLine ||
      a.genCol - b.genCol ||
      (a.srcIdx ?? -1) - (b.srcIdx ?? -1) ||
      (a.srcLine ?? -1) - (b.srcLine ?? -1) ||
      (a.srcCol ?? -1) - (b.srcCol ?? -1) ||
      (a.nameIdx ?? -1) - (b.nameIdx ?? -1),
  );
}

/** 0-based (line, column) of a string offset within `text`. */
function positionOfOffset(text, offset) {
  let line = 0;
  let lineStart = 0;
  for (let i = 0; i < offset; i += 1) {
    if (text.charCodeAt(i) === 10) {
      line += 1;
      lineStart = i + 1;
    }
  }
  return { line, column: offset - lineStart };
}

/**
 * Composes the assembled module's map from the per-fragment official maps.
 *
 * @param {Array<{
 *   map: object|null,
 *   preEditCode: string,
 *   postEditCode: string,
 *   edit: { start: number, end: number, replacementLength: number }|null,
 * }>} parts in assembly order; `edit` offsets address `preEditCode`
 * @returns {object|null} a v3 map for `parts.map(p => p.postEditCode).join("\n")`,
 *   or null when no part carries a map
 */
export function composeAssembledModuleMap(parts) {
  const sources = [];
  const sourcesContent = [];
  const sourceIndexByKey = new Map();
  const names = [];
  const nameIndexByKey = new Map();
  const segments = [];
  let lineOffset = 0;
  let anyMap = false;

  for (const part of parts) {
    if (part.map) {
      anyMap = true;
      const map = part.map;
      const sourceRemap = (map.sources ?? []).map((source, index) => {
        const content = map.sourcesContent?.[index] ?? null;
        const key = `${source}\0${content}`;
        if (!sourceIndexByKey.has(key)) {
          sourceIndexByKey.set(key, sources.length);
          sources.push(source);
          sourcesContent.push(content);
        }
        return sourceIndexByKey.get(key);
      });
      const nameRemap = (map.names ?? []).map((name) => {
        if (!nameIndexByKey.has(name)) {
          nameIndexByKey.set(name, names.length);
          names.push(name);
        }
        return nameIndexByKey.get(name);
      });

      let edit = null;
      if (part.edit) {
        const startPos = positionOfOffset(part.preEditCode, part.edit.start);
        const endPos = positionOfOffset(part.preEditCode, part.edit.end);
        if (startPos.line !== endPos.line) {
          // The assembler only ever replaces a single-line keyword span
          // (`export default `, `export `); anything else means the
          // fragment shape drifted from the assembler's contract.
          throw new Error("assembly edit spans multiple lines; cannot re-anchor its map");
        }
        edit = {
          line: startPos.line,
          colStart: startPos.column,
          colEnd: endPos.column,
          delta: part.edit.replacementLength - (part.edit.end - part.edit.start),
        };
      }

      for (const seg of decodeMappings(map.mappings ?? "")) {
        let genCol = seg.genCol;
        if (edit && seg.genLine === edit.line) {
          if (genCol >= edit.colStart && genCol < edit.colEnd) continue; // replaced keyword span
          if (genCol >= edit.colEnd) genCol += edit.delta;
        }
        segments.push({
          genLine: seg.genLine + lineOffset,
          genCol,
          srcIdx: seg.srcIdx === null ? null : sourceRemap[seg.srcIdx],
          srcLine: seg.srcLine,
          srcCol: seg.srcCol,
          nameIdx: seg.nameIdx === null ? null : nameRemap[seg.nameIdx],
        });
      }
    }
    lineOffset += part.postEditCode.split("\n").length;
  }

  if (!anyMap) return null;
  return {
    version: 3,
    sources,
    sourcesContent,
    names,
    mappings: encodeMappings(segments),
  };
}
