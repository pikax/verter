/**
 * Oracle `.ts` anchor resolution.
 *
 * The hand-authored `.ts` oracles mark their query points with the same
 * `// @dx-anchor id` comments the fixture materializer uses ({@link ../anchors}).
 * The baseline bridge addresses files by UTF-8 byte offset (verter's TypeProvider
 * contract), while authored anchor columns are UTF-16 code units, so each stripped
 * anchor is converted into the byte offset the bridge `query`/`diagnostics`
 * requests carry. The conversion reuses the shared {@link DocumentPositions}
 * coordinate converter rather than re-deriving a byte walk.
 *
 * Unlike the `.vue` fixtures, the oracle `.ts` files ARE formatted by `oxfmt`,
 * which relocates a trailing line comment to the end of the line PAST the
 * statement terminator it inserts (`model.value // @dx-anchor m` becomes
 * `model.value; // @dx-anchor m`). The raw stripped anchor then sits after the
 * `;`, where the provider resolves no symbol. So each anchor resolves to the START
 * of the LAST identifier on its line — the symbol the oracle author places there
 * as the query target — which is stable under the formatter and lands inside the
 * token the provider must answer for. An anchorless / identifier-free line (an
 * own-line marker) keeps its raw position.
 */

import { DocumentPositions } from "@verter/lsp-test-client";

import { AnchorError, stripAnchors } from "../anchors.js";

/** A maximal ASCII TypeScript identifier run (the oracle files are ASCII). */
const IDENTIFIER = /[A-Za-z_$][A-Za-z0-9_$]*/g;

/** The start column of the LAST identifier in `lineCode`, or `null` if it has none. */
function lastIdentifierColumn(lineCode: string): number | null {
  let start: number | null = null;
  IDENTIFIER.lastIndex = 0;
  for (let match = IDENTIFIER.exec(lineCode); match !== null; match = IDENTIFIER.exec(lineCode)) {
    start = match.index;
  }
  return start;
}

/** A stripped oracle `.ts` source plus each anchor as a UTF-8 byte offset. */
export interface PreparedOracleSource {
  /** The oracle source with every `@dx-anchor` comment removed. */
  readonly stripped: string;
  /** Anchor name → UTF-8 byte offset into {@link stripped} (the bridge's offset space). */
  readonly byteOffsets: ReadonlyMap<string, number>;
}

/**
 * Strip an oracle `.ts` source's anchors and resolve each to the UTF-8 byte offset
 * the baseline bridge addresses. Total: an anchorless source yields an empty map.
 *
 * @throws {AnchorError} if an anchor name appears more than once (from {@link stripAnchors}).
 */
export function prepareOracleSource(source: string): PreparedOracleSource {
  const { stripped, anchors } = stripAnchors(source);
  const doc = new DocumentPositions(stripped);
  const lines = stripped.split(/\r\n|\r|\n/);
  const byteOffsets = new Map<string, number>();
  for (const [name, pos] of anchors) {
    // Resolve to the start of the anchored line's last identifier (the query
    // target), falling back to the raw column when the line has no identifier.
    const lineCode = (lines[pos.line] ?? "").slice(0, pos.character);
    const character = lastIdentifierColumn(lineCode) ?? pos.character;
    // Authored columns are UTF-16 code units; fold to the UTF-8 byte offset the
    // provider — and so the bridge — addresses with.
    byteOffsets.set(name, doc.positionToByte({ line: pos.line, character }, "utf-16"));
  }
  return { stripped, byteOffsets };
}

/**
 * Look up a prepared oracle anchor's byte offset, failing loudly when absent — an
 * oracle binding that references a missing `.ts` anchor is an authoring error, not
 * a silently-skipped query.
 *
 * @throws {AnchorError} if `anchor` is not present in the prepared source.
 */
export function requireOracleByteOffset(prepared: PreparedOracleSource, anchor: string): number {
  const offset = prepared.byteOffsets.get(anchor);
  if (offset === undefined) {
    throw new AnchorError(`required oracle anchor "${anchor}" not found`);
  }
  return offset;
}
