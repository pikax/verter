import { ElementTypes } from "@vue/compiler-core";
import {
  ParsedBlockTemplate,
  TemplateProp,
  TemplateTypes,
} from "../../../../parser";
import { definePlugin } from "../../types";
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
        if (
          item.params.every((p) => p.type === "Identifier" && !p.typeAnnotation)
        ) {
          // we can patch

          const template = ctx.blocks.find(
            (x) => x.type === "template"
          ) as ParsedBlockTemplate;

          // Limitation: This plugin does not currently support functions used in multiple event handlers.
          // When a function is used in multiple @event bindings, only the first binding is considered.
          // Only search for TemplateProp since @event handlers (v-on) are always parsed as props.
          // TemplateDirective would be for other directive types which are not event handlers.
          const directive = template.result.items.find(
            (x) =>
              x.type === TemplateTypes.Prop &&
              x.name === "on" &&
              x.node &&
              x.node.loc.start.offset <=
                templateBinding.node.loc.start.offset &&
              x.node.loc.end.offset >= templateBinding.node.loc.end.offset
          ) as TemplateProp | undefined;
          if (!directive) {
            // directive not found
            return;
          }

          const element = directive.element;
          if (!element) {
            return;
          }

          let type = `ReturnType<typeof ${ctx.prefix("Comp")}${
            element.loc.start.offset
          }${ctx.generic ? `<${ctx.generic.declaration}>` : ""}>`;
          let property: string | undefined;

          if (
            "event" in directive &&
            directive.event &&
            directive.arg &&
            directive.arg.length > 0
          ) {
            const argName = directive.arg[0].name;
            if (!argName) {
              throw new Error("Unable to infer event name");
            }
            property =
              element.tagType === ElementTypes.COMPONENT
                ? "on" + capitalize(argName)
                : argName;
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
              s.overwrite(end, end + 1, text + s.original[end]);
            }
          }
        }
      }
    }
  },
});
