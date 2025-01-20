import { ProcessItemType } from "../../../types";
import { definePlugin } from "../../types";

export const AttributesPlugin = definePlugin({
  name: "VerterAttributes",

  pre(s, ctx) {
    const tag = ctx.block.block.tag;
    const isTS = ctx.block.lang.startsWith("ts");
    const generic = ctx.generic;

    const attribute = tag.attributes.attributes;
    if (!attribute || !attribute.value) return;
    ctx.handledAttributes?.add("attributes");

    const prefix = ctx.prefix("");
    if (isTS) {
      s.prependRight(attribute.start, `;type ${prefix}`);

      if (generic) {
        s.prependRight(attribute.key.end, `<${generic.source}>`);
      }

      // remove delimiter
      s.remove(attribute.value.start - 1, attribute.value.start);

      s.overwrite(attribute.value.end, attribute.value.end + 1, ";");

      s.move(attribute.start, attribute.end, tag.pos.close.end);
    } else {
      const moveTo = tag.pos.close.end;
      // s.prependLeft(moveTo, `/** @typedef {${prefix}} ${attribute} */`);
      s.prependLeft(moveTo, `/** @typedef `);
      if (attribute.value) {
        // update delimiters to {}
        s.overwrite(attribute.value.start - 1, attribute.value.start, "{");
        s.overwrite(attribute.value.end, attribute.value.end + 1, "}");

        s.move(attribute.value.start - 1, attribute.value.end + 1, moveTo);

        // remove =
        s.remove(attribute.key.end, attribute.value.start - 1);
      } else {
        s.prependLeft(moveTo, `{}`);
      }

      s.prependRight(attribute.key.start, `${prefix}`);
      s.prependLeft(attribute.key.end, `*/`);
      s.move(attribute.key.start, attribute.key.end, moveTo);
    }
  },
});
