// The v3 `mappings` wire codec: a STRICT decoder and the canonical encoder.
//
// Implements LAYER 1 §4.3 step 1.21 (the three ordered decode phases and the
// `U3.*` taxonomy) and §7.6 (canonical `mappings` encoding) of
// `spec/assembled-map-composition-layer1.md`.
//
// WHY NOT `src/sourcemap.mjs`'s CODEC. Its `decodeMappings` is the accepted
// WIRE-FORMAT authority — layer 1 cites it for the shape of a decoded segment
// (§2.2) and for the fact that a zero column delta pushes an ADDITIONAL segment
// rather than replacing one (§7.6). It is deliberately LENIENT where layer 1 is
// strict: `U3.4`'s own table row records that `"A"` and `"ggggggE"` both decode
// to 0 there "because a 32-bit shift wraps", and layer 1 requires the second to
// be REJECTED. It also raises untyped errors where layer 1 requires six
// distinguishable sub-codes in a mandated order. So the decode is reimplemented
// to layer 1's contract; `test/` cross-checks this module's ENCODER against
// that accepted decoder, which is where the wire-format authority belongs.
//
// Its `encodeMappings` sorts by `(genLine, genCol)` first. §7.6 requires the
// sequence to be encoded "in sequence order, never re-sorted" and states that
// "an implementation must not rely on the sort to impose order". It also encodes
// through `<<`, which wraps for the largest in-range field values. Hence a local
// encoder.

const BASE64 = "ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
const BASE64_DIGIT = new Map([...BASE64].map((character, index) => [character, index]));

const INT32_MAX = 2 ** 31 - 1;

/** A `U3.*` rejection from the decoder. */
function reject(code) {
  return { ok: false, code };
}

/**
 * §4.3 step 1.21 — decodes `mappings` into an absolute segment sequence, or
 * reports the FIRST violation in wire order.
 *
 * The decode is a single left-to-right pass and segments are examined in wire
 * order. Within one segment the checks run in three ordered phases:
 *
 *   Phase A — lexical and per-field, as each field is read, in wire order:
 *     `U3.1` invalid character, `U3.2` truncated segment, `U3.4` field range.
 *   Phase B — arity, once the segment has been read in full: `U3.3`.
 *   Phase C — accumulator application, and ONLY if phase B passed: `U3.5`
 *     then, for `genCol` only, `U3.6`.
 *
 * "Arity therefore beats every accumulator property" and, within phase C,
 * "range beats ordering".
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
    // An empty group is a generated line with no segments — not a zero-field
    // segment. (`"A,,B"`'s middle token IS an empty segment token and is
    // `U3.3` under phase B.)
    if (group === "") continue;

    let genCol = 0;
    let previousGenColOnLine = null;
    for (const token of group.split(",")) {
      // ---- phase A -------------------------------------------------------
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
          // "the field's encoding continues past bit 31": a digit group that
          // lies wholly beyond bit 31, or one that sets a bit at position ≥ 32.
          // At shift 30 (the seventh digit) the legal value bits are 0..3; the
          // `U3.4` row's own example `"ggggggE"` sets bit 32 there.
          if (shift >= 32) pastBit31 = true;
          else if (bits >= 2 ** (32 - shift)) pastBit31 = true;
          if (!pastBit31) value += bits * 2 ** shift;
          shift += 5;
          if (!continues) break;
        }
        // The bullets are ordered `U3.1`, `U3.2`, `U3.4`, so a segment that
        // both ends mid-field AND ran past bit 31 reports `U3.2`.
        if (continues) return reject("U3.2");
        if (pastBit31) return reject("U3.4");
        const negative = value % 2 === 1;
        const magnitude = (value - (negative ? 1 : 0)) / 2;
        const decoded = negative ? -magnitude : magnitude;
        if (decoded < -INT32_MAX || decoded > INT32_MAX) return reject("U3.4");
        fields.push(decoded);
      }

      // ---- phase B -------------------------------------------------------
      if (fields.length !== 1 && fields.length !== 4 && fields.length !== 5) {
        return reject("U3.3");
      }

      // ---- phase C -------------------------------------------------------
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
