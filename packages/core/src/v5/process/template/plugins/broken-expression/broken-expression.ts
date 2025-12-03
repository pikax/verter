import { ScriptTypes } from "../../../../parser";
import { VerterASTNode } from "../../../../parser/ast";
import { ParseTemplateContext } from "../../../../parser/template";
import {
  TemplateElement,
  TemplateTypes,
} from "../../../../parser/template/types";
import { declareTemplatePlugin, TemplateContext } from "../../template";

export const BrokenExpressionPlugin = declareTemplatePlugin({
  name: "VerterBrokenExpression",

  transformBrokenExpression(item, s, context) {
    const { node, directive } = item;

    const ctx = context.retrieveAccessor("ctx");

    const allBindings = context.blocks
      .map(
        (x) =>
          x.type === "script" &&
          x.result?.items
            .filter((x) => x.type === ScriptTypes.Declaration)
            .map((x) => x.name)
      )
      .flat()
      .filter(Boolean) as string[];
    const allBindingsStr =
      allBindings.length > 0 ? `\nlet {${allBindings.join(",")}}=${ctx};` : "";

    s.prependLeft(
      node.loc.start.offset,
      `/*__VERTER_BROKEN_EXPRESSION__*/${allBindingsStr}`
    );
  },
});
