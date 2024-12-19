import { VerterASTBlock } from "@verter/core";
import { createSubDocument } from "../../utils";

export type BlockId = "bundle" | "template" | "script" | "style";

export interface ProcessedBlock {
  id: BlockId | string;
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
    let arr = byTag.get(tag);
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
      result.push({
        blocks,
        id: "template",
        languageId: "tsx",
        uri: createSubDocument(uri, "render.tsx"),
      });
    }
  }

  // script
  {
    const blocks = byTag.get("script");
    if (blocks) {
      const lang = blocks[0].block.attrs.lang;
      const languageId = lang === true ? "js" : lang ?? "js";

      result.push({
        blocks,
        languageId,
        id: "script",
        uri: createSubDocument(uri, "options.tsx"),
      });
    }
  }

  // style
  {
    const byLang = "";
  }

  // bespoke
  {
  }

  return result;
}
