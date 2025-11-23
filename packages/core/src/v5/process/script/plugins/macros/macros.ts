import { definePlugin, ScriptContext } from "../../types";
import type { CallExpression } from "../../../../parser/ast/types";
import {
  ProcessContext,
  ProcessItemImport,
  ProcessItemType,
} from "../../../types";
import { generateTypeDeclaration, generateTypeString } from "../utils";
import type { AvailableExports } from "@verter/types/string";
import { createHelperImport } from "../../../utils";

const MacrosNames = [
  "defineProps",
  "defineEmits",
  "defineExpose",
  "defineOptions",
  "defineModel",
  "defineSlots",
  "withDefaults",

  // useSlots()/useAttrs()
] as const;
type MacroNames = (typeof MacrosNames)[number];

const Macros = new Set<string>(MacrosNames);

export const MacrosPlugin = definePlugin({
  name: "VerterMacro",
  hasWithDefaults: false,
  pre(s, ctx) {
    this.hasWithDefaults = false;
  },

  post(s, ctx) {
    // // not needed anymore
    // const isTS = ctx.block.lang.startsWith("ts");
    // const macroBindinds = ctx.items.filter(
    //   (x) => x.type === ProcessItemType.MacroBinding
    // );
    // for (const macro of Macros) {
    //   if (macro === "withDefaults") continue;
    //   if (macro === "defineOptions") {
    //     continue;
    //   }
    //   const name = ctx.prefix(macro);
    //   const itemMacro =
    //     macro === "defineModel"
    //       ? ctx.items.find((x) => x.type === ProcessItemType.DefineModel)
    //       : macroBindinds.find((x) => x.macro === macro);
    //   const TemplateBinding = ctx.prefix("TemplateBinding");
    //   if (itemMacro) {
    //     const str = generateTypeString(
    //       name,
    //       {
    //         from: TemplateBinding,
    //         key: name,
    //         isType: true,
    //       },
    //       ctx
    //     );
    //     s.append(str);
    //   } else {
    //     const str = generateTypeDeclaration(
    //       name,
    //       "{}",
    //       ctx.generic?.source,
    //       isTS
    //     );
    //     s.append(str);
    //   }
    // }
  },

  transformDeclaration(item, s, ctx) {
    if (
      item.parent.type === "VariableDeclarator" &&
      item.parent.init?.type === "CallExpression"
    ) {
      if (item.parent.init.callee.type === "Identifier") {
        const macroName = item.parent.init.callee.name;
        if (!Macros.has(macroName) || macroName === "defineOptions") {
          return;
        }
        addMacroDependencies(macroName, ctx);

        let varName =
          item.parent.id.type === "Identifier" ? item.parent.id.name : "";
        // let originalName: string | undefined = undefined;
        // if (macroName === "defineModel") {
        //   // const accessor = ctx.prefix("models");
        //   const modelName = getModelName(item.parent.init);

        //   return
        //   // varName = `${accessor}_${modelName}`;
        //   originalName = modelName;
        // }

        if (ctx.isSetup) {
          // s.prependLeft(item.declarator.end, `let ${varName} ;`);
          // s.prependLeft(item.parent.id.end, `=${varName}`);

          if (macroName === "defineModel") {
            ctx.items.push({
              type: ProcessItemType.DefineModel,
              varName,
              name: getModelName(item.parent.init),
              node: item.parent,
            });
          } else if (macroName == "withDefaults") {
            const defineProps = item.parent.init.arguments[0];
            const pName = ctx.prefix("Props");
            if (
              defineProps.type === "CallExpression" &&
              defineProps.callee.type === "Identifier" &&
              defineProps.callee.name === "defineProps"
            ) {
              addMacroDependencies("defineProps", ctx);
              ctx.items.push({
                type: ProcessItemType.MacroBinding,
                name: pName,
                isType: defineProps.typeArguments !== undefined,
                macro: "defineProps",
                node: defineProps,
              });
            }

            let splitFromOffset = -1;
            let splitToOffset = -1;

            const propsBoxInfo = boxInfo("defineProps", ctx);

            if (defineProps.type === "CallExpression") {
              if (defineProps.arguments.length > 0) {
                // add a warning if there's arguments, since we are using withDefaults
                ctx.items.push({
                  type: ProcessItemType.Warning,
                  message: "INVALID_WITH_DEFAULTS_DEFINE_PROPS_WITH_OBJECT_ARG",
                  node: defineProps,
                  start: defineProps.start,
                  end: defineProps.end,
                });
              }

              const isType = defineProps.typeArguments !== undefined;

              splitFromOffset = defineProps.typeArguments
                ? defineProps.typeArguments.start
                : defineProps.arguments[0].start;
              splitToOffset = defineProps.typeArguments
                ? defineProps.typeArguments.end
                : defineProps.arguments[defineProps.arguments.length - 1].end;

              if (isType) {
                const paramsStart = defineProps.typeArguments?.params[0].start!;
                const paramEnd =
                  defineProps.typeArguments?.params[
                    defineProps.typeArguments.params.length - 1
                  ].end!;
                // s.prependRight(
                //   paramsStart,
                //   `type ${propsBoxInfo.boxedName}_TYPE=`
                // );
                // s.prependLeft(paramEnd, ";");

                // s.move(paramsStart, paramEnd, item.declarator.start! - 1);

                const propsType = ctx.prefix("defineProps_Type");

                s.appendLeft(paramsStart, propsType);
                // create type and prettify it with "{}&" otherwise on hover it will
                // show `defineProps_Type` only
                s.appendRight(paramsStart, `;type ${propsType}={}&`);

                s.move(paramsStart, paramEnd, item.declarator.end!);

                // s.move(
                //   defineProps.typeArguments!.start!,
                //   defineProps.typeArguments!.end!,
                //   item.declarator.start!
                // );
              } else {
                s.appendLeft(
                  item.declarator.start!,
                  `let ${propsBoxInfo.boxedName}`
                );
                if (splitFromOffset !== -1 && splitToOffset !== -1) {
                  s.appendLeft(
                    splitFromOffset,
                    `${propsBoxInfo.boxedName}=${propsBoxInfo.boxName}(`
                  );
                  s.appendRight(splitToOffset, `)`);
                }
              }

              ctx.items.push(
                createHelperImport([propsBoxInfo.name], ctx.prefix)
              );
            }

            const withDefaultOps = boxMacro(
              item.parent.init,
              item.declarator.start!,
              s,
              ctx,
              // { from: 63, to: 77 }
              null
              /*   splitFromOffset !== -1 && splitToOffset !== -1
                ? { from: splitFromOffset, to: splitToOffset }
                : null*/
            );

            // withDefaultOps!();
            withDefaultOps!.start();
            withDefaultOps!.end();

            withDefaultOps!.move();

            // // temp move type
            // s.move(63, 77, item.declarator.start! -1);
            this.hasWithDefaults = true;
          } else {
            if (macroName === "defineProps" && this.hasWithDefaults) {
              return;
            }
            ctx.items.push({
              type: ProcessItemType.MacroBinding,
              name: varName!,
              macro: macroName,
              node: item.parent,
              isType: item.parent.init.typeArguments !== undefined,
            });
            boxMacro(item.parent.init, item.declarator.start!, s, ctx);
          }
        } else {
          ctx.items.push({
            type: ProcessItemType.Warning,
            message: "MACRO_NOT_IN_SETUP",
            node: item.node,
            // @ts-expect-error TODO improve this, this shouldn't be necessary
            start: item.node.start,
            // @ts-expect-error TODO improve this, this shouldn't be necessary
            end: item.node.end,
          });
        }
      }
    }
  },
  transformFunctionCall(item, s, ctx) {
    if (
      !Macros.has(item.name) ||
      ctx.items.some(
        (x) =>
          (x.type === ProcessItemType.MacroBinding ||
            x.type === ProcessItemType.DefineModel) &&
          // check if is inside another macro
          // @ts-expect-error TODO improve this, this shouldn't be necessary
          x.node.start <= item.node.start &&
          // @ts-expect-error TODO improve this, this shouldn't be necessary
          x.node.end >= item.node.end
      )
    ) {
      return;
    }

    const macroName = item.name;

    addMacroDependencies(macroName, ctx);

    let varName = "";

    if (item.name === "defineModel") {
      const modelName = getModelName(item.node);
      const accessor = ctx.prefix("models");
      varName = `${accessor}_${modelName}`;
    } else if (macroName === "withDefaults") {
      if (this.hasWithDefaults) {
        return;
      }
      // check if there's an defineProps inside

      if (ctx.isSetup) {
        const defineProps = item.node.arguments[0];
        const pName = ctx.prefix("Props");
        if (
          defineProps.type === "CallExpression" &&
          defineProps.callee.type === "Identifier" &&
          defineProps.callee.name === "defineProps"
        ) {
          addMacroDependencies("defineProps", ctx);
          ctx.items.push({
            type: ProcessItemType.MacroBinding,
            name: pName,
            macro: "defineProps",
            node: defineProps,
          });
        }
        // prepend props
        s.appendLeft(item.node.start, `const ${pName}=`);
        s.appendLeft(defineProps.end, ";");
        s.appendRight(defineProps.end, pName);

        s.move(defineProps.start, defineProps.end, item.node.start);
        this.hasWithDefaults = true;

        return;
      }
    } else if (macroName === "defineOptions") {
      return handleDefineOptions(item.node, ctx);
    } else if (macroName === "defineProps" && this.hasWithDefaults) {
      return;
    } else {
      varName = ctx.prefix(macroName.replace("define", ""));
    }

    if (ctx.isSetup) {
      s.prependLeft(item.node.start, `const ${varName}=`);

      if (macroName === "defineModel") {
        ctx.items.push({
          type: ProcessItemType.DefineModel,
          varName,
          name: getModelName(item.node),
          node: item.node,
        });
      } else {
        ctx.items.push({
          type: ProcessItemType.MacroBinding,
          name: varName!,
          macro: macroName,
          node: item.node,
        });
      }
    } else {
      ctx.items.push({
        type: ProcessItemType.Warning,
        message: "MACRO_NOT_IN_SETUP",
        node: item.node,
        start: item.node.start,
        end: item.node.end,
      });
    }
  },
});

function addMacroDependencies(macroName: string, ctx: ScriptContext) {
  if (!ctx.block.lang.startsWith("ts")) return;

  // @ts-expect-error fix this later
  createHelperImport([`${macroName as MacroNames}_Box`], ctx.prefix);

  ctx.items.push({
    type: ProcessItemType.Import,
    from: "vue",
    items: [
      {
        name: macroName,
      },
    ],
  });

  // const dependencies = MacroDependencies.get(macroName);
  // if (dependencies) {
  //   ctx.items.push({
  //     type: ProcessItemType.Import,
  //     asType: true,
  //     from: HelperLocation,
  //     items: dependencies.map((dep) => ({ name: ctx.prefix(dep) })),
  //   });
  // }
}

function handleDefineOptions(node: CallExpression, ctx: ProcessContext) {
  if (node.arguments.length > 0) {
    const [arg] = node.arguments;
    switch (arg.type) {
      case "Identifier":
      case "ObjectExpression": {
        ctx.items.push({
          type: ProcessItemType.Options,
          node: node,
          expression: arg,
        });
        break;
      }
      default: {
        ctx.items.push({
          type: ProcessItemType.Warning,
          message: "INVALID_DEFINE_OPTIONS",
          node: node,
          start: node.start,
          end: node.end,
        });
      }
    }
    if (node.arguments.length > 1) {
      ctx.items.push({
        type: ProcessItemType.Warning,
        message: "INVALID_DEFINE_OPTIONS",
        node: node,
        start: arg.end,
        end: node.end,
      });
    }
    return;
  } else {
    ctx.items.push({
      type: ProcessItemType.Warning,
      message: "INVALID_DEFINE_OPTIONS",
      node: node,
      start: node.start,
      end: node.end,
    });
  }
}

function getModelName(node: CallExpression) {
  const nameArg = node.arguments[0];
  const modelName =
    nameArg?.type === "Literal"
      ? nameArg.value?.toString() ?? "modelValue"
      : "modelValue";

  return modelName;
}

function boxInfo(macroName: string, ctx: ScriptContext) {
  const boxedName = ctx.prefix(`${macroName}_Boxed`);
  const name = `${macroName}_Box` as AvailableExports;
  const boxName = ctx.prefix(name);
  return { boxedName, boxName, name };
}

function boxMacro(
  caller: CallExpression,
  toOffset: number,
  s: any,
  ctx: ScriptContext,
  split: { from: number; to: number } | null = null
) {
  const name: AvailableExports | undefined =
    caller.callee.type === "Identifier"
      ? (caller.callee.name as AvailableExports)
      : undefined;
  if (!name) {
    console.warn("boxMacro: callee is not an identifier", caller.callee);
    return;
  }
  const { boxedName, boxName } = boxInfo(name, ctx);
  ctx.items.push(
    createHelperImport([`${name}_Box` as AvailableExports], ctx.prefix)
  );

  if (caller.typeArguments) {
    const args = caller.typeArguments;
  } else {
    const args = caller.arguments;
    const multiple = args.length > 1;
    const start = args[0].start;
    const end = args[args.length - 1].end;

    const doStart = (method = "appendLeft") => {
      s[method](toOffset, `;const ${boxedName}=${boxName}(`);

      s[method](
        start!,
        multiple
          ? [1, 2].map((_, i) => `${boxedName}[${i}]`).join(",")
          : boxedName
      );
    };
    const doEnd = (method = "appendRight") => {
      s.appendRight(toOffset, `);`);
      // s.prependLeft(end, `);`);
    };

    // s.prependLeft(toOffset, `;const ${boxedName}=${boxName}(`);
    // s.prependRight(toOffset, `);`);
    // s.prependLeft(
    //   start!,
    //   multiple
    //     ? [1, 2].map((_, i) => `${boxedName}[${i}]`).join(",")
    //     : boxedName
    // );

    const doMove = () => {
      if (split) {
        s.move(start!, split.from, toOffset);
        s.move(split.to, end!, toOffset);
      } else {
        s.move(start!, end!, toOffset);
      }
    };

    const move = () => {
      doStart();
      doEnd();
      doMove();
    };
    move.start = doStart;
    move.end = doEnd;
    move.move = doMove;

    return move;
  }

  // const isType = caller.typeArguments !== undefined;
  // const moveStart = isType
  //   ? caller.typeArguments!.start
  //   : caller.arguments[0].start;
  // const moveEnd = isType
  //   ? caller.typeArguments!.end
  //   : caller.arguments[caller.arguments.length - 1].end;

  // const multiple = isType
  //   ? caller.typeArguments!.params.length > 1
  //   : caller.arguments.length > 1;

  // s.appendLeft(
  //   toOffset,
  //   `;${isType ? "type" : "const"} ${boxedName}=${
  //     multiple ? "[" : ""
  //   }${boxName}(`
  // );
  // s.appendRight(toOffset, ");");
  // s.appendLeft(
  //   caller.start!,
  //   [1, 2].map((_, i) => `${boxedName}[${i}]`).join(",")
  // );
  // s.move(caller.start!, caller.end!, toOffset);
}
