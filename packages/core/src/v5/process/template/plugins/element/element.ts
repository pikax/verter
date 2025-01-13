import { ParseTemplateContext } from "../../../../parser/template";
import { declareTemplatePlugin } from "../../template";
import { BlockPlugin } from "../block";

export const ElementPlugin = declareTemplatePlugin({
  name: "VeterElement",

  transformElement(item, s, ctx) {
    const node = item.node;
    console.log("item", node.loc.source);

    if (~item.tag.indexOf(".")) {
      if (!item.condition) {
        BlockPlugin.addItem(
          node,
          item.parent,
          item.context as ParseTemplateContext
        );
      }

      // move tag to the beginning and then add {{
      const tagNameStart = node.loc.start.offset + 1;
      const tagNameEnd = tagNameStart + item.tag.length;

      s.move(tagNameStart, tagNameEnd, node.loc.start.offset);

      const Replacer = "l__verter__l";

      // just to make it easier to replace later
      const newName = item.tag.replace(/\./g, Replacer);
      s.prependLeft(tagNameStart, newName);

      s.prependRight(tagNameStart, `const ${newName}=`);
      s.prependLeft(tagNameEnd, ";");

      // update closing tag
      const closingTagIndex =
        node.loc.source.lastIndexOf(
          `</${item.tag}`,
          node.children.at(-1)?.loc.end.offset ?? undefined
        ) + 2;

      const offset = node.loc.start.offset;
      // add ; because this will also be moved
      s.prependLeft(offset + closingTagIndex + item.tag.length, ";");
      // move to beginning
      s.move(
        offset + closingTagIndex,
        offset + closingTagIndex + item.tag.length,
        node.loc.start.offset
      );

      s.prependLeft(offset + closingTagIndex, newName);
    }
  },
});
