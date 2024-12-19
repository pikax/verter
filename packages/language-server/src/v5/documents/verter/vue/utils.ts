import { VerterASTBlock } from "@verter/core";

export type BlockId = "bundle" | "template" | "script" | "style";

export interface ProcessedBlock {
  id: BlockId | string;
  index: number;
  uri: string;
  languageId: string;
  block: VerterASTBlock;
}

export function processBlocks(blocks: VerterASTBlock[]): ProcessedBlock[] {
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

  {
    const block = byTag.get("template")[0];
    if (block) {
      result.push({
        id: "template",
        languageId: "tsx",
        index: blocks.indexOf(block),
        block,
      });
    }
  }

  return result;
}
