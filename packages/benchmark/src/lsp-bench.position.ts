/**
 * Re-express the benchmark's configured probe target in the negotiated LSP
 * position encoding.
 */
import {
  DocumentPositions,
  type LspPosition,
  type PositionEncoding,
} from "@verter/lsp-test-client";

/**
 * Convert a configured hover/definition target into an LSP position measured in
 * the encoding negotiated at `initialize`.
 *
 * **Config contract.** The benchmark configures its probe target as a 1-based
 * UTF-16 code-unit `line`/`character` (editor-native; `lsp-bench.config.ts`
 * subtracts 1 to make it 0-based). The LSP `Position.character` field, however,
 * is counted in code units of the encoding the server selects from the client's
 * advertised `general.positionEncodings` — Verter chooses `utf-8`. On a line
 * whose text before the target column contains non-ASCII characters, the UTF-8
 * byte offset diverges from the UTF-16 code-unit count, so sending the raw
 * config `character` verbatim would probe the wrong column. Routing every
 * position-send through this conversion keeps the probe on the intended token
 * regardless of the negotiated encoding. Under `utf-16` the conversion is the
 * identity, so ASCII targets (and any utf-16 server) are unaffected.
 */
export function toNegotiatedPosition(
  text: string,
  sourcePosition: LspPosition,
  encoding: PositionEncoding,
): LspPosition {
  const doc = new DocumentPositions(text);
  // UTF-16 (editor-native) position → UTF-16 offset → position in `encoding`.
  return doc.utf16ToPosition(doc.sourceToUtf16(sourcePosition), encoding);
}
