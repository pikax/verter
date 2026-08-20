// Rewrite/chaining algebra for the two authorized rewrites.
//
// Implements layer 1 §2.4 (`CodeTransform` token geometry, CT-1…CT-14),
// §2.5 (authorized rewrites), §5.1, §5.3 (chain), §5.4 (sourceless-barrier),
// §5.5 (equal-coordinate ordering), §5.7 (template fragment is not rewritten)
// of `spec/assembled-map-composition-layer1.md`.

import {
  advance,
  payloadOf,
  resolveAt,
  SegmentIndex,
  sourcelessPayload,
  TextGeometry,
} from "./assembled-map-coordinates.mjs";

/**
 * §2.5 — pass 1 replaces every occurrence of the literal byte pattern
 * `__sfc__` (7) with `_sfc_main` (9).
 */
export const RENAME_PASS = { pattern: "__sfc__", replacement: "_sfc_main" };

/**
 * §2.5 — pass 2 operates on PASS 1's OUTPUT coordinate space — its pattern
 * contains the pass-1 output spelling — and removes every occurrence of the
 * literal byte pattern `export default _sfc_main;\n` (26).
 */
export const EXPORT_REMOVAL_PASS = { pattern: "export default _sfc_main;\n", replacement: "" };

/**
 * §2.4 CT-1…CT-4 — the chunk list of one pass over `text`.
 *
 * A fresh transform over a non-empty source holds exactly one chunk,
 * `Original{0, len}`; over an empty source it holds none (CT-1). Each overwrite
 * splits the covering `Original` chunk at the edit boundaries only, and NO EMPTY
 * `Original` CHUNK is ever produced (CT-4), so the chunk boundaries are exactly
 * `{0} ∪ {edit starts} ∪ {edit ends} ∪ {len}`.
 *
 * Matching is literal, global, non-overlapping, left-to-right, exactly as
 * `str::replace` does — and NOT identifier-aware: `___sfc__` contains `__sfc__`
 * at offset 1 and is rewritten (§2.5). `remove` is exactly `overwrite(s,e,"")`
 * (CT-2), so a removal is an `Overwritten` chunk with empty content.
 */
export function buildChunks(text, pattern, replacement) {
  const chunks = [];
  let cursor = 0;
  for (;;) {
    const found = text.indexOf(pattern, cursor);
    if (found === -1) break;
    if (found > cursor) chunks.push({ kind: "Original", start: cursor, end: found });
    chunks.push({
      kind: "Overwritten",
      start: found,
      end: found + pattern.length,
      content: replacement,
    });
    cursor = found + pattern.length;
  }
  if (cursor < text.length) chunks.push({ kind: "Original", start: cursor, end: text.length });
  return chunks;
}

/** §2.4 CT-14 — `build_string` concatenates every chunk's bytes in chunk order. */
export function buildString(text, chunks) {
  let out = "";
  for (const chunk of chunks) {
    out += chunk.kind === "Original" ? text.slice(chunk.start, chunk.end) : chunk.content;
  }
  return out;
}

/**
 * §5.3 — the chain operation. Chains `segments` (in `text`'s coordinate space)
 * through one pass's chunk list, producing the sequence in the pass OUTPUT's
 * coordinate space.
 *
 * Rules, as stated:
 *
 *   (a) `Original{s,e}` — its emission points are
 *         `{ s | s is the end of a replaced range of this pass }
 *          ∪ { o ∈ [s,e) : seg(o) is non-empty }`
 *       in increasing offset order. At each, if `seg(o)` is non-empty emit
 *       EVERY segment of `seg(o)` in `M`'s order carrying its own payload
 *       unchanged; otherwise emit ONE segment carrying `lookup(o)`'s payload,
 *       or a SOURCELESS segment if `lookup(o)` is absent.
 *   (b) non-empty `Overwritten` — emit exactly ONE segment at the replacement's
 *       generated start carrying `lookup(s)`'s payload, or sourceless if absent.
 *   (c) empty `Overwritten` — emit NOTHING; the generated position advances by
 *       zero.
 *   (d) end of walk — segments at offset `len(T)` are emitted, in `M`'s order,
 *       at the output's end position. THAT POSITION IS ALWAYS IN-BOUNDS AND
 *       THIS RULE IS ALWAYS LIVE; "an implementation that guards it on a
 *       trailing LF drops legitimate segments".
 *
 * Rule precedence (§5.3, §5.5 rule 2): (b) and (c) govern the WHOLE replaced
 * range — every segment of `M` whose offset lies in `[s,e)` is DROPPED, whatever
 * its multiplicity — and rule (a)'s emission-point machinery applies only inside
 * `Original` chunks.
 *
 * The suppression in rule (a) is §2.6's standard at the emission point: when
 * `seg(o)` is non-empty, `lookup(o)` IS the last segment of `seg(o)`, so the
 * resume segment would be a byte-identical duplicate of the last emitted
 * segment. Suppression removes a duplicate; it never removes information.
 *
 * The barrier (§5.4) is not a special rule here: `lookup` is "last applicable
 * segment, WHATEVER IT IS" rather than "last source-bearing segment", and a
 * chained emission point whose lookup is absent emits a SOURCELESS SEGMENT, not
 * nothing.
 */
export function chainThroughChunks(text, chunks, segments, origin) {
  const geometry = new TextGeometry(text);
  const index = new SegmentIndex(segments);

  // `seg(o)`: the sub-sequence of `M` whose position equals `pos(o)`, in `M`'s
  // order. Offsets are formed with `off(line, column)` (§5.2).
  const byOffset = new Map();
  for (const segment of segments) {
    const offset = geometry.offsetOf(segment.genLine, segment.genCol);
    let group = byOffset.get(offset);
    if (group === undefined) {
      group = [];
      byOffset.set(offset, group);
    }
    group.push(segment);
  }
  const declaredOffsets = [...byOffset.keys()].sort((a, b) => a - b);
  let nextDeclared = 0;

  const emitted = [];
  let out = { line: 0, column: 0 };

  const emitPayload = (position, payload) => {
    emitted.push({
      genLine: position.line,
      genCol: position.column,
      srcIdx: payload.srcIdx,
      srcLine: payload.srcLine,
      srcCol: payload.srcCol,
      nameIdx: payload.nameIdx,
      origin,
    });
  };

  const lookupPayload = (offset) => {
    const position = geometry.positionOf(offset);
    const resolved = resolveAt(index, position.line, position.column);
    return resolved === null ? sourcelessPayload() : payloadOf(resolved);
  };

  for (let i = 0; i < chunks.length; i += 1) {
    const chunk = chunks[i];

    if (chunk.kind !== "Original") {
      // Every segment of `M` inside `[s,e)` is dropped; skip them wholesale.
      while (nextDeclared < declaredOffsets.length && declaredOffsets[nextDeclared] < chunk.end) {
        nextDeclared += 1;
      }
      if (chunk.content.length === 0) continue; // rule (c)
      emitPayload(out, lookupPayload(chunk.start)); // rule (b)
      out = advance(out, chunk.content);
      continue;
    }

    // Rule (a). The first clause's condition — "s is the end of a replaced
    // range of this pass" — is equivalent to "this chunk is immediately
    // preceded in chunk order by an `Overwritten` chunk", because chunk
    // boundaries are exactly `{0} ∪ {edit starts} ∪ {edit ends} ∪ {len}` and a
    // replaced range is always exactly one chunk.
    const resumes = i > 0 && chunks[i - 1].kind === "Overwritten";
    const points = [];
    if (resumes) points.push(chunk.start);
    while (nextDeclared < declaredOffsets.length && declaredOffsets[nextDeclared] < chunk.start) {
      nextDeclared += 1;
    }
    for (let d = nextDeclared; d < declaredOffsets.length; d += 1) {
      const offset = declaredOffsets[d];
      if (offset >= chunk.end) break;
      if (points.length > 0 && points[points.length - 1] === offset) continue; // union
      points.push(offset);
    }

    let cursor = chunk.start;
    let at = out;
    for (const offset of points) {
      at = advance(at, text.slice(cursor, offset));
      cursor = offset;
      const group = byOffset.get(offset);
      if (group !== undefined) {
        for (const segment of group) emitPayload(at, payloadOf(segment));
      } else {
        emitPayload(at, lookupPayload(offset));
      }
    }
    out = advance(at, text.slice(cursor, chunk.end));
  }

  // Rule (d). Segments at `len(T)` are covered by no chunk and would otherwise
  // be silently dropped.
  const endGroup = byOffset.get(text.length);
  if (endGroup !== undefined) {
    for (const segment of endGroup) emitPayload(out, payloadOf(segment));
  }

  return emitted;
}

/**
 * §5.1 — the script fragment's two passes, applied SEQUENTIALLY, pass 2 over
 * pass 1's output coordinate space, exactly as the code does (§2.5).
 *
 * `segments` may be `null`: §5.8's mapless present fragment is still rewritten,
 * because "the passes determine the module's BYTES, and the code baseline is
 * pinned regardless of any map", but there is no `M` to chain and §5.3 is not
 * invoked for it at all.
 */
export function runScriptRewritePasses(code, segments, origin) {
  let text = code;
  let current = segments;
  for (const pass of [RENAME_PASS, EXPORT_REMOVAL_PASS]) {
    const chunks = buildChunks(text, pass.pattern, pass.replacement);
    const next = buildString(text, chunks);
    if (current !== null) current = chainThroughChunks(text, chunks, current, origin);
    text = next;
  }
  return { code: text, segments: current };
}
