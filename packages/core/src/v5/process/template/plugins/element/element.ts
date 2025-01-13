import { ElementTypes } from "@vue/compiler-core";
import { ParseTemplateContext } from "../../../../parser/template";
import { declareTemplatePlugin, TemplateContext } from "../../template";
import { BlockPlugin } from "../block";
import { TemplateElement } from "../../../../parser/template/types";
import { MagicString } from "@vue/compiler-sfc";

export const ElementPlugin = declareTemplatePlugin({
  name: "VeterElement",

  transformElement(item, s, ctx) {
    if (item.node.tagType !== ElementTypes.COMPONENT) {
      return;
    }
    const node = item.node;

    const tagNameStart = node.loc.start.offset + 1;
    const tagNameEnd = tagNameStart + item.tag.length;

    const offset = node.loc.start.offset;
    const closingTagStartIndex = node.isSelfClosing
      ? -1
      : node.loc.source.lastIndexOf(
          `</${item.tag}`,
          node.children.at(-1)?.loc.end.offset ?? undefined
        ) + 2;

    const shouldWrap = item.tag.includes("-");

    console.log("item", node.loc.source);

    const componentAccessor = ctx.retrieveAccessor("COMPONENT_CTX");
    s.prependRight(
      node.loc.start.offset + 1,
      `${componentAccessor}${shouldWrap ? '["' : "."}`
    );
    if (!node.isSelfClosing) {
      s.prependRight(offset + closingTagStartIndex, `${componentAccessor}.`);
    }

    if (~item.tag.indexOf(".")) {
      renameElementTag(item, ".", closingTagStartIndex, s, ctx);
    } else if (shouldWrap) {
      renameElementTag(item, "-", closingTagStartIndex, s, ctx);
      s.prependLeft(tagNameEnd, '"]');
    }
  },
});

// type Delimiter = "." | "-";
// const DelimitersReplacer: Record<Delimiter, string> = {
//   ".": "l__verter__l",
//   "-": "O__verter__O",
// };
const Replacer = "l__verter__l";

function renameElementTag(
  item: TemplateElement,
  delimiter: "." | "-", // delimiter to use
  closingTagStartIndex: number,
  s: MagicString,
  ctx: TemplateContext
) {
  const node = item.node;

  const tagNameStart = node.loc.start.offset + 1;
  const tagNameEnd = tagNameStart + item.tag.length;
  const offset = node.loc.start.offset;

  const shouldWrap = delimiter === "-";

  if (!item.condition) {
    BlockPlugin.addItem(node, item, item.context as ParseTemplateContext);
  }

  // move tag to the beginning and then add {{
  s.move(tagNameStart, tagNameEnd, node.loc.start.offset);

  // just to make it easier to replace later
  const newName =
    delimiter === "."
      ? item.tag.replace(/\./g, Replacer)
      : item.tag.replace(/-/g, "").toUpperCase();
  s.prependLeft(tagNameStart, newName);

  s.prependRight(tagNameStart, `const ${newName}=`);
  s.prependLeft(tagNameEnd, ";");

  if (!node.isSelfClosing) {
    // update closing tag
    // add ; because this will also be moved
    s.prependLeft(offset + closingTagStartIndex + item.tag.length, ";");
    // move to beginning
    s.move(
      offset + closingTagStartIndex,
      offset + closingTagStartIndex + item.tag.length,
      node.loc.start.offset
    );

    s.prependLeft(offset + closingTagStartIndex, newName);
  }
}
