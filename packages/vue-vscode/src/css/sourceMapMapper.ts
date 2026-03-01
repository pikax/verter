/**
 * Bidirectional position mapping via source maps.
 *
 * Used to map between original Sass/Stylus source and compiled CSS output,
 * enabling position-accurate completions, hover, and diagnostics.
 */

import { SourceMapConsumer, type RawSourceMap } from "source-map";

export interface MappedPosition {
  line: number; // 1-based (source-map convention)
  column: number; // 0-based
}

/**
 * Map a position in the original source to a position in the generated output.
 *
 * @param sourceMap - The source map from the transpiler.
 * @param originalLine - 1-based line in the original source.
 * @param originalColumn - 0-based column in the original source.
 * @returns Position in the generated CSS, or null if unmapped.
 */
export async function originalToGenerated(
  sourceMap: RawSourceMap,
  originalLine: number,
  originalColumn: number,
): Promise<MappedPosition | null> {
  const consumer = await new SourceMapConsumer(sourceMap);
  try {
    const source = sourceMap.sources?.[0] ?? null;
    if (!source) return null;

    const pos = consumer.generatedPositionFor({
      source,
      line: originalLine,
      column: originalColumn,
    });

    if (pos.line == null || pos.column == null) return null;

    return { line: pos.line, column: pos.column };
  } finally {
    consumer.destroy();
  }
}

/**
 * Map a position in the generated output back to the original source.
 *
 * @param sourceMap - The source map from the transpiler.
 * @param generatedLine - 1-based line in the generated CSS.
 * @param generatedColumn - 0-based column in the generated CSS.
 * @returns Position in the original source, or null if unmapped.
 */
export async function generatedToOriginal(
  sourceMap: RawSourceMap,
  generatedLine: number,
  generatedColumn: number,
): Promise<MappedPosition | null> {
  const consumer = await new SourceMapConsumer(sourceMap);
  try {
    const pos = consumer.originalPositionFor({
      line: generatedLine,
      column: generatedColumn,
    });

    if (pos.line == null || pos.column == null) return null;

    return { line: pos.line, column: pos.column };
  } finally {
    consumer.destroy();
  }
}

/**
 * Map a range in the generated output back to the original source.
 */
export async function generatedRangeToOriginal(
  sourceMap: RawSourceMap,
  startLine: number,
  startColumn: number,
  endLine: number,
  endColumn: number,
): Promise<{
  start: MappedPosition;
  end: MappedPosition;
} | null> {
  const consumer = await new SourceMapConsumer(sourceMap);
  try {
    const startPos = consumer.originalPositionFor({
      line: startLine,
      column: startColumn,
    });
    const endPos = consumer.originalPositionFor({
      line: endLine,
      column: endColumn,
    });

    if (
      startPos.line == null ||
      startPos.column == null ||
      endPos.line == null ||
      endPos.column == null
    ) {
      return null;
    }

    return {
      start: { line: startPos.line, column: startPos.column },
      end: { line: endPos.line, column: endPos.column },
    };
  } finally {
    consumer.destroy();
  }
}
