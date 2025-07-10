import {
  ElementTypes,
  DirectiveNode,
  SimpleExpressionNode,
} from "@vue/compiler-core";
import { MagicString } from "@vue/compiler-sfc";
import { declareTemplatePlugin, TemplateContext } from "../../template";
import { BindingPlugin } from "../binding";
import { ParseTemplateContext } from "../../../../parser/template";
import {
  capitalize,
  Directive,
  GlobalDirectives,
  ObjectDirective,
  vModelCheckbox,
  vModelDynamic,
  vModelRadio,
  vModelSelect,
  vModelText,
} from "vue";
import { ProcessItemType } from "../../../types";

/**
 * Vmodel 
 * 
 * type DirectiveRenderBetter<T extends HTMLElement> =

    | (<Value = any, Modifiers extends string = string, Arg extends string = string>(directive: Directive<T & { _assigning?: boolean }, Value, Modifiers, Arg>) => {
        value: Value,
        arg: Arg,
        modifiers: Modifiers[]
    });


// TODO maybe add the instance and also support binding instance
declare function ___VERTER___instanceToDirectiveFn<T extends HTMLElement>(node: T): DirectiveRenderBetter<T>

 * 
accessor= {
    vModelText: {} as ModelDirective<
        any,
        'trim' | 'number' | 'lazy'
    >,
}
 */

export const DirectivePlugin = declareTemplatePlugin({
  name: "VerterDirective",

  // create type safe modifiers
  handleDirectiveModifiers(
    node: DirectiveNode,
    context: ParseTemplateContext,
    s: MagicString,
    ctx: TemplateContext
  ) {
    if (node.modifiers.length === 0) {
      return;
    }
    const directiveAccessor = ctx.retrieveAccessor("directiveAccessor");
    const instancePropertySymbol = ctx.retrieveAccessor(
      "instancePropertySymbol"
    );
    const slotInstance = ctx.retrieveAccessor("slotInstance");
    const instanceToDirectiveFn = ctx.retrieveAccessor("instanceToDirectiveFn");
    const instanceToDirectiveVar = ctx.retrieveAccessor(
      "instanceToDirectiveVar"
    );
    const directiveName = ctx.retrieveAccessor("directiveName");

    const index = node.loc.start.offset;

    const declaration =
      `const ${instanceToDirectiveVar}=${instanceToDirectiveFn}(${slotInstance});` +
      `const ${directiveName}=${instanceToDirectiveVar}(${directiveAccessor}.${
        // NOTE v-model text is the default, maybe we can add more here later
        node.name === "model" ? "vModelText" : `v${capitalize(node.name)}`
      });`;

    s.prependLeft(index, declaration);
    if (ctx.doNarrow && context.conditions.length > 0) {
      ctx.doNarrow(
        {
          index: index,
          conditions: context.conditions,
          inBlock: true,
          type: "prepend",
          direction: "left",
        },
        s
      );
    }
    s.prependLeft(
      index,
      `{...{[${instancePropertySymbol}]:(${slotInstance})=>{`
    );

    if (node.modifiers.length > 0) {
      const start = node.modifiers[0].loc.start.offset - 1;
      const end = node.modifiers[node.modifiers.length - 1].loc.end.offset;
      s.move(start, end, index);

      s.overwrite(start, start + 1, `${directiveName}.modifiers=[`);

      // s.overwrite(start - 1, start, `${directiveName}.modifiers=[`);
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

    s.prependRight(index, "}}} ");
  },

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

        let bindingTo =
          element.tagType === ElementTypes.ELEMENT ? "input" : "modelValue";

        let isDynamic = false;

        if (node.arg) {
          const arg = node.arg as SimpleExpressionNode;

          if (!node.arg.ast && arg.isStatic) {
            bindingTo = arg.content;
            s.overwrite(node.loc.start.offset, arg.loc.end.offset, bindingTo);
          } else {
            isDynamic = true;

            // remove v-model
            s.overwrite(node.loc.start.offset, arg.loc.start.offset, "");
            // replace = with :
            if (s.original[arg.loc.end.offset] === "=") {
              s.overwrite(arg.loc.end.offset, arg.loc.end.offset + 1, ":");
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

          const eventName =
            element.tagType === ElementTypes.ELEMENT
              ? "onInput"
              : isDynamic
              ? "onUpdate"
              : `onUpdate:${bindingTo}`;

          const valueAccessor =
            element.tagType === ElementTypes.ELEMENT
              ? "$event.target.value"
              : "$event";
          const pre = isDynamic
            ? `,[\`${eventName}:\${${bindingTo}}\`]:`
            : `} ${eventName}={`;

          const post = isDynamic ? "}}" : "}";

          s.overwrite(
            node.exp.loc.end.offset,
            node.exp.loc.end.offset + 1,
            `${pre}($event)=>(${exp.toString()}=${valueAccessor})${post}`
          );
        } else {
          // shouldn't be here
          // todo add a warning

          ctx.items.push({
            type: ProcessItemType.Warning,
            message: "NO_EXPRESSION_VMODEL",
            start: node.loc.start.offset,
            end: node.loc.end.offset,
            node,
          });
        }

        this.handleDirectiveModifiers(
          node,
          item.context as ParseTemplateContext,
          s,
          ctx
        );
        break;
      }
      case "is": {
        if (item.element.tag === "component") {
          return;
        }
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
          const arg = node.arg as SimpleExpressionNode;
          // replace ':' with '='
          s.overwrite(arg.loc.start.offset - 1, arg.loc.start.offset, "=");

          s.prependRight(arg.loc.start.offset - 1, `${directiveName}.arg`);
          s.prependLeft(arg.loc.end.offset, ";");

          if (arg.isStatic) {
            // add quotes
            s.prependLeft(arg.loc.start.offset, '"');
            s.prependLeft(arg.loc.end.offset, '"');
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
