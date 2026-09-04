import type { DocumentStructureResponseV1, StructureRangeV1 } from "@verter/language-shared";

export type StyleLang = "css" | "scss" | "less" | "sass" | "stylus" | "postcss";

export interface StyleBlockInfo {
  /** The parser's sealed block identity — the only key consumers join on. */
  blockToken: string;
  /**
   * The block's dialect, or `null` when the parser reported one this client
   * cannot address. Consumers fail closed on `null` — see [`styleLang`].
   */
  lang: StyleLang | null;
  scoped: boolean;
  /**
   * Parser-owned `src` attribute: the block's content is an EXTERNAL file
   * and the inline slice is framework-IGNORED. External-src blocks yield NO
   * inline slice — consumers must not slice, validate, transpile, or send
   * overrides for them (typed unavailable, mirroring the host's
   * `ExternalSrcDeferred` state).
   */
  externalSrc: boolean;
  contentStartLine: number;
  contentStartColumn: number;
  contentStartOffset: number;
  contentEndOffset: number;
  langAttributeRange?: { startLine: number; startCol: number; endLine: number; endCol: number };
}

function utf8OffsetToUtf16(source: string, byteOffset: number): number {
  let bytes = 0;
  let utf16 = 0;
  for (const scalar of source) {
    if (bytes >= byteOffset) break;
    const codePoint = scalar.codePointAt(0)!;
    bytes += codePoint <= 0x7f ? 1 : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
    utf16 += scalar.length;
  }
  return utf16;
}

function toOffset(source: string, range: StructureRangeV1): number {
  return utf8OffsetToUtf16(source, range.start);
}

function lineCol(source: string, offset: number): { line: number; col: number } {
  let line = 0;
  let start = 0;
  for (let index = 0; index < offset; index += 1) {
    if (source.charCodeAt(index) === 10) {
      line += 1;
      start = index + 1;
    }
  }
  return { line, col: offset - start };
}

/**
 * Map the parser's reported dialect onto the closed set this client can serve.
 *
 * Exhaustive rather than defaulting: an unrecognised dialect used to fall
 * through to `"css"`, so a block whose `lang` names no dialect at all was
 * parsed and validated by the plain-CSS service and reported CSS errors for
 * syntax that was never CSS. An unknown dialect returns `null` and the block is
 * served nothing — missing intelligence beats confidently wrong intelligence.
 *
 * `"missing"` is that case, not the absence of a `lang` attribute. The parser
 * reports a block with NO `lang` as `"css"`; `"missing"` is what it reports for
 * a `lang` it does not recognise, which is exactly the block this client has no
 * service for.
 */
function styleLang(dialect: string): StyleLang | null {
  switch (dialect.toLowerCase()) {
    case "css":
      return "css";
    case "scss":
      return "scss";
    case "less":
      return "less";
    case "sass":
      return "sass";
    case "stylus":
      return "stylus";
    case "postcss":
      return "postcss";
    default:
      return null;
  }
}

export function styleBlocksFromStructure(
  source: string,
  response: DocumentStructureResponseV1,
): StyleBlockInfo[] {
  if (response.kind !== "available") return [];
  const result: StyleBlockInfo[] = [];
  for (const block of response.structure.blocks) {
    if (block.kind !== "section" || block.section.role.kind !== "style") continue;
    const contentStartOffset = toOffset(source, block.section.contentRange);
    const contentEndOffset = utf8OffsetToUtf16(source, block.section.contentRange.end);
    const start = lineCol(source, contentStartOffset);
    const langAttribute = block.section.attributes.find(
      (attribute) => attribute.kind === "named" && attribute.name?.normalized === "lang",
    );
    const langRange = langAttribute && {
      start: lineCol(source, toOffset(source, langAttribute.fullRange)),
      end: lineCol(source, utf8OffsetToUtf16(source, langAttribute.fullRange.end)),
    };
    const externalSrc = block.section.attributes.some(
      (attribute) => attribute.kind === "named" && attribute.name?.normalized === "src",
    );
    result.push({
      blockToken: block.section.blockToken,
      lang: styleLang(block.section.role.dialect),
      scoped: block.section.role.scoped,
      externalSrc,
      contentStartLine: start.line,
      contentStartColumn: start.col,
      contentStartOffset,
      contentEndOffset,
      ...(langRange
        ? {
            langAttributeRange: {
              startLine: langRange.start.line,
              startCol: langRange.start.col,
              endLine: langRange.end.line,
              endCol: langRange.end.col,
            },
          }
        : {}),
    });
  }
  return result;
}

export function findStyleBlockAt(
  blocks: StyleBlockInfo[],
  source: string,
  line: number,
  column: number,
): StyleBlockInfo | undefined {
  const lines = source.split("\n");
  if (line < 0 || line >= lines.length) return undefined;
  let offset = column;
  for (let index = 0; index < line; index += 1) offset += lines[index].length + 1;
  return blocks.find(
    (block) => offset >= block.contentStartOffset && offset <= block.contentEndOffset,
  );
}
