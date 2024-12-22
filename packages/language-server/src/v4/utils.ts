import { SourceMapConsumer } from "source-map-js";

import {
  Position as DocumentPosition,
  Range as DocumentRange,
} from "vscode-languageserver-textdocument";

function toLowerCase(x: string) {
  return x.toLowerCase();
}
const fileNameLowerCaseRegExp = /[^\u0130\u0131\u00DFa-z0-9\\/:\-_\. ]+/g;
// FROM https://github.com/microsoft/TypeScript/blob/8192d550496d884263e292488e325ae96893dc78/src/compiler/core.ts#L1769-L1807
export function toFileNameLowerCase(x: string) {
  return fileNameLowerCaseRegExp.test(x)
    ? x.replace(fileNameLowerCaseRegExp, toLowerCase)
    : x;
}

/**
 * Receives the DocumentPosition for the original and returns the DocumentPosition of generated
 * for the generated file.
 * @param consumer SourceMapConsumer
 * @param pos DocumentPosition
 * @param bias number
 * @returns DocumentPosition for the generated file
 */
export function generatedPositionFor(
  consumer: SourceMapConsumer,
  pos: DocumentPosition,
  bias?: number
): DocumentPosition {
  const result = consumer.generatedPositionFor({
    line: pos.line + 1,
    column: pos.character,
    source: ".",
    bias,
  });

  return {
    line: result.line - 1,
    character: result.column,
  };
}

/**
 * Receives the DocumentRange for the orignal and returns the DocumentRange of generated
 * @param consumer SourceMapConsumer
 * @param range DocumentRange
 * @returns DocumentRange
 */
export function generatedRangeFor(
  consumer: SourceMapConsumer,
  range: DocumentRange
): DocumentRange {
  return {
    start: generatedPositionFor(consumer, range.start),
    end: generatedPositionFor(consumer, range.end),
  };
}

/**
 * Receives the DocumentPosition for the generated and returns the DocumentPosition of original
 * @param consumer SourceMapConsumer
 * @param pos DocumentPosition
 * @returns DocumentPosition for the original file
 */
export function originalPositionFor(
  consumer: SourceMapConsumer,
  pos: DocumentPosition
): DocumentPosition {
  const result = consumer.originalPositionFor({
    line: pos.line + 1,
    column: pos.character,
  });

  return {
    line: result.line - 1,
    character: result.column,
  };
}

/**
 * Receives the DocumentRange for the generated and returns the DocumentRange of original
 * @param consumer SourceMapConsumer
 * @param range DocumentRange
 * @returns DocumentRange
 */
export function originalRangeFor(
  consumer: SourceMapConsumer,
  range: DocumentRange
): DocumentRange {
  return {
    start: originalPositionFor(consumer, range.start),
    end: originalPositionFor(consumer, range.end),
  };
}
