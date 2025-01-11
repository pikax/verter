import { ElementTypes } from "@vue/compiler-core";
import { declareTemplatePlugin } from "../../template";
import { BindingPlugin } from "../binding";

export const DirectivePlugin = declareTemplatePlugin({
  name: "VerterDirective",
  transformDirective(directive, s, ctx) {
    const element = directive.element;
    const node = directive.node;

    switch (directive.name) {
      case "model": {
        const clonedS = s.clone();
        // because the binding is not yet transformed
        // we need to clone the source and transform the binding
        directive.exp?.forEach((x) => {
          BindingPlugin.transformBinding!(x, clonedS, ctx);
        });
        directive.arg?.forEach((x) => {
          BindingPlugin.transformBinding!(x, clonedS, ctx);
        });

        const fallbakName =
          element.tagType === ElementTypes.ELEMENT ? "value" : "modelValue";

        let bindingTo = "modelValue";

        let isDynamic = false;

        if (node.arg) {
          if (!node.arg.ast && node.arg.isStatic) {
            bindingTo = node.arg.content;
            s.overwrite(
              node.loc.start.offset,
              node.arg.loc.end.offset,
              bindingTo
            );
          } else {
            isDynamic = true;

            // remove v-model
            s.overwrite(node.loc.start.offset, node.arg.loc.start.offset, "");
            // replace = with :
            if (s.original[node.arg.loc.end.offset] === "=") {
              s.overwrite(
                node.arg.loc.end.offset,
                node.arg.loc.end.offset + 1,
                ":"
              );
            }
            s.prependLeft(node.loc.start.offset, "{...{");
          }
        } else {
          s.overwrite(
            node.loc.start.offset,
            node.loc.start.offset + "v-model".length,
            fallbakName
          );
        }

        if (node.exp) {
          // update delimiters
          if (isDynamic) {
            s.remove(node.exp.loc.start.offset - 1, node.exp.loc.start.offset);
          } else {
            s.overwrite(
              node.exp.loc.start.offset - 1,
              node.exp.loc.start.offset,
              "{"
            );
          }

          // this will be updated in the next iteration
          // s.overwrite(node.exp.loc.end.offset, node.exp.loc.end.offset + 1, "}");

          const exp = clonedS.slice(
            node.exp.loc.start.offset,
            node.exp.loc.end.offset
          );

          if (isDynamic) {
            bindingTo = clonedS
              .slice(node.arg!.loc.start.offset, node.arg!.loc.end.offset)
              .toString()
              .slice(1, -1);
          }

          const pre = isDynamic
            ? `,[\`onUpdate:\${${bindingTo}}\`]:`
            : `} onUpdate:${bindingTo}={`;

          const post = isDynamic ? "}}" : "}";

          s.overwrite(
            node.exp.loc.end.offset,
            node.exp.loc.end.offset + 1,
            `${pre}($event)=>(${exp.toString()}=$event)${post}`
          );
        } else {
          // shouldn't be here
        }
      }
      case "once":

      default: {
      }
    }
    // if (directive.name === "model") {
    // } else {
    // }
  },
});
