import { TemplatePlugin } from "../../template";

export const TemplateTagPlugin = {
  name: "VerterTemplateTag",
  pre(s, context) {
    const pos = context.block.block.tag.pos;
    // replace <
    s.overwrite(pos.open.start, pos.open.start + 1, "function ");
    s.update(pos.open.end - 1, pos.open.end, `(){`) ;


    // move generic to template
    if(context.generic) {
      s.prependLeft(pos.open.end -1, `<`);
      s.move(context.generic.position.start, context.generic.position.end, pos.open.end - 1);
      s.prependRight(pos.open.end -1, `>`);
    }

    // replace closing tag
    s.overwrite(pos.close.start, pos.close.end, "}");
  },
} as TemplatePlugin;
