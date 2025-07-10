import { ElementNode } from "@vue/compiler-core";
import { ParseTemplateContext } from "../../../../parser/template";
import { declareTemplatePlugin, TemplateContext } from "../../template";
import { BlockPlugin } from "../block";
import { ProcessItemType } from "../../../types";

export const LoopPlugin = declareTemplatePlugin({
  name: "VerterLoop",

  transformLoop(item, s, ctx) {
    const forParseResult = item.node.forParseResult;

    const renderList = ctx.retrieveAccessor("renderList");

    ctx.items.push({
      type: ProcessItemType.Import,
      from: "vue",
      items: [{ name: "renderList", alias: renderList }],
    });

    // move v-for beginnning
    s.move(
      item.node.loc.start.offset,
      item.node.loc.end.offset,
      item.element.loc.start.offset
    );

    // replace v-for with {renderList}
    s.overwrite(
      item.node.loc.start.offset,
      item.node.loc.start.offset + 5,
      renderList
    );

    // // wrap in a block
    // if (item.context.conditions.length > 0 && ctx.doNarrow) {
    //   ctx.doNarrow(
    //     {
    //       index: item.node.loc.start.offset,
    //       inBlock: true,
    //       conditions: item.context.conditions,
    //       type: "prepend",
    //       direction: "right",
    //       condition: null,
    //     },
    //     s
    //   );
    // }

    // s.prependRight(item.node.loc.start.offset, "{()=>{");

    // s.prependLeft(item.element.loc.end.offset, "}}");

    // replace '=' with '('
    s.overwrite(
      item.node.loc.start.offset + 5,
      item.node.loc.start.offset + 6,
      "("
    );

    // s.update(
    //   item.node.exp!.loc.end.offset,
    //   item.node.exp!.loc.end.offset + 1,
    //   ")"
    // );

    // remove last delimiter, it will be added by the block plugin
    s.remove(item.node.exp!.loc.end.offset, item.node.exp!.loc.end.offset + 1);

    const { key, index, value, source } = forParseResult;
    const vforSource = item.node.loc.source;

    // move source to the beginning
    s.move(
      source.loc.start.offset,
      source.loc.end.offset,
      item.node.loc.start.offset + 6
    );

    // find in or of and replace with ,
    let inOfIndex = -1;
    const tokens = ["in", "of"];

    const fromIndex = Math.max(
      key?.loc.end.offset ?? 0,
      index?.loc.end.offset ?? 0,
      value?.loc.end.offset ?? 0
    );

    const condition = vforSource.slice(fromIndex - item.node.loc.start.offset);
    for (let i = 0; i < tokens.length; i++) {
      const token = tokens[i];
      const index = condition.indexOf(token);
      if (index !== -1) {
        inOfIndex = index;
        break;
      }
    }

    const inOfStart = fromIndex + inOfIndex;
    const inOfEnd = inOfStart + 2;

    // move , to start of expression
    s.move(inOfStart, inOfEnd, item.node.loc.start.offset + 6);
    // replace in/of with ,
    s.overwrite(inOfStart, inOfEnd, ",");

    // check if needs to be wrapped in ()
    const shouldWrapParams = !condition.startsWith(")");

    // update start delimiters
    s.update(
      item.node.exp!.loc.start.offset - 1,
      item.node.exp!.loc.start.offset,
      shouldWrapParams ? "(" : ""
    );
    // add => { after the expression
    if (shouldWrapParams) {
      s.prependLeft(fromIndex, ")=>{");
    } else {
      s.prependLeft(fromIndex + 1, "=>{");
    }

    const context = item.context as ParseTemplateContext;
    if (ctx.doNarrow && context.conditions.length > 0) {
      ctx.doNarrow(
        {
          index: item.node.exp!.loc.end.offset,
          inBlock: true,
          conditions: context.conditions,
          // type: "append",
          type: "prepend",
          direction: "right",
          condition: null,
        },
        s
      );
    }

    if (
      (item.element as ElementNode).props.every(
        (x) => !["if", "else", "else-if"].includes(x.name)
      )
    ) {
      BlockPlugin.addItem(
        item.element,
        item.parent,
        item.context as ParseTemplateContext
      );
    }

    s.prependLeft(item.element.loc.end.offset, "})");
  },
});
