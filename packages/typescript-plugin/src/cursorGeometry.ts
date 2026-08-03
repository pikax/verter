/**
 * Bounded framework-tag attribute-name classification.
 *
 * The tag anchor is a PARSER-IDENTIFIED opening span supplied by the caller
 * (the store-published structure stamp's `markup_opening_ranges`) — this
 * helper never rediscovers `<` from raw source. Inside that span it lexes at
 * most 256 UTF-8 bytes of lookback to classify the unfinished token: a
 * boolean answer, no delimiter discovery, no persistent geometry.
 */
/**
 * Convert stamp span endpoints from UTF-8 BYTE offsets to UTF-16 code-unit
 * offsets in `source`.
 *
 * The stamped opening ranges are produced by the Rust side from inventory
 * byte spans (`carrier_sync.rs`); tsserver positions are UTF-16 code units.
 * Comparing them raw shifts every span after the first non-ASCII character
 * (TE-C-13 class). One forward pass; an endpoint that does not land on a
 * character boundary, or lies past the end of the source, clamps forward —
 * the fail-closed direction (it can only shrink a span, never widen it).
 */
function utf8RangesToUtf16(
  source: string,
  ranges: readonly (readonly [number, number])[],
): [number, number][] {
  const endpoints: number[] = [];
  for (const [start, end] of ranges) endpoints.push(start, end);
  endpoints.sort((a, b) => a - b);
  const map = new Map<number, number>();
  let next = 0;
  while (next < endpoints.length && endpoints[next] <= 0) {
    map.set(endpoints[next], 0);
    next += 1;
  }
  let bytes = 0;
  let index = 0;
  while (index < source.length && next < endpoints.length) {
    const code = source.codePointAt(index)!;
    bytes += code <= 0x7f ? 1 : code <= 0x7ff ? 2 : code <= 0xffff ? 3 : 4;
    index += code > 0xffff ? 2 : 1;
    while (next < endpoints.length && endpoints[next] <= bytes) {
      map.set(endpoints[next], index);
      next += 1;
    }
  }
  while (next < endpoints.length) {
    map.set(endpoints[next], source.length);
    next += 1;
  }
  return ranges.map(([start, end]) => [
    map.get(start) ?? source.length,
    map.get(end) ?? source.length,
  ]);
}

export function isFrameworkAttributeNamePosition(
  source: string | undefined,
  position: number,
  openingRanges: readonly (readonly [number, number])[] | undefined,
): boolean {
  if (source === undefined || position <= 0 || position > source.length) return false;
  if (openingRanges === undefined) return false;

  // The innermost parser-identified opening span owning the position. The
  // stamped spans are UTF-8 byte offsets; convert before comparing against
  // the UTF-16 position.
  let tagStart = -1;
  for (const [start, end] of utf8RangesToUtf16(source, openingRanges)) {
    if (position > start && position <= end && start > tagStart) {
      tagStart = start;
    }
  }
  if (tagStart < 0) return false;

  // Bounded lookback: at most 256 UTF-8 bytes before the position. If the
  // parser-identified tag start is farther back than the bound allows, the
  // classification fails closed rather than lexing an unbounded window.
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
  if (tagStart < floor) return false;

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
    // A `>` outside quotes/braces closes the opening tag before the
    // position: not an attribute-name position.
    else if (code === 62 && braceDepth === 0) return false;
  }
  return quote === 0 && braceDepth === 0;
}
