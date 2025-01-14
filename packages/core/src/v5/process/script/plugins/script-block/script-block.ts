import { ParsedBlockScript } from "../../../../parser/types";
import { definePlugin } from "../../types";

export const ScriptBlockPlugin = definePlugin({
  name: "VerterScriptBlock",

  post(s, ctx) {
    // move non-main to the top
    const scripts  = ctx.blocks.filter((block) => block.type === "script") as ParsedBlockScript[];
    if(scripts.length < 2) return;

    const notMain = scripts.filter((block) => block.isMain === false);
    for (const script of notMain) {
        const start = script.block.tag.pos.open.start;
        const end = script.block.tag.pos.close.end;
        s.move(start, end, 0);

        // clean tags, the main script will be handled by other plugin
        s.remove(script.block.tag.pos.open.start, script.block.tag.pos.open.end);
        s.remove(script.block.tag.pos.close.start, script.block.tag.pos.close.end);
    }
  },
});
