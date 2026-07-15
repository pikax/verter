/**
 * Test-anchor stripping and per-region position recomputation.
 *
 * Authored fixture sources mark probe points with named `@dx-anchor` comments
 * (id ∈ `[A-Za-z0-9_.-]+`), in the comment form natural to each SFC region:
 *
 *  - template regions: an HTML comment `<!-- @dx-anchor id -->`;
 *  - script/style regions: a line comment `// @dx-anchor id`.
 *
 * The whole comment IS the anchor, and {@link stripAnchors} removes the WHOLE
 * comment so no residue reaches the `verter_dx_baseline` materializer. The two
 * forms are stripped by their own grammar: the HTML comment is delimited, so only
 * the bytes through `-->` are removed and any markup after it stays live; the line
 * comment runs to end-of-line, so the ENTIRE line comment is removed — the `//`,
 * the id, and any trailing prose after the id (`// @dx-anchor id some words`),
 * plus the horizontal whitespace that separated it from preceding code (a
 * code-trailing `const x = 1 // @dx-anchor id` strips to `const x = 1`, no
 * now-trailing space). The newline itself is never consumed, so an anchor on its
 * own line leaves the now-empty line in place and line numbers are preserved.
 *
 * Each removed comment's position is recorded as a 0-based `{ line, character }`
 * **in the stripped text**, accounting for the FULL span every earlier comment on
 * the line removed; a same-line position after a stripped anchor shifts left by
 * exactly that comment's length.
 *
 * Positions are LSP-shaped (line breaks folded across `\r\n`/`\r`/`\n`; the
 * column is a UTF-16 code-unit count), so a CRLF fixture and its LF twin resolve
 * an anchor to the same position; each position carries an explicit
 * `encoding: "utf-16"` so a consumer never has to assume the column unit.
 */

import { offsetToLineChar } from "./paths.js";

/**
 * A well-formed anchor comment, in either region form. The first capture group
 * is the template HTML-comment id, the second the script/style line-comment id;
 * exactly one is populated per match, and `m[0]` is the entire removed span.
 *
 * The HTML-comment alternative is delimited: it ends at `-->`, so trailing markup
 * on the same template line is left live. The line-comment alternative runs to
 * end-of-line: a leading `[ \t]*` swallows the horizontal whitespace separating
 * the comment from any preceding code (so the code keeps no now-trailing space),
 * and a trailing `[^\r\n]*` swallows the rest of the line comment after the id
 * (trailing whitespace or prose) — but never the `\r`/`\n`, so a malformed
 * `// @dx-anchor` whose id would sit on the next line never swallows the newline
 * and an own-line anchor leaves the now-empty line in place.
 */
const ANCHOR =
  /<!--\s*@dx-anchor\s+([A-Za-z0-9_.\-]+)\s*-->|[ \t]*\/\/[ \t]*@dx-anchor[ \t]+([A-Za-z0-9_.\-]+)[^\r\n]*/g;

/**
 * The position-encoding of an anchor's `character` column. Authored anchor
 * columns are UTF-16 code units (LSP's default document-position unit), which a
 * downstream raw-LSP / extension consumer must distinguish from a negotiated
 * byte / UTF-8 offset a provider may use. Carried explicitly on the DTO so the
 * unit is data, not a doc-comment assumption.
 */
export type AnchorEncoding = "utf-16";

/** The encoding every authored anchor column is recorded in. */
const ANCHOR_ENCODING: AnchorEncoding = "utf-16";

/** A 0-based anchor position within one stripped source file. */
export interface AnchorPosition {
  line: number;
  character: number;
  /** The unit of {@link character} — always UTF-16 code units; see {@link AnchorEncoding}. */
  encoding: AnchorEncoding;
}

/** A resolved anchor: its owning file plus its position in that stripped file. */
export interface Anchor extends AnchorPosition {
  file: string;
}

/** A workspace-wide anchor lookup, keyed by globally-unique anchor name. */
export type AnchorMap = Map<string, Anchor>;

/** The result of stripping anchors from one source file. */
export interface StripResult {
  /** The source with every `@dx-anchor` comment removed. */
  stripped: string;
  /** Anchor name → position in {@link stripped}. */
  anchors: Map<string, AnchorPosition>;
}

/** A typed error for anchor faults (duplicate name, missing required anchor). */
export class AnchorError extends Error {
  constructor(message: string) {
    super(message);
    this.name = "AnchorError";
  }
}

/**
 * Remove every `@dx-anchor` comment from `source`, returning the cleaned text and
 * each anchor's recomputed position in that cleaned text.
 *
 * @throws {AnchorError} if a name appears more than once in this file — a named
 *   probe point must be unique.
 */
export function stripAnchors(source: string): StripResult {
  // name → the comment's char index in the stripped text (= the running length of
  // `stripped` at the point the comment is dropped, which already excludes every
  // earlier comment).
  const strippedIndex = new Map<string, number>();
  let stripped = "";
  let lastIndex = 0;

  ANCHOR.lastIndex = 0;
  for (let m = ANCHOR.exec(source); m !== null; m = ANCHOR.exec(source)) {
    // Exactly one of the two alternatives' id groups is populated per match.
    const name = m[1] ?? m[2];
    if (strippedIndex.has(name)) {
      throw new AnchorError(`duplicate anchor "${name}" in source`);
    }
    // Carry forward the text between the previous comment and this one, then the
    // comment's position in the stripped text is the current stripped length.
    stripped += source.slice(lastIndex, m.index);
    strippedIndex.set(name, stripped.length);
    lastIndex = m.index + m[0].length;
  }
  stripped += source.slice(lastIndex);

  // Fold each stripped-text index into an LSP `{ line, character }` against the
  // FINAL stripped text, so line breaks are counted consistently, and tag the
  // column unit explicitly so consumers never have to assume UTF-16.
  const anchors = new Map<string, AnchorPosition>();
  for (const [name, index] of strippedIndex) {
    anchors.set(name, { ...offsetToLineChar(stripped, index), encoding: ANCHOR_ENCODING });
  }

  return { stripped, anchors };
}

/**
 * Fold one file's {@link StripResult} into a workspace-wide {@link AnchorMap},
 * attaching `file` to each anchor.
 *
 * @throws {AnchorError} if an anchor name already exists in `map` — anchor names
 *   are globally unique across the fixture set so a probe target is unambiguous.
 */
export function addFileAnchors(map: AnchorMap, file: string, result: StripResult): void {
  for (const [name, pos] of result.anchors) {
    const existing = map.get(name);
    if (existing) {
      throw new AnchorError(
        `duplicate anchor "${name}" across files: ${existing.file} and ${file}`,
      );
    }
    map.set(name, { file, line: pos.line, character: pos.character, encoding: pos.encoding });
  }
}

/**
 * Look up an anchor by name, failing loudly when the fixture set lacks it.
 *
 * @throws {AnchorError} if `name` is not present — a probe that references a
 *   missing anchor is a fixture error, never a silently-skipped no-op.
 */
export function requireAnchor(map: AnchorMap, name: string): Anchor {
  const anchor = map.get(name);
  if (!anchor) {
    throw new AnchorError(`required anchor "${name}" not found`);
  }
  return anchor;
}
