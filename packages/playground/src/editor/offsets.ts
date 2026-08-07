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
