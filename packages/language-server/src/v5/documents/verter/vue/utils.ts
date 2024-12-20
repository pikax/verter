import { VerterASTBlock } from "@verter/core";
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
