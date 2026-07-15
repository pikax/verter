/**
 * Position-encoding negotiation and coordinate conversion.
 *
 * LSP 3.17 lets a client advertise `general.positionEncodings` (a priority
 * list); the server picks one and reports it back in
 * `InitializeResult.capabilities.positionEncoding`. The `character` field of
 * every LSP `Position` is then measured in code units of that encoding:
 *
 *  - `utf-16` — UTF-16 code units (the spec default, and JS-string-native).
 *  - `utf-8`  — UTF-8 bytes.
 *  - `utf-32` — Unicode code points.
 *
 * Verter's own type-provider contract speaks in UTF-8 byte offsets, so a DX
 * harness needs to move between three coordinate spaces: source positions
 * (UTF-16 line/character, editor-native), UTF-8 byte offsets, and LSP
 * positions in whatever encoding was negotiated. {@link DocumentPositions}
 * owns those conversions for a single document.
 */

export type PositionEncoding = "utf-8" | "utf-16" | "utf-32";

/** The encoding assumed when a server omits `positionEncoding` (LSP 3.17). */
export const DEFAULT_POSITION_ENCODING: PositionEncoding = "utf-16";

const VALID_ENCODINGS: readonly PositionEncoding[] = ["utf-8", "utf-16", "utf-32"];

export interface LspPosition {
  line: number;
  character: number;
}

/** A minimal structural view of LSP `InitializeParams` for encoding injection. */
export interface InitializeParamsLike {
  capabilities?: {
    general?: { positionEncodings?: string[] } & Record<string, unknown>;
  } & Record<string, unknown>;
  [k: string]: unknown;
}

export function isPositionEncoding(value: unknown): value is PositionEncoding {
  return typeof value === "string" && (VALID_ENCODINGS as readonly string[]).includes(value);
}

/**
 * The client's default advertised encodings, in priority order. UTF-16 leads
 * (the LSP default, safe for any server); UTF-8 is offered so byte-accurate
 * servers like verter-lsp can select it.
 */
export function defaultClientPositionEncodings(): PositionEncoding[] {
  return ["utf-16", "utf-8"];
}

/**
 * Adopt the server's chosen encoding from an `InitializeResult`. Per LSP 3.17
 * the client MUST honour `capabilities.positionEncoding`; when it is missing or
 * unrecognised, the encoding defaults to UTF-16.
 */
export function adoptServerEncoding(serverChosen: unknown): PositionEncoding {
  return isPositionEncoding(serverChosen) ? serverChosen : DEFAULT_POSITION_ENCODING;
}

/**
 * Return a copy of `params` with `capabilities.general.positionEncodings` set,
 * preserving every other capability field. The input is never mutated.
 */
export function withPositionEncodings<T extends InitializeParamsLike>(
  params: T,
  encodings: readonly PositionEncoding[],
): T {
  const capabilities = { ...(params.capabilities ?? {}) };
  const general = { ...(capabilities.general ?? {}) };
  general.positionEncodings = [...encodings];
  capabilities.general = general;
  return { ...params, capabilities };
}

// ── Coordinate conversion ────────────────────────────────────────────────

function utf8Len(codePoint: number): number {
  if (codePoint <= 0x7f) return 1;
  if (codePoint <= 0x7ff) return 2;
  if (codePoint <= 0xffff) return 3;
  return 4;
}

function clamp(value: number, min: number, max: number): number {
  if (value < min) return min;
  if (value > max) return max;
  return value;
}

/** Index of the last element of `sorted` that is `<= target` (>= 0). */
function lastIndexAtMost(sorted: number[], target: number): number {
  let lo = 0;
  let hi = sorted.length - 1;
  let result = 0;
  while (lo <= hi) {
    const mid = (lo + hi) >> 1;
    if (sorted[mid] <= target) {
      result = mid;
      lo = mid + 1;
    } else {
      hi = mid - 1;
    }
  }
  return result;
}

/**
 * Encoding-aware coordinate converter over a single immutable text document.
 *
 * Coordinate spaces:
 *  - **UTF-16 offset** — a JS string index (editor-native).
 *  - **byte offset** — a UTF-8 byte index into the document (verter's contract).
 *  - **LSP position** — `{ line, character }` with `character` counted in the
 *    code units of a given {@link PositionEncoding} from the line start.
 *
 * Line terminators `\n`, `\r`, and `\r\n` all start a new line (per the LSP
 * spec). Out-of-range inputs are clamped; byte offsets that fall inside a
 * character clamp back to that character's start.
 */
export class DocumentPositions {
  readonly text: string;
  /** UTF-16 (JS string index) offset of each line start. */
  private readonly lineStartUtf16: number[];
  /** UTF-8 byte offset of each line start. */
  private readonly lineStartBytes: number[];
  private readonly totalBytes: number;

  constructor(text: string) {
    this.text = text;
    const starts16: number[] = [0];
    const startsBytes: number[] = [0];
    let bytes = 0;
    let i = 0;
    const n = text.length;
    while (i < n) {
      const code = text.charCodeAt(i);
      let codePoint = code;
      let u16 = 1;
      if (code >= 0xd800 && code <= 0xdbff && i + 1 < n) {
        const lo = text.charCodeAt(i + 1);
        if (lo >= 0xdc00 && lo <= 0xdfff) {
          codePoint = (code - 0xd800) * 0x400 + (lo - 0xdc00) + 0x10000;
          u16 = 2;
        }
      }
      bytes += utf8Len(codePoint);
      if (code === 0x0a) {
        // \n
        starts16.push(i + 1);
        startsBytes.push(bytes);
      } else if (code === 0x0d) {
        // \r — consume a following \n as part of the same terminator.
        if (i + 1 < n && text.charCodeAt(i + 1) === 0x0a) {
          bytes += 1;
          i += 1;
        }
        starts16.push(i + 1);
        startsBytes.push(bytes);
      }
      i += u16;
    }
    this.lineStartUtf16 = starts16;
    this.lineStartBytes = startsBytes;
    this.totalBytes = bytes;
  }

  /** Number of lines (always >= 1). */
  get lineCount(): number {
    return this.lineStartUtf16.length;
  }

  private characterFor(slice: string, encoding: PositionEncoding): number {
    if (encoding === "utf-16") return slice.length;
    if (encoding === "utf-32") return [...slice].length;
    let bytes = 0;
    for (const ch of slice) bytes += utf8Len(ch.codePointAt(0)!);
    return bytes;
  }

  /** UTF-16 (JS string index) offset → UTF-8 byte offset. */
  utf16ToByte(utf16Offset: number): number {
    const offset = clamp(utf16Offset, 0, this.text.length);
    const line = lastIndexAtMost(this.lineStartUtf16, offset);
    const lineStart16 = this.lineStartUtf16[line];
    const slice = this.text.slice(lineStart16, offset);
    return this.lineStartBytes[line] + Buffer.byteLength(slice, "utf-8");
  }

  /** UTF-8 byte offset → UTF-16 (JS string index) offset. */
  byteToUtf16(byteOffset: number): number {
    const target = clamp(byteOffset, 0, this.totalBytes);
    const line = lastIndexAtMost(this.lineStartBytes, target);
    let remaining = target - this.lineStartBytes[line];
    let i = this.lineStartUtf16[line];
    const n = this.text.length;
    while (i < n && remaining > 0) {
      const codePoint = this.text.codePointAt(i)!;
      const u16 = codePoint > 0xffff ? 2 : 1;
      const b = utf8Len(codePoint);
      if (b > remaining) break; // mid-character → clamp to its start
      remaining -= b;
      i += u16;
    }
    return i;
  }

  /** UTF-16 offset → LSP position whose `character` is in `encoding` units. */
  utf16ToPosition(utf16Offset: number, encoding: PositionEncoding): LspPosition {
    const offset = clamp(utf16Offset, 0, this.text.length);
    const line = lastIndexAtMost(this.lineStartUtf16, offset);
    const character = this.characterFor(
      this.text.slice(this.lineStartUtf16[line], offset),
      encoding,
    );
    return { line, character };
  }

  /** LSP position (in `encoding`) → UTF-16 offset. */
  positionToUtf16(pos: LspPosition, encoding: PositionEncoding): number {
    const line = clamp(Math.trunc(pos.line), 0, this.lineStartUtf16.length - 1);
    const lineStart16 = this.lineStartUtf16[line];
    // The end of the addressable line content excludes the line terminator. The
    // next line start sits just after the terminator, so strip a trailing
    // \r\n / \n / \r — an over-long `character` then clamps to the line length
    // (LSP 3.17) instead of walking across the newline into the next line.
    let lineEnd16 =
      line + 1 < this.lineStartUtf16.length ? this.lineStartUtf16[line + 1] : this.text.length;
    if (lineEnd16 > lineStart16) {
      const last = this.text.charCodeAt(lineEnd16 - 1);
      if (last === 0x0a) {
        lineEnd16 -= 1;
        if (lineEnd16 > lineStart16 && this.text.charCodeAt(lineEnd16 - 1) === 0x0d) lineEnd16 -= 1;
      } else if (last === 0x0d) {
        lineEnd16 -= 1;
      }
    }
    const target = Math.max(0, Math.trunc(pos.character));
    let units = 0;
    let i = lineStart16;
    while (i < lineEnd16 && units < target) {
      const codePoint = this.text.codePointAt(i)!;
      const u16 = codePoint > 0xffff ? 2 : 1;
      const add = encoding === "utf-8" ? utf8Len(codePoint) : encoding === "utf-32" ? 1 : u16;
      if (units + add > target) break; // clamp to character boundary
      units += add;
      i += u16;
    }
    return i;
  }

  /** UTF-8 byte offset → LSP position in `encoding`. */
  byteToPosition(byteOffset: number, encoding: PositionEncoding): LspPosition {
    return this.utf16ToPosition(this.byteToUtf16(byteOffset), encoding);
  }

  /** LSP position (in `encoding`) → UTF-8 byte offset. */
  positionToByte(pos: LspPosition, encoding: PositionEncoding): number {
    return this.utf16ToByte(this.positionToUtf16(pos, encoding));
  }

  /** Source (editor / UTF-16) position → UTF-16 offset. */
  sourceToUtf16(pos: LspPosition): number {
    return this.positionToUtf16(pos, "utf-16");
  }

  /** Source (editor / UTF-16) position → UTF-8 byte offset. */
  sourceToByte(pos: LspPosition): number {
    return this.utf16ToByte(this.sourceToUtf16(pos));
  }
}

/** One-shot convenience: UTF-8 byte offset → LSP position in `encoding`. */
export function byteOffsetToPosition(
  text: string,
  byteOffset: number,
  encoding: PositionEncoding,
): LspPosition {
  return new DocumentPositions(text).byteToPosition(byteOffset, encoding);
}

/** One-shot convenience: LSP position (in `encoding`) → UTF-8 byte offset. */
export function positionToByteOffset(
  text: string,
  position: LspPosition,
  encoding: PositionEncoding,
): number {
  return new DocumentPositions(text).positionToByte(position, encoding);
}
