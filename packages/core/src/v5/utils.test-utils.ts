import { Position } from "source-map-js";
import { SourceMapConsumer } from "source-map-js/lib/source-map-consumer";
import { MagicString } from "vue/compiler-sfc";
import fs from "node:fs";

export function getSourceMap({ s }: { s: MagicString }) {
  return new SourceMapConsumer(
    s.generateMap({ hires: true, includeContent: true }) as any
  );
}

export function expectFindStringWithMap(
  toFind: string,
  {
    content,
    s,
    loc: { source },
  }: { content: string; s: MagicString; loc: { source: string } }
) {
  function getLineOffsets(text: string) {
    const lineOffsets: number[] = [];
    let isLineStart = true;

    for (let i = 0; i < text.length; i++) {
      if (isLineStart) {
        lineOffsets.push(i);
        isLineStart = false;
      }
      const ch = text.charAt(i);
      isLineStart = ch === "\r" || ch === "\n";
      if (ch === "\r" && i + 1 < text.length && text.charAt(i + 1) === "\n") {
        i++;
      }
    }

    if (isLineStart && text.length > 0) {
      lineOffsets.push(text.length);
    }

    return lineOffsets;
  }
  function clamp(num: number, min: number, max: number) {
    return Math.max(min, Math.min(max, num));
  }
  /**
   * Get the line and character based on the offset
   * @param offset The index of the position
   * @param text The text for which the position should be retrived
   * @param lineOffsets number Array with offsets for each line. Computed if not given
   */
  function positionAt(
    offset: number,
    text: string,
    lineOffsets = getLineOffsets(text)
  ): Position {
    offset = clamp(offset, 0, text.length);

    let low = 0;
    let high = lineOffsets.length;
    if (high === 0) {
      return {
        line: 1,
        column: offset,
      };
    }

    while (low <= high) {
      const mid = Math.floor((low + high) / 2);
      const lineOffset = lineOffsets[mid];

      if (lineOffset === offset) {
        return {
          line: mid + 1,
          column: 0,
        };
      } else if (offset > lineOffset) {
        low = mid + 1;
      } else {
        high = mid - 1;
      }
    }

    // low is the least x for which the line offset is larger than the current offset
    // or array.length if no line offset is larger than the current offset
    const line = low;
    const column =
      offset < lineOffsets[line]
        ? offset - lineOffsets[line - 1]
        : offset - lineOffsets[line];
    return { line, column };
  }

  const map = getSourceMap({ s });

  const index = content.indexOf(toFind);

  const pos = positionAt(index, content);
  const loc = map.originalPositionFor(pos);

  const sourceLines = getLineOffsets(source);

  const lineOffset = sourceLines[loc.line - 1] + loc.column;
  const mappedString = source.slice(lineOffset, lineOffset + toFind.length);
  expect(mappedString).toBe(toFind);
}

export function testSourceMaps({
  content,
  map,
}: {
  content: string;
  map: ReturnType<MagicString["generateMap"]>;
}) {
  fs.writeFileSync(
    "D:/Downloads/sourcemap-test/sourcemap-example.js",
    content,
    "utf-8"
  );
  fs.writeFileSync(
    "D:/Downloads/sourcemap-test/sourcemap-example.js.map",
    JSON.stringify(map),
    "utf-8"
  );
}
