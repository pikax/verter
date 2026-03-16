/**
 * Lightweight scanner that extracts `<style>` block positions from SFC source text.
 *
 * Used to detect which style block the cursor is in and to compute position offsets
 * between the SFC document and virtual CSS documents. Does NOT extract content —
 * content comes from Verter's virtual files.
 */

export type StyleLang = "css" | "scss" | "less" | "sass" | "stylus" | "postcss";

export interface StyleBlockInfo {
  /** 0-based index among all `<style>` blocks in the file. */
  index: number;
  /** Language of the style block (from `lang` attribute, defaults to "css"). */
  lang: StyleLang;
  /** Whether the block has the `scoped` attribute. */
  scoped: boolean;
  /** 0-based line where the content starts (the line after `>`). */
  contentStartLine: number;
  /** 0-based column where the content starts on contentStartLine. */
  contentStartColumn: number;
  /** UTF-16 offset of the first content character in the SFC source. */
  contentStartOffset: number;
  /** UTF-16 offset of the character just past the last content character (`<` of `</style>`). */
  contentEndOffset: number;
  /**
   * Range of the `lang="..."` attribute text (including key, `=`, and quotes)
   * within the SFC source. Present only when a `lang` attribute exists.
   * All values are 0-based.
   */
  langAttributeRange?: {
    startLine: number;
    startCol: number;
    endLine: number;
    endCol: number;
  };
}

const STYLE_TAG_RE = /<style\b([^>]*)>/g;
const STYLE_CLOSE_RE = /<\/style\s*>/g;

interface ParsedLang {
  lang: StyleLang;
  /** Offset of the `lang` attribute within the attrs string, or -1 if absent. */
  matchStart: number;
  /** Length of the full `lang="..."` attribute match. */
  matchLength: number;
}

function parseLang(attrs: string): ParsedLang {
  const m = /\blang\s*=\s*(?:"([^"]*)"|'([^']*)'|(\S+))/.exec(attrs);
  if (!m) return { lang: "css", matchStart: -1, matchLength: 0 };
  const raw = (m[1] ?? m[2] ?? m[3]).toLowerCase();
  let lang: StyleLang;
  switch (raw) {
    case "scss":
    case "less":
    case "sass":
    case "stylus":
    case "postcss":
      lang = raw;
      break;
    default:
      lang = "css";
      break;
  }
  return { lang, matchStart: m.index, matchLength: m[0].length };
}

function hasAttr(attrs: string, name: string): boolean {
  // Matches both `scoped` (bare) and `scoped="..."` (valued)
  return new RegExp(`\\b${name}\\b`).test(attrs);
}

/**
 * Scan SFC source and return metadata for every `<style>` block.
 *
 * Line/column are 0-based. Offsets are UTF-16 code units (JS string indices).
 */
export function scanStyleBlocks(source: string): StyleBlockInfo[] {
  const results: StyleBlockInfo[] = [];

  // Pre-compute line starts for offset → line:col conversion
  const lineStarts: number[] = [0];
  for (let i = 0; i < source.length; i++) {
    if (source.charCodeAt(i) === 10 /* \n */) {
      lineStarts.push(i + 1);
    }
  }

  function offsetToLineCol(offset: number): { line: number; col: number } {
    // Binary search for the line containing this offset
    let lo = 0;
    let hi = lineStarts.length - 1;
    while (lo < hi) {
      const mid = (lo + hi + 1) >> 1;
      if (lineStarts[mid] <= offset) {
        lo = mid;
      } else {
        hi = mid - 1;
      }
    }
    return { line: lo, col: offset - lineStarts[lo] };
  }

  let index = 0;
  STYLE_TAG_RE.lastIndex = 0;
  let openMatch: RegExpExecArray | null;

  while ((openMatch = STYLE_TAG_RE.exec(source)) !== null) {
    const attrs = openMatch[1];
    const contentStartOffset = openMatch.index + openMatch[0].length;

    // Find matching </style>
    STYLE_CLOSE_RE.lastIndex = contentStartOffset;
    const closeMatch = STYLE_CLOSE_RE.exec(source);
    if (!closeMatch) break; // Malformed — no closing tag

    const contentEndOffset = closeMatch.index;
    const { line, col } = offsetToLineCol(contentStartOffset);
    const parsed = parseLang(attrs);

    // Compute langAttributeRange if the lang attribute was found
    let langAttributeRange: StyleBlockInfo["langAttributeRange"];
    if (parsed.matchStart >= 0) {
      const attrsStartInOpenTag = openMatch[0].indexOf(attrs);
      if (attrsStartInOpenTag >= 0) {
        const attrsOffset = openMatch.index + attrsStartInOpenTag;
        const langAbsStart = attrsOffset + parsed.matchStart;
        const langAbsEnd = langAbsStart + parsed.matchLength;
        const start = offsetToLineCol(langAbsStart);
        const end = offsetToLineCol(langAbsEnd);
        langAttributeRange = {
          startLine: start.line,
          startCol: start.col,
          endLine: end.line,
          endCol: end.col,
        };
      }
    }

    results.push({
      index,
      lang: parsed.lang,
      scoped: hasAttr(attrs, "scoped"),
      contentStartLine: line,
      contentStartColumn: col,
      contentStartOffset,
      contentEndOffset,
      langAttributeRange,
    });

    index++;
    // Continue searching after the closing tag
    STYLE_TAG_RE.lastIndex = closeMatch.index + closeMatch[0].length;
  }

  return results;
}

/**
 * Find which `<style>` block (if any) contains the given 0-based line and column.
 */
export function findStyleBlockAt(
  blocks: StyleBlockInfo[],
  source: string,
  line: number,
  column: number,
): StyleBlockInfo | undefined {
  // Convert line:col to offset
  let offset = 0;
  let currentLine = 0;
  for (let i = 0; i < source.length; i++) {
    if (currentLine === line) {
      offset = i + column;
      break;
    }
    if (source.charCodeAt(i) === 10) {
      currentLine++;
    }
  }
  // Edge case: if line is beyond source, offset stays 0
  if (currentLine < line) return undefined;

  return blocks.find(
    (b) => offset >= b.contentStartOffset && offset <= b.contentEndOffset,
  );
}
