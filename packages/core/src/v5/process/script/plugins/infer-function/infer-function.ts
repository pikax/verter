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

          const directive = template.result.items.find(
            (x) =>
              (x.type === TemplateTypes.Directive ||
                x.type === TemplateTypes.Prop) &&
              x.name === "on" &&
              x.node &&
              x.node.loc.start >= templateBinding.node.loc.start &&
              x.node.loc.end <= templateBinding.node.loc.end
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

          let type = "";
          let property: string | undefined = "";

          if (element.tagType === ElementTypes.ELEMENT) {
            type = `HTML${capitalize(element.tag)}Element`;
          }

          if ("event" in directive && directive.event && directive.arg) {
            property = directive.arg[0].name;
            if (!property) {
              throw new Error("Unable to infer event name");
            }
          }

          type A = HTMLElementEventMap["click"];

          if (type && property) {
            const start = item.params[0].start;
            const end = item.params[item.params.length - 1].end;
            
            s.appendRight(start, "...[");
            if (element.tagType === ElementTypes.ELEMENT) {
              // todo handle custom elements]
              // s.appendLeft(end + 1, `]: [HTMLElementEventMap["${property}"]]`);
              // s.appendRight(end + 1, `]: [HTMLElementEventMap["${property}"]]`);
              // s.prependLeft(end + 1, `]: [HTMLElementEventMap["${property}"]]`);
              // s.prependRight(end + 1, `]: [HTMLElementEventMap["${property}"]]`);
              s.overwrite(end, end + 1, `]: [HTMLElementEventMap["${property}"]]${s.original[end]}`);
            } else {
              // s.appendLeft(end, `]: ${type}EventMap["${property}"]`)
            }
          }

          console.log("directive", directive, type);
        }
      }
    }

    if (item.name === "foo") {
      return;
    }
  },
});
