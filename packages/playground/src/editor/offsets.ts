/**
 * UTF-8 byte ↔ UTF-16 code-unit offset conversion.
 *
 * Structure projections carry UTF-8 BYTE ranges (the wire contract); JavaScript
 * string indexing/slicing is UTF-16. Every comparison or slice that mixes the
 * two MUST convert first — mixing them mis-slices and mis-classifies as soon as
 * the source holds non-ASCII (or astral) characters.
 */

/** Convert a JS UTF-16 code-unit offset to the UTF-8 byte offset. */
export function utf16ToUtf8Offset(source: string, utf16Offset: number): number {
  return new TextEncoder().encode(source.slice(0, utf16Offset)).length;
}

/** Convert a UTF-8 byte offset to the JS UTF-16 code-unit offset. */
export function utf8ToUtf16Offset(source: string, byteOffset: number): number {
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

/**
 * Convert a UTF-8 byte offset to the JS UTF-16 code-unit offset, exact or
 * refused: `null` when the offset is past the end of `source` or inside a
 * multi-byte character. Use this for wire spans tagged `utf8.v1`; the lenient
 * {@link utf8ToUtf16Offset} snaps and so cannot refuse.
 */
export function utf8ToUtf16OffsetExact(source: string, byteOffset: number): number | null {
  if (!Number.isInteger(byteOffset) || byteOffset < 0) return null;
  let bytes = 0;
  let utf16 = 0;
  for (const scalar of source) {
    if (bytes === byteOffset) return utf16;
    if (bytes > byteOffset) return null;
    const codePoint = scalar.codePointAt(0)!;
    bytes += codePoint <= 0x7f ? 1 : codePoint <= 0x7ff ? 2 : codePoint <= 0xffff ? 3 : 4;
    utf16 += scalar.length;
  }
  return bytes === byteOffset ? utf16 : null;
}

/** Convert a half-open `utf8.v1` byte range to UTF-16, exact or refused. */
export function utf8ToUtf16RangeExact(
  source: string,
  startByte: number,
  endByte: number,
): { start: number; end: number } | null {
  if (startByte > endByte) return null;
  const start = utf8ToUtf16OffsetExact(source, startByte);
  const end = utf8ToUtf16OffsetExact(source, endByte);
  return start === null || end === null ? null : { start, end };
}
