import { ElementTypes } from "@vue/compiler-core";
import {
  ParsedBlockTemplate,
  TemplateDirective,
  TemplateProp,
  TemplatePropDirective,
  TemplateTypes,
} from "../../../../parser";
import { ScriptTypes } from "../../../../parser/script";
import { BlockPlugin } from "../../../template/plugins";
import { ProcessItemBinding, ProcessItemType } from "../../../types";
import { definePlugin } from "../../types";
import { generateTypeString } from "../utils";
import { capitalize } from "vue";

/**
 * TODO rename this to be infer vars and others, not only functions
 */
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

          // TODO support multiple usages
          const directive = template.result.items.find(
            (x) =>
              (x.type === TemplateTypes.Directive ||
                x.type === TemplateTypes.Prop) &&
              x.name === "on" &&
              x.node &&
              x.node.loc.start.offset <=
                templateBinding.node.loc.start.offset &&
              x.node.loc.end.offset >= templateBinding.node.loc.end.offset
          ) as TemplateDirective | TemplateProp | undefined;
          if (!directive) {
            // directive not found
            return;
          }

          const element =
            directive.type === TemplateTypes.Prop
              ? directive.element
              : null; /*TODO handle directives on elements*/

          if (!element) {
            return;
          }

          let type = `ReturnType<typeof ${ctx.prefix("Comp")}${
            element.loc.start.offset
          }${ctx.generic ? `<${ctx.generic.declaration}>` : ""}>`;
          let property: string | undefined = "";

          if ("event" in directive && directive.event && directive.arg) {
            property =
              element.tagType === ElementTypes.COMPONENT
                ? "on" + capitalize(directive.arg[0].name!)
                : directive.arg[0].name;
            if (!property) {
              throw new Error("Unable to infer event name");
            }
          }
          if (element.tagType === ElementTypes.COMPONENT) {
            type = `Required<${type}['$props']>`;
          }

          if (type && property) {
            const start = item.params[0].start;
            const end = item.params[item.params.length - 1].end;

            s.appendRight(start, "...[");

            if (element.tagType === ElementTypes.ELEMENT) {
              s.overwrite(
                end,
                end + 1,
                `]: [HTMLElementEventMap["${property}"]]${s.original[end]}`
              );
            } else {
              const text = `]: Parameters<${type}["${property}"]>`;
              s.overwrite(
                end,
                end + 1,
                text + s.original.toString().slice(end, end + 1)
              );
            }
          }
        }
      }
    }
  },
});
