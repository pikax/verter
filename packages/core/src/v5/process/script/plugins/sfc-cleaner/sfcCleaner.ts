import { ElementNode, ElementTypes, NodeTypes } from "@vue/compiler-core";
import { definePlugin } from "../../types";

export const SFCCleanerPlugin = definePlugin({
  name: "VerterSFCCleaner",

  post(s, ctx) {
    const processType = new Set(["script"]);

    ctx.blocks.forEach((block) => {
      if (processType.has(block.type)) {
        // add extra line at the end and beginning of the block
        // this allows the comment lines to not affect the line numbers of the script block
        s.prependLeft(block.block.tag.pos.close.end, "\n");
        s.prependRight(block.block.tag.pos.open.start, "\n");

        return;
      }
      // comment out every line
      const content = s.original.slice(
        block.block.tag.pos.open.start,
        block.block.tag.pos.close.end
      );

      const lines = content.split("\n");
      let lineOffset = block.block.tag.pos.open.start;
      for (let i = 0; i < lines.length; i++) {
        const l = lines[i];
        const comment = "// ";

        s.appendRight(lineOffset, comment);
        lineOffset += l.length + 1;
        // stop if reached the end of the block
        if (lineOffset >= block.block.tag.pos.close.end) {
          break;
        }
      }
    });
  },
});
