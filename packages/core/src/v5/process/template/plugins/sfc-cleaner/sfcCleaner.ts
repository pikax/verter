import { ElementNode, ElementTypes, NodeTypes } from "@vue/compiler-core";
import { declareTemplatePlugin } from "../../template";

export const SFCCleanerPlugin = declareTemplatePlugin({
  name: "VerterSFCCleaner",

  post(s, ctx) {
    ctx.blocks.forEach((block) => {
      if (block === ctx.block) {
        return;
      }
      // comment out every line
      const content = s.original.slice(
        block.block.tag.pos.open.start,
        block.block.tag.pos.close.end
      );

      const lines = content.split("\n");
      let lineOffset = block.block.tag.pos.open.start;
      for (const l of lines) {
        // s.update(lineOffset, lineOffset + l.length, "// " + l);
        s.appendLeft(lineOffset, "// ");
        lineOffset += l.length + 1;
      }

      // if (block !== ctx.block) {
      //   s.update(
      //     block.block.tag.pos.open.start,
      //     block.block.tag.pos.close.end,
      //     ""
      //   );
      // }
    });
  },
});
