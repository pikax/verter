/**
 * Shared test helper: build a V3 source map JSON string from ABSOLUTE segments,
 * independent of the decoder under test (so a decode bug fails the consuming
 * test). Each segment is `[genCol, srcIdx, srcLine, srcCol]`; the generated
 * column resets per generated line, the source fields stay cumulative across
 * lines, matching the V3 wire encoding.
 */

const VLQ_CHARS = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

function encodeVlqInt(value: number): string {
  let vlq = value < 0 ? (-value << 1) | 1 : value << 1;
  let out = "";
  do {
    let digit = vlq & 31;
    vlq >>>= 5;
    if (vlq > 0) digit |= 32;
    out += VLQ_CHARS[digit];
  } while (vlq > 0);
  return out;
}

/**
 * Encode absolute segments-per-line into a V3 `mappings` string. A length-4 segment
 * `[genCol, srcIdx, srcLine, srcCol]` is a MAPPED token; a length-1 segment `[genCol]`
 * is a source-less (unmapped) token — the shape verter's compiler emits for inserted
 * content.
 */
export function encodeMappings(
  segmentsPerLine: readonly (readonly (readonly number[])[])[],
): string {
  let prevSrcIdx = 0;
  let prevSrcLine = 0;
  let prevSrcCol = 0;
  const lines: string[] = [];
  for (const segs of segmentsPerLine) {
    let prevGenCol = 0;
    const parts: string[] = [];
    for (const seg of segs) {
      const genCol = seg[0];
      if (seg.length >= 4) {
        const [, srcIdx, srcLine, srcCol] = seg;
        parts.push(
          encodeVlqInt(genCol - prevGenCol) +
            encodeVlqInt(srcIdx - prevSrcIdx) +
            encodeVlqInt(srcLine - prevSrcLine) +
            encodeVlqInt(srcCol - prevSrcCol),
        );
        prevSrcIdx = srcIdx;
        prevSrcLine = srcLine;
        prevSrcCol = srcCol;
      } else {
        // A generated-column-only (source-less / unmapped) token.
        parts.push(encodeVlqInt(genCol - prevGenCol));
      }
      prevGenCol = genCol;
    }
    lines.push(parts.join(","));
  }
  return lines.join(";");
}

/** Build a full V3 source map JSON string for the given sources and segments. */
export function buildSourceMapJson(
  sources: readonly string[],
  segmentsPerLine: readonly (readonly (readonly number[])[])[],
  file = "out.tsx",
): string {
  return JSON.stringify({
    version: 3,
    file,
    sources,
    names: [],
    mappings: encodeMappings(segmentsPerLine),
  });
}
