import { ParsedBlockScript } from "../../../../parser/types";
import { definePlugin } from "../../types";

export const ScriptBlockPlugin = definePlugin({
  name: "VerterScriptBlock",

  // clean the script tag
  pre(s, ctx) {
    const tag = ctx.block.block.tag;

    // replace < with function
    s.overwrite(
      tag.pos.open.start,
      tag.pos.open.start + 1,
      `${ctx.isAsync ? "async " : ""}function `
    );
    s.appendRight(tag.pos.open.start, ";");


    // replace > with (){
    s.overwrite(tag.pos.open.end - 1, tag.pos.open.end, "(){");

    // replace </script> with }
    s.update(tag.pos.close.start, tag.pos.close.end, "");
    s.prependRight(tag.pos.close.start, "}");
  },

  post(s, ctx) {
    // move non-main to the top
    const notMainScripts = ctx.blocks.filter(
      (block) =>
        block.type === "script" && (block as ParsedBlockScript).isMain === false
    ) as ParsedBlockScript[];

    if (notMainScripts.length) {
      for (const script of notMainScripts) {
        const start = script.block.tag.pos.open.start;
        const end = script.block.tag.pos.close.end;
        s.move(start, end, 0);

        // clean tags, the main script will be handled by other plugin
        s.remove(
          script.block.tag.pos.open.start,
          script.block.tag.pos.open.end
        );
        s.remove(
          script.block.tag.pos.close.start,
          script.block.tag.pos.close.end
        );
      }
    }

    // remove attributes
    const attributes = ctx.block.block.tag.attributes;
    for (const key in attributes) {
      if (ctx.handledAttributes?.has(key)) continue;
      const attr = attributes[key];

      if (key === "generic") {
        if (attr.value) {
          s.remove(attr.start, attr.value.start - 1);
          s.overwrite(attr.value.start - 1, attr.value.start, "<");
          s.overwrite(attr.value.end, attr.end, ">");
          continue;
        }
      }

      s.remove(attr.start, attr.end);
    }
  },
});
