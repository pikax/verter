import { ParsedBlockTemplate, TemplateTypes } from "../../../../parser";
import { ScriptTypes } from "../../../../parser/script";
import { BlockPlugin } from "../../../template/plugins";
import { ProcessItemBinding, ProcessItemType } from "../../../types";
import { definePlugin } from "../../types";
import { generateTypeString } from "../utils";

export const InferFunctionPlugin = definePlugin({
  name: "VerterInferFunction",

  transformDeclaration(item, s, ctx) {
    if (
      item.node.type !== "FunctionDeclaration" ||
      !("params" in item) ||
      !("name" in item) ||
      !item.name
    ) {
      return;
    }
    const templateBinding = ctx.templateBindings.find(
      (x) => x.name === item.name
    );
    if (!templateBinding) {
      return;
    }

    // check if has types
    if (ctx.block.lang === "ts") {
      if (item.params && item.params.length > 0) {
        if (item.params.every((p) => p.type === "Identifier")) {
          // we can patch

          const template = ctx.blocks.find(
            (x) => x.type === "template"
          ) as ParsedBlockTemplate;

          const directive = template.result.items.filter(
            (x) =>
              (x.type === TemplateTypes.Directive ||
                x.type === TemplateTypes.Prop) &&
              x.name === "on" &&
              x.node.loc.start >= templateBinding.node.loc.start &&
              x.node.loc.end <= templateBinding.node.loc.end
          );

          console.log("directive", directive);
        }
      }
    }

    if (item.name === "foo") {
      return;
    }
  },
});
