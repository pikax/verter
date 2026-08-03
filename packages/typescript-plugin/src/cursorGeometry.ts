/** Bounded framework-tag attribute-name classification (maximum 256 UTF-8 bytes). */
export function isFrameworkAttributeNamePosition(
  source: string | undefined,
  position: number,
): boolean {
  if (source === undefined || position <= 0 || position > source.length) return false;
  let floor = position;
  let byteCount = 0;
  while (floor > 0) {
    const trailing = source.charCodeAt(floor - 1);
    let width = 1;
    let units = 1;
    if (trailing >= 0xdc00 && trailing <= 0xdfff && floor > 1) {
      const leading = source.charCodeAt(floor - 2);
      if (leading >= 0xd800 && leading <= 0xdbff) {
        width = 4;
        units = 2;
      }
    } else if (trailing > 0x7f) {
      width = trailing <= 0x7ff ? 2 : 3;
    }
    if (byteCount + width > 256) break;
    byteCount += width;
    floor -= units;
  }
  const prefix = source.slice(floor, position);
  const localTagStart = prefix.lastIndexOf("<");
  if (localTagStart < 0 || prefix.lastIndexOf(">") > localTagStart) return false;
  const tagStart = floor + localTagStart;
  const first = source.charCodeAt(tagStart + 1);
  if (first === 47 || first === 33 || first === 63) return false;

  let quote = 0;
  let braceDepth = 0;
  for (let offset = tagStart + 1; offset < position; offset++) {
    const code = source.charCodeAt(offset);
    if (quote !== 0) {
      if (code === 92) offset += 1;
      else if (code === quote) quote = 0;
      continue;
    }
    if (code === 34 || code === 39) quote = code;
    else if (code === 123) braceDepth += 1;
    else if (code === 125 && braceDepth > 0) braceDepth -= 1;
  }
  return quote === 0 && braceDepth === 0;
}
