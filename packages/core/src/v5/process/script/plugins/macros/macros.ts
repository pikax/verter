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
    type MacroInfo = {
      start: number;
      end: number;

      typeName?: string;
      valueName?: string;
      objectName?: string;
    };

    const modelReturn = {} as Record<string, MacroInfo>;
    const result = {} as Record<string, MacroInfo>;

    for (const macro of ctx.items) {
      switch (macro.type) {
        case ProcessItemType.DefineModel: {
          // modelReturn[macro.name] = {
          //   typeName: macro.varName,
          // };
          break;
        }
        case ProcessItemType.MacroBinding: {
          result[macro.macro] = {
            start: macro.node.start!,
            end: macro.node.end!,

            typeName: macro.typeName,
            valueName: macro.valueName,
            objectName: macro.objectName,
          };
          break;
        }
      }
    }

    const content = Object.entries(result).map(([macro, info]) => {
      const value = info.valueName
        ? `"value":{} as typeof ${info.valueName}`
        : ``;
      const type = info.typeName ? `"type":{} as ${info.typeName}` : ``;
      const object = info.objectName
        ? `"object":{} as typeof ${info.objectName}`
        : ``;

      return `${macro}:{${[value, type, object]
        .filter((x) => x !== "")
        .join(",")}}`;
    });

    ctx.items.push({
      type: ProcessItemType.MacroReturn,
      content: `{${content.join(",")}}`,
    });

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

        if (ctx.isSetup) {
          if (macroName === "defineModel") {
            ctx.items.push({
              type: ProcessItemType.DefineModel,
              varName,
              name: getModelName(item.parent.init),
              node: item.parent,
            });
          } else if (macroName == "withDefaults") {
            const defineProps = item.parent.init.arguments[0];

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

              const isType = !!defineProps.typeArguments;

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

                s.appendLeft(paramsStart, propsBoxInfo.type);
                // create type and prettify it with "{}&" otherwise on hover it will
                // show `defineProps_Type` only
                s.appendRight(paramsStart, `;type ${propsBoxInfo.type}={}&`);

                s.move(paramsStart, paramEnd, item.declarator.end!);
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

              ctx.items.push({
                type: ProcessItemType.MacroBinding,
                name: varName,
                macro: "defineProps",
                node: defineProps,
                isType,
                valueName: varName,
                typeName: isType ? propsBoxInfo.type : undefined,
                objectName: !isType ? propsBoxInfo.boxedName : undefined,
              });
            }

            const withDefaultOps = boxMacro(
              item.parent.init,
              item.declarator.start!,
              s,
              ctx
            );

            withDefaultOps!();

            const isTypeModel = !!item.parent.init.typeArguments;

            ctx.items.push({
              type: ProcessItemType.MacroBinding,
              name: varName,
              macro: macroName,
              node: item.parent,
              isType: isTypeModel !== undefined,
              valueName: varName,
              objectName: !isTypeModel
                ? withDefaultOps!.info.boxedName
                : undefined,
              typeName: isTypeModel ? withDefaultOps!.info.type : undefined,
            });

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
  const type = ctx.prefix(`${macroName}_Type`);
  const boxName = ctx.prefix(name);
  return { boxedName, boxName, name, type };
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
  const info = boxInfo(name, ctx);
  const { boxedName, boxName } = info;
  ctx.items.push(
    createHelperImport([`${name}_Box` as AvailableExports], ctx.prefix)
  );

  // TODO types and arguments both must be passed to the box function
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
    move.info = info;
    // move.boxedName = boxedName;
    // move.boxName = boxName;
    // move.name = info.name;
    // move.type = info.type;

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
