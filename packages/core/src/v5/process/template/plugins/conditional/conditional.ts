import { NodeTypes } from "@vue/compiler-core";
import {
  TemplateCondition,
  TemplateTypes,
} from "../../../../parser/template/types";
import { declareTemplatePlugin } from "../../template";
import { ParseTemplateContext } from "../../../../parser/template";

export const ConditionalPlugin = declareTemplatePlugin({
  name: "VerterConditional",

  narrows: [] as {
    index: number;
    condition: TemplateCondition[];
  }[],

  transformCondition(item, s, ctx) {
    console.log("sss", item);

    const element = item.element;
    const node = item.node;
    const rawName = node.rawName!;

    // move v-* to the beginning of the element
    s.move(
      node.loc.start.offset,
      node.loc.end.offset,
      element.loc.start.offset
    );

    // // remove v-
    s.remove(node.loc.start.offset, node.loc.start.offset + 2);

    // s.overwrite(
    //   node.loc.start.offset,
    //   node.loc.start.offset + rawName.length,
    //   "{"
    // );

    if (node.exp) {
      // remove =
      s.remove(
        node.loc.start.offset + rawName.length,
        node.loc.start.offset + rawName.length + 1
      );

      // update delimiters
      s.overwrite(
        node.exp.loc.start.offset - 1,
        node.exp.loc.start.offset,
        "("
      );
      s.overwrite(node.exp.loc.end.offset, node.exp.loc.end.offset + 1, "){");
      // s.overwrite(node.exp.loc.end.offset, node.exp.loc.end.offset + 1, ")?");

      // remove delimiters
      //   s.remove(node.exp.loc.start.offset - 1, node.exp.loc.start.offset);
      //   s.remove(node.exp.loc.end.offset, node.exp.loc.end.offset + 1);

      //   s.overwrite(node.exp.loc.end.offset, node.exp.loc.end.offset + 1, "?");
    } else {
      //   s.prependLeft(node.loc.end.offset, ": undefined");

      // add {  after else
      s.prependLeft(node.loc.start.offset + rawName.length, "{");
    }

    // switch (node.name) {
    //   case "if":
    //     // remove v-if
    //     // s.remove(node.loc.start.offset, node.loc.end.offset);

    //     break;
    //   case "else-if":
    //     // remove v-else-if
    //     s.remove(node.loc.start.offset, node.loc.end.offset);
    //     break;
    //   case "else":
    //     // remove v-else
    //     s.remove(node.loc.start.offset, node.loc.end.offset);
    //     break;
    // }

    s.prependLeft(element.loc.end.offset, "}");
  },

  transformProp(item, s, ctx) {
    if (
      item.node === null ||
      item.node.type !== NodeTypes.DIRECTIVE ||
      !item.node.exp ||
      !("context" in item)
    ) {
      return;
    }

    const context = item.context as ParseTemplateContext;
    if (context.conditions.length === 0) {
      return;
    }

    const node = item.node;

    // this.narrows.push({
    //   index: item.index,
    //   condition: {
    //     element: item.element,
    //     node: item.node,
    //   },
    // });
  },

  //   transform(item, s, ctx) {
  //     if (item.type === TemplateTypes.Condition) {
  //       return;
  //       //   this.transformCondition(item, s, ctx);
  //     }

  //   },
});
