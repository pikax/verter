import { ElementTypes } from "@vue/compiler-core";
import { declareTemplatePlugin, TemplateContext } from "../../template";
import { BindingPlugin } from "../binding";
import { ParseTemplateContext } from "../../../../parser/template";

export const DirectivePlugin = declareTemplatePlugin({
  name: "VerterDirective",
  transformDirective(item, s, ctx) {
    const element = item.element;
    const node = item.node;

    switch (item.name) {
      case "model": {
        const clonedS = s.clone();
        // because the binding is not yet transformed
        // we need to clone the source and transform the binding
        item.exp?.forEach((x) => {
          BindingPlugin.transformBinding!(x, clonedS, ctx);
        });
        item.arg?.forEach((x) => {
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

        break;
      }
      default: {
        const directiveAccessor = ctx.retrieveAccessor("directiveAccessor");
        const instancePropertySymbol = ctx.retrieveAccessor(
          "instancePropertySymbol"
        );
        const slotInstance = ctx.retrieveAccessor("slotInstance");
        const instanceToDirectiveFn = ctx.retrieveAccessor(
          "instanceToDirectiveFn"
        );
        const instanceToDirectiveVar = ctx.retrieveAccessor(
          "instanceToDirectiveVar"
        );
        const directiveName = ctx.retrieveAccessor("directiveName");

        const context = item.context as ParseTemplateContext;
        if (ctx.doNarrow && context.conditions.length > 0) {
          ctx.doNarrow(
            {
              index: node.loc.start.offset,
              conditions: context.conditions,
              inBlock: true,
              type: "prepend",
              direction: "left",
            },
            s
          );
        }

        const declaration =
          `const ${instanceToDirectiveVar}=${instanceToDirectiveFn}(${slotInstance});` +
          `const ${directiveName}=${instanceToDirectiveVar}(`;

        s.prependLeft(
          node.loc.start.offset,
          `{...{[${instancePropertySymbol}]:(${slotInstance})=>{`
        );

        s.prependRight(node.loc.start.offset, `${directiveAccessor}.`);
        s.prependRight(node.loc.start.offset, declaration);

        // replace ={\w} with {\w} uppercase letter
        s.overwrite(
          node.loc.start.offset + 1,
          node.loc.start.offset + 3,
          item.name[0].toUpperCase()
        );

        s.prependLeft(node.loc.start.offset + 2 + item.name.length, ");");

        if (node.arg) {
          // replace ':' with '='
          s.overwrite(
            node.arg.loc.start.offset - 1,
            node.arg.loc.start.offset,
            "="
          );

          s.prependRight(node.arg.loc.start.offset - 1, `${directiveName}.arg`);
          s.prependLeft(node.arg.loc.end.offset, ";");

          if (node.arg.isStatic) {
            // add quotes
            s.prependLeft(node.arg.loc.start.offset, '"');
            s.prependLeft(node.arg.loc.end.offset, '"');
          }
        }

        if (node.modifiers.length > 0) {
          const start = node.modifiers[0].loc.start.offset;
          const end = node.modifiers[node.modifiers.length - 1].loc.end.offset;

          s.overwrite(start - 1, start, `${directiveName}.modifiers=[`);
          s.prependLeft(end, "];");

          for (let i = 0; i < node.modifiers.length; i++) {
            const modifier = node.modifiers[i];
            if (i > 0) {
              s.overwrite(
                modifier.loc.start.offset - 1,
                modifier.loc.start.offset,
                ","
              );
            }
            s.prependRight(modifier.loc.start.offset, '"');
            s.prependLeft(modifier.loc.end.offset, '"');
          }
        }

        if (node.exp) {
          // replace ="
          s.overwrite(
            node.exp.loc.start.offset - 2,
            node.exp.loc.start.offset,
            `${directiveName}.value=`
          );

          s.overwrite(
            node.exp.loc.end.offset,
            node.exp.loc.end.offset + 1,
            ";"
          );
        }

        s.prependRight(node.loc.end.offset, "}}}");
      }
    }
  },
});
