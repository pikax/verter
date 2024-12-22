import { VerterASTBlock } from "@verter/core";
import { SourceMapConsumer } from "source-map-js";
import {
  Position as DocumentPosition,
  Range as DocumentRange,
} from "vscode-languageserver-textdocument";
import { createSubDocument } from "../../utils";

export type BlockId = "bundle" | "template" | "script" | "style";

export interface ProcessedBlock {
  type: BlockId | string;
  //   index: number;
  uri: string;
  languageId: string;
  blocks: Array<VerterASTBlock>;
}

export function processBlocks(
  uri: string,
  blocks: VerterASTBlock[]
): ProcessedBlock[] {
  const byTag = new Map<string, VerterASTBlock[]>();

  for (let i = 0; i < blocks.length; i++) {
    const block = blocks[i];
    const tag = block.tag.type;
    const arr = byTag.get(tag);
    if (arr) {
      arr.push(block);
    } else {
      byTag.set(tag, [block]);
    }
  }

  const result: ProcessedBlock[] = [];

  // template
  {
    const blocks = byTag.get("template");
    if (blocks) {
      byTag.delete("template");
      result.push({
        blocks,
        type: "template",
        languageId: "tsx",
        uri: createSubDocument(uri, "render.tsx"),
      });
    }
  }

  // script
  {
    const blocks = byTag.get("script");
    if (blocks) {
      byTag.delete("script");

      // all the blocks must be the same language
      const lang = blocks[0].block.attrs.lang;
      const languageId = lang === true ? "js" : lang ?? "js";

      result.push({
        blocks,
        languageId,
        type: "script",
        uri: createSubDocument(uri, "options.tsx"),
      });
    }
  }

  // style
  {
    const blocks = byTag.get("style");
    if (blocks) {
      byTag.delete("style");

      const byLang = new Map<string, VerterASTBlock[]>();
      for (let i = 0; i < blocks.length; i++) {
        const block = blocks[i];
        const language =
          block.block.attrs.lang === true
            ? "css"
            : block.block.attrs.lang ?? "css";
        const arr = byLang.get(language);
        if (arr) {
          arr.push(block);
        } else {
          byLang.set(language, [block]);
        }
      }

      for (const [languageId, langBlocks] of byLang) {
        result.push({
          languageId,
          type: "style",
          uri: createSubDocument(uri, "style." + languageId),
          blocks: langBlocks,
        });
      }
    }
  }

  // bespoke
  {
    const blocks = Array.from(byTag.values()).flat();
    if (blocks.length) {
      /**
       * TODO add json and other handlers
       * lang should be resolved by block.block.attrs.lang
       */
      result.push({
        type: "custom",
        blocks: Array.from(byTag.values()).flat(),
        languageId: "",
        uri: createSubDocument(uri, "custom.temp"),
      });
    }
  }

  return result;
}

export function toLowerCase(x: string) {
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
 * @returns DocumentPosition for the generated file, if the position falls outside of the map
 * character will return Infinite
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
    character: result.lastColumn ?? result.column,
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
