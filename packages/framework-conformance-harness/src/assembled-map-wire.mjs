// v3 `mappings` wire codec: a strict decoder and the canonical encoder.
//
// Implements layer 1 §4.3 step 1.21 (three ordered decode phases, `U3.*`
// taxonomy) and §7.6 (canonical `mappings` encoding) of
// `spec/assembled-map-composition-layer1.md`.
//
// Not `src/sourcemap.mjs`: that `decodeMappings` is the accepted wire-format
// authority (decoded segment shape §2.2; a zero column delta pushes an
// additional segment rather than replacing one, §7.6) but is lenient where
// layer 1 is strict — `"A"` and `"ggggggE"` both decode to 0 there because
// a 32-bit shift wraps; layer 1 requires the second rejected. It also
// raises untyped errors where layer 1 requires six distinguishable
// sub-codes in mandated order. Decode is reimplemented to that contract;
// tests cross-check this encoder against the accepted decoder.
//
// That `encodeMappings` sorts by `(genLine, genCol)` first. §7.6 requires
// sequence order, never re-sorted, and forbids relying on the sort to
// impose order. It also encodes through `<<`, which wraps for the largest
// in-range field values. Hence a local encoder.

const BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_DIGIT = new Map([...BASE64].map((character, index) => [character, index]));

const INT32_MAX = 2 ** 31 - 1;

/** A `U3.*` rejection from the decoder. */
function reject(code) {
  return { ok: false, code };
}

/**
 * §4.3 step 1.21 — decode `mappings` into an absolute segment sequence, or
 * report the first violation in wire order.
 *
 * Single left-to-right pass. Within one segment, three ordered phases:
 *
 *   A — lexical and per-field as each field is read: `U3.1` invalid
 *     character, `U3.2` truncated segment, `U3.4` field range.
 *   B — arity once the segment is fully read: `U3.3`.
 *   C — accumulator application, only if B passed: `U3.5`, then `U3.6`
 *     for `genCol` only.
 *
 * Arity beats every accumulator property; within C, range beats ordering.
 *
 * @returns {{ ok: true, segments: object[] } | { ok: false, code: string }}
 */
export function decodeMappingsStrict(mappings) {
  const segments = [];
  let srcIdx = 0;
  let srcLine = 0;
  let srcCol = 0;
  let nameIdx = 0;

  const groups = mappings.split(";");
  for (let genLine = 0; genLine < groups.length; genLine += 1) {
    const group = groups[genLine];
    // Empty group = generated line with no segments, not a zero-field
    // segment (`"A,,B"` middle token is `U3.3` under B).
    if (group === "") continue;

    let genCol = 0;
    let previousGenColOnLine = null;
    for (const token of group.split(",")) {
      // A
      const fields = [];
      let at = 0;
      while (at < token.length) {
        let value = 0;
        let shift = 0;
        let continues = true;
        let pastBit31 = false;
        while (at < token.length) {
          const digit = BASE64_DIGIT.get(token[at]);
          if (digit === undefined) return reject("U3.1");
          at += 1;
          continues = (digit & 32) !== 0;
          const bits = digit & 31;
          // Field encoding continues past bit 31: a digit group wholly
          // beyond bit 31, or one that sets a bit at position ≥ 32. At
          // shift 30 the legal value bits are 0..3; `"ggggggE"` sets bit 32.
          if (shift >= 32) pastBit31 = true;
          else if (bits >= 2 ** (32 - shift)) pastBit31 = true;
          if (!pastBit31) value += bits * 2 ** shift;
          shift += 5;
          if (!continues) break;
        }
        // Ordered `U3.1`, `U3.2`, `U3.4`: mid-field end + past bit 31 → `U3.2`.
        if (continues) return reject("U3.2");
        if (pastBit31) return reject("U3.4");
        const negative = value % 2 === 1;
        const magnitude = (value - (negative ? 1 : 0)) / 2;
        const decoded = negative ? -magnitude : magnitude;
        if (decoded < -INT32_MAX || decoded > INT32_MAX) return reject("U3.4");
        fields.push(decoded);
      }

      // B
      if (fields.length !== 1 && fields.length !== 4 && fields.length !== 5) {
        return reject("U3.3");
      }

      // C
      const nextGenCol = genCol + fields[0];
      if (nextGenCol < 0 || nextGenCol > INT32_MAX) return reject("U3.5");
      if (previousGenColOnLine !== null && nextGenCol < previousGenColOnLine) {
        return reject("U3.6");
      }
      genCol = nextGenCol;
      previousGenColOnLine = nextGenCol;

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

      const nextSrcIdx = srcIdx + fields[1];
      if (nextSrcIdx < 0 || nextSrcIdx > INT32_MAX) return reject("U3.5");
      srcIdx = nextSrcIdx;
      const nextSrcLine = srcLine + fields[2];
      if (nextSrcLine < 0 || nextSrcLine > INT32_MAX) return reject("U3.5");
      srcLine = nextSrcLine;
      const nextSrcCol = srcCol + fields[3];
      if (nextSrcCol < 0 || nextSrcCol > INT32_MAX) return reject("U3.5");
      srcCol = nextSrcCol;

      if (fields.length === 4) {
        segments.push({ genLine, genCol, srcIdx, srcLine, srcCol, nameIdx: null });
        continue;
      }

      const nextNameIdx = nameIdx + fields[4];
      if (nextNameIdx < 0 || nextNameIdx > INT32_MAX) return reject("U3.5");
      nameIdx = nextNameIdx;
      segments.push({ genLine, genCol, srcIdx, srcLine, srcCol, nameIdx });
    }
  }

  return { ok: true, segments };
}

/** Encodes one VLQ field. Arithmetic, not `<<`: an in-range field's wire value reaches `2^32 − 1`. */
function encodeVlqField(value) {
  let wire = value < 0 ? -value * 2 + 1 : value * 2;
  let out = "";
  do {
    let digit = wire % 32;
    wire = Math.floor(wire / 32);
    if (wire > 0) digit += 32;
    out += BASE64[digit];
  } while (wire > 0);
  return out;
}

/**
 * §7.6 — the canonical `mappings` encoding. The sequence is encoded IN
 * SEQUENCE ORDER, never re-sorted:
 *
 *  - a generated line advance emits one `;` per line crossed; segments on the
 *    same line are separated by `,`; a generated line with no segments
 *    contributes an empty group;
 *  - within a line the first segment's column field is its absolute column;
 *    subsequent fields are deltas against the running accumulators, `genCol`
 *    reset to 0 at each line and the other four carried across lines;
 *  - a sourceless segment encodes exactly one field; a source-bearing segment
 *    four, or five with a name;
 *  - encoding STOPS after the last segment-bearing line: no trailing `;` group.
 */
export function encodeMappings(segments) {
  let out = "";
  let line = 0;
  let genCol = 0;
  let srcIdx = 0;
  let srcLine = 0;
  let srcCol = 0;
  let nameIdx = 0;
  let firstOnLine = true;

  for (const segment of segments) {
    while (line < segment.genLine) {
      out += ";";
      line += 1;
      genCol = 0;
      firstOnLine = true;
    }
    if (!firstOnLine) out += ",";
    firstOnLine = false;

    out += encodeVlqField(segment.genCol - genCol);
    genCol = segment.genCol;

    if (segment.srcIdx !== null) {
      out += encodeVlqField(segment.srcIdx - srcIdx);
      srcIdx = segment.srcIdx;
      out += encodeVlqField(segment.srcLine - srcLine);
      srcLine = segment.srcLine;
      out += encodeVlqField(segment.srcCol - srcCol);
      srcCol = segment.srcCol;
      if (segment.nameIdx !== null) {
        out += encodeVlqField(segment.nameIdx - nameIdx);
        nameIdx = segment.nameIdx;
      }
    }
  }

  return out;
}
