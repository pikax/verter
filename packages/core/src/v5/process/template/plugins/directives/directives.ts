import {
  DirectiveNode,
  ElementNode,
  ElementTypes,
  Namespaces,
  NodeTypes,
  SimpleExpressionNode,
} from "@vue/compiler-core";
import { ParseTemplateContext, TemplateDirective } from "../../../../parser";
import { declareTemplatePlugin, TemplateContext } from "../../template";
import { ProcessItemType } from "../../../types";
import { createHelperImport } from "../../../utils";
import { capitalize, isBuiltInDirective } from "@vue/shared";
import { AvailableExports } from "@verter/types/string";
import { BindingPlugin } from "../binding";

function pushWarning(
  node: DirectiveNode,
  ctx: import("../../template").TemplateContext,
  message:
    | "UNSUPPORTED_BUILTIN_DIRECTIVE_MODIFIER"
    | "UNSUPPORTED_BUILTIN_DIRECTIVE_ARGUMENT"
    | "UNSUPPORTED_BUILTIN_DIRECTIVE_VALUE"
) {
  ctx.items.push({
    type: ProcessItemType.Warning,
    message,
    start: node.loc.start.offset,
    end: node.loc.end.offset,
    node,
  });
}

function validateBuiltInDirective(
  name: string,
  node: DirectiveNode,
  ctx: TemplateContext
) {
  if (!isBuiltInDirective(name)) {
    return;
  }

  const allowArg = BUILTIN_ALLOW_ARG.has(name);
  const allowValue = BUILTIN_ALLOW_VALUE.has(name);
  const allowModifiers = BUILTIN_ALLOW_MODIFIERS.has(name);

  const hasModifiers = node.modifiers && node.modifiers.length > 0;

  if (!allowModifiers && hasModifiers) {
    pushWarning(node, ctx, "UNSUPPORTED_BUILTIN_DIRECTIVE_MODIFIER");
  }

  if (!allowArg && node.arg) {
    pushWarning(node, ctx, "UNSUPPORTED_BUILTIN_DIRECTIVE_ARGUMENT");
  }

  if (!allowValue && node.exp) {
    pushWarning(node, ctx, "UNSUPPORTED_BUILTIN_DIRECTIVE_VALUE");
  }
}

const BUILTIN_ALLOW_ARG = new Set(["slot", "model", "bind", "on"]);
const BUILTIN_ALLOW_VALUE = new Set([
  "text",
  "html",
  "show",
  "if",
  "else-if",
  "for",
  "memo",
  "model",
  "bind",
  "on",
  "slot",
]);
const BUILTIN_ALLOW_MODIFIERS = new Set(["model", "bind", "on"]);

export const DirectiveRunnerPlugin = declareTemplatePlugin({
  name: "VerterDirectiveRunner",

  directivesByElement: new Map<ElementNode, TemplateDirective[]>(),

  pre() {
    this.directivesByElement.clear();
  },

  transformDirective(item, s, ctx) {
    // handle type checking for directives
    validateBuiltInDirective(item.name, item.node, ctx);
    const list = this.directivesByElement.get(item.element) ?? [];
    list.push(item);
    this.directivesByElement.set(item.element, list);
    // /handle type checking for directives

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
              ? node.modifiers.find((x) => x.content === "lazy")
                ? "onChange"
                : "onInput"
              : isDynamic
              ? "onUpdate"
              : `onUpdate:${bindingTo}`;

          const valueAccessor =
            element.tagType === ElementTypes.ELEMENT
              ? `${
                  node.modifiers.find((x) => x.content === "number") ? "+" : ""
                }$event.target.value`
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

        break;
      }
      case "is": {
        if (item.element.tag === "component") {
          return;
        }
      }
    }
  },

  transformProp(prop, _s, ctx) {
    const node = prop.node;
    if (!node || node.type !== NodeTypes.DIRECTIVE) return;
    validateBuiltInDirective(prop.name, node, ctx);
    if (isBuiltInDirective(node.name)) {
      const list = this.directivesByElement.get(prop.element) ?? [];
      list.push(prop as any);
      this.directivesByElement.set(prop.element, list);
    }
  },

  post(s, ctx) {
    if (this.directivesByElement.size === 0) return;

    const slotInstance = ctx.retrieveAccessor("slotInstance");
    const directiveAccessor = ctx.retrieveAccessor("directiveAccessor");
    const directiveElement = ctx.prefix("directiveElement");
    const runCustomDirective = ctx.prefix("runCustomDirective");
    const extractLeafElement = ctx.prefix("ExtractLeafElement");

    const helperImports = new Set<AvailableExports>();

    const hasAnyCustomDirective = Array.from(
      this.directivesByElement.values()
    ).some((dirs) => dirs.some((x) => !isBuiltInDirective(x.name)));

    if (hasAnyCustomDirective) {
      helperImports.add("ExtractLeafElement");
      helperImports.add("runCustomDirective");
    }

    if (helperImports.size > 0) {
      ctx.items.push(createHelperImport([...helperImports], ctx.prefix));
    }

    for (const [element, directives] of this.directivesByElement) {
      const insertPos = element.loc.start.offset + element.tag.length + 1;

      const hasCustomDirective =
        directives.filter((x) => !isBuiltInDirective(x.name)).length > 0;
      const hasBuiltInWithModifiers = directives.some(
        (x) => isBuiltInDirective(x.name) && x.node.modifiers?.length
      );

      // Skip emitting when only built-ins without modifiers are present
      if (!hasCustomDirective && !hasBuiltInWithModifiers) {
        continue;
      }

      const directiveElementStr = `const ${directiveElement}={} as ${extractLeafElement}<typeof ${slotInstance}>;`;

      s.appendLeft(
        insertPos,
        ` v-directive={(${slotInstance})=>{${
          hasCustomDirective ? directiveElementStr : ""
        }`
      );

      let lastPos = insertPos;
      let prepend = "";

      // for (const dir of directives) {
      for (let i = 0; i < directives.length; i++) {
        const dir = directives[i];
        const node = dir.node;
        const arg = node.arg;
        const exp = node.exp;

        if (i === 0) {
          const context = dir.context;
          if (ctx.doNarrow && context.conditions.length > 0) {
            ctx.doNarrow(
              {
                index: insertPos,
                inBlock: false,
                conditions: context.conditions,
                type: "append",
              },
              s
            );
          }
        }

        const startName = node.loc.start.offset;
        const endName = Math.min(
          startName + 2 /* "v-".length */ + dir.name.length,
          node.loc.end.offset
        );
        const moves = [] as Array<() => void>;

        const isBuiltIn = isBuiltInDirective(dir.name);
        if (isBuiltIn) {
          if (node.modifiers && node.modifiers.length === 0) {
            continue;
          }
          const name = `v${capitalize(dir.name)}Modifiers`;
          ctx.items.push(
            createHelperImport([name as AvailableExports], ctx.prefix)
          );
          prepend += "(";
          processModifiers(false);

          try {
            const arg =
              node.name === "on"
                ? node.arg
                  ? // TODO this is incorrect, it's not correctly prepending the context
                    "on" +
                    s.slice(node.arg.loc.start.offset, node.arg.loc.end.offset)
                  : ""
                : node.name === "bind"
                ? node.arg
                  ? s.slice(node.arg.loc.start.offset, node.arg.loc.end.offset)
                  : ""
                : node.name === "model"
                ? node.arg
                  ? node.arg
                  : dir.element.tagType === ElementTypes.ELEMENT
                  ? "value"
                  : "modelValue"
                : "";
            const isStaticArg =
              (node.arg &&
                node.arg.type === NodeTypes.SIMPLE_EXPRESSION &&
                node.arg.isStatic) ??
              true;
            prepend = `satisfies ${ctx.prefix(name)}<typeof ${slotInstance},${
              isStaticArg ? "'" : ""
            }${arg}${isStaticArg ? "'" : ""}>)`;

            moves.forEach((move) => move());
          } catch (e) {
            console.log("NOTE IT SHOULD NOT FAIL BUT IT DOES!!!", e);
            debugger;
            const arg = node.arg
              ? s.slice(node.arg.loc.start.offset, node.arg.loc.end.offset)
              : "";
            console.log("asdasd", arg);
          }
          continue;
        }
        lastPos = node.loc.end.offset;

        // remove - & capitalise first letter
        if (dir.name) {
          s.overwrite(
            node.loc.start.offset + 1,
            node.loc.start.offset + 3,
            dir.name.charAt(0).toUpperCase()
          );

          // if contains hyphen, remove and capitalise next letter
          dir.name.replace(/-([a-zA-Z])/g, (_, char, index) => {
            s.overwrite(
              node.loc.start.offset + 2 + index,
              node.loc.start.offset + 4 + index,
              char.toUpperCase()
            );
            return "";
          });
        } else {
          s.overwrite(node.loc.start.offset + 1, node.loc.start.offset + 2, "");
        }

        s.appendRight(
          startName,
          `${prepend}${runCustomDirective}(${directiveElement},${directiveAccessor}["`
        );
        s.appendLeft(endName, `"])(${directiveElement}`);

        prepend = "";

        moves.push(() => s.move(startName, endName, insertPos));

        if (exp) {
          s.remove(exp.loc.start.offset - 2, exp.loc.start.offset); // remove ="
          // s.remove(exp.loc.end.offset, exp.loc.end.offset + 1); // remove ending "
          s.overwrite(exp.loc.end.offset, exp.loc.end.offset + 1, "");
          s.prependRight(exp.loc.start.offset, ",");
          moves.push(() =>
            s.move(exp.loc.start.offset, exp.loc.end.offset, insertPos)
          );
        } else {
          prepend += ",true";
        }

        if (arg) {
          s.remove(arg.loc.start.offset - 1, arg.loc.start.offset); // remove ":"
          s.appendRight(arg.loc.start.offset, `${prepend},`);
          prepend = "";
          if (arg.type === NodeTypes.SIMPLE_EXPRESSION && arg.isStatic) {
            s.appendRight(arg.loc.start.offset, `"`);
            s.appendLeft(arg.loc.end.offset, `"`);
          }
          moves.push(() =>
            s.move(arg.loc.start.offset, arg.loc.end.offset, insertPos)
          );
        } else {
          prepend += `,undefined`;
        }

        function processModifiers(appendComma = true) {
          if (node.modifiers && node.modifiers.length > 0) {
            for (let i = 0; i < node.modifiers.length; i++) {
              const modifier = node.modifiers[i];
              const isLast = i === node.modifiers.length - 1;

              const isDotShorthand =
                node.rawName?.startsWith(".") && modifier.loc.source === "";
              const posStart = isDotShorthand
                ? node.loc.start.offset
                : modifier.loc.start.offset;
              const posEnd = isDotShorthand
                ? posStart + 1
                : modifier.loc.end.offset;

              // source is empty when using dot shorthand
              if (isDotShorthand) {
                s.appendRight(
                  insertPos,
                  `${prepend}{"prop":true${
                    isLast ? `}${appendComma ? ");" : ""}` : ","
                  }`
                );
              } else {
                // remove dot "."
                s.overwrite(posStart - 1, posStart, '"');

                if (i === 0) {
                  s.appendRight(
                    posStart - 1,
                    `${prepend}${appendComma ? "," : ""}{`
                  );
                }
                s.appendLeft(
                  posEnd,
                  `":true${isLast ? `}${appendComma ? ");" : ""}` : ","}`
                );
              }
              prepend = "";

              if (!isDotShorthand) {
                moves.push(() => s.move(posStart - 1, posEnd, insertPos));
              }
            }
          } else {
            prepend += `,{});`;
          }
        }

        processModifiers();

        for (let i = 0; i < moves.length; i++) {
          const move = moves[i];

          move();
        }
      }

      prepend += "}}";
      if (prepend) {
        s.appendRight(lastPos, prepend);
      }
    }
  },
});
