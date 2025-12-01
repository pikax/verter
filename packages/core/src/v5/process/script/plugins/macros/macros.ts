import { definePlugin, ScriptContext } from "../../types";
import type {
  CallExpression,
  VerterASTNode,
} from "../../../../parser/ast/types";
import { ProcessContext, ProcessItemType } from "../../../types";
import type { AvailableExports } from "@verter/types/string";
import { createHelperImport } from "../../../utils";
import { MagicString } from "@vue/compiler-sfc";
import type { TSType } from "@oxc-project/types";

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
const NoReturnMacros = new Set<string>(["defineOptions", "defineExpose"]);

// const PrettifyType = new Set<TSType["type"]>([
//   "TSStringKeyword",
//   "TSNumberKeyword",
// ]);

/**
 * If it should prettify the Type types otherwise on hover it will show `___VERTER___*_Type` instead
 * of the actual type
 * @param name
 * @param isTypeDeclaration
 * @returns
 */
function shouldPrettifyType(
  name: TSType | undefined,
  isTypeDeclaration: boolean
) {
  if (!name) return true;
  if (isTypeDeclaration) {
    if (
      name.type === "TSTypeReference" ||
      (name.type === "TSUnionType" &&
        name.types.every(
          (x) => x.type === "TSTypeReference" || x.type === "TSLiteralType"
        )) /*||
      (name.type.startsWith("TS") && name.type.endsWith("Keyword"))*/
    ) {
      return false;
    }
    return true;
  }
  return false;
}

type MacroInfo = {
  typeName?: string | null | undefined;
  valueName?: string | null | undefined;
  objectName?: string | null | undefined;
};

function generateMacroInfoString(info: MacroInfo): string {
  const parts = [
    info.valueName && `"value":{} as typeof ${info.valueName}`,
    info.typeName && `"type":{} as ${info.typeName}`,
    info.objectName && `"object":{} as typeof ${info.objectName}`,
    !info.typeName &&
      !info.objectName &&
      info.valueName &&
      `"object":{} as typeof ${info.valueName}`,
  ].filter(Boolean);
  return parts.join(",");
}

export const MacrosPlugin = definePlugin({
  name: "VerterMacro",

  pre(s, ctx) {
    ctx.items.push(createHelperImport(["Prettify"], ctx.prefix));
  },
  post(s, ctx) {
    const modelReturn = {} as Record<string, MacroInfo>;
    const macroBindings = {} as Record<string, MacroInfo>;

    for (const macro of ctx.items) {
      switch (macro.type) {
        case ProcessItemType.DefineModel: {
          modelReturn[macro.name] = {
            typeName: macro.typeName,
            valueName: macro.valueName,
            objectName: macro.objectName,
          };
          break;
        }
        case ProcessItemType.MacroBinding: {
          macroBindings[macro.macro] = {
            typeName: macro.typeName,
            valueName: macro.valueName,
            objectName: macro.objectName,
          };
          break;
        }
      }
    }

    const content = Object.entries(macroBindings)
      .map(([macro, info]) => {
        const name = normaliseDefineFromMacro(macro);
        const content = generateMacroInfoString(info);
        if (!content) {
          return "";
        }

        return `${name}:{${generateMacroInfoString(info)}}`;
      })
      .filter(Boolean);

    const modelEntries = Object.entries(modelReturn);
    if (modelEntries.length > 0) {
      const modelContent = modelEntries.map(
        ([modelName, info]) => `${modelName}:{${generateMacroInfoString(info)}}`
      );
      content.push(`model:{${modelContent.join(",")}}`);
    }

    ctx.items.push({
      type: ProcessItemType.MacroReturn,
      content: `{${content.join(",")}}`,
    });
  },

  transformDeclaration(item, s, ctx) {
    if (
      ctx.items.some(
        (x) =>
          (x.type === ProcessItemType.MacroBinding ||
            x.type === ProcessItemType.DefineModel) &&
          // if this is a declarator might have been processed already
          // @ts-expect-error TODO improve this, this shouldn't be necessary
          x.node.start >= item.declarator.start &&
          // @ts-expect-error TODO improve this, this shouldn't be necessary
          x.node.end <= item.declarator.end
      )
    ) {
      return;
    }
    if (
      item.parent.type === "VariableDeclarator" &&
      item.parent.init?.type === "CallExpression"
    ) {
      if (item.parent.init.callee.type === "Identifier") {
        return processMacroCall(
          item.parent.init,
          item.parent.id.type === "Identifier" ? item.parent.id.name : null,
          item.declarator,
          s,
          ctx
        );
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

    return processMacroCall(item.node, null, null, s, ctx);
  },
});

function processMacroCall(
  node: CallExpression,
  varName: string | null,
  declarator: VerterASTNode | null,
  s: MagicString,
  ctx: ScriptContext
) {
  const macroName =
    node.callee.type === "Identifier" ? node.callee.name : undefined;
  if (!macroName || !Macros.has(macroName)) {
    return;
  }
  if (!ctx.isSetup) {
    ctx.items.push({
      type: ProcessItemType.Warning,
      message: "MACRO_NOT_IN_SETUP",
      node: node,
      start: node.start,
      end: node.end,
    });
    return;
  }
  if (macroName === "defineOptions") {
    handleDefineOptions(node, ctx);
    return;
  }

  const start = declarator?.start ?? node.start;
  const end = declarator?.end ?? node.end;

  let prependName = macroName === "defineModel" ? getModelName(node) : null;

  const box = boxMacro(node, start, end, prependName, s, ctx);
  addMacroDependencies(macroName as MacroNames, box.info.name, ctx);

  // For NoReturnMacros (defineExpose, defineOptions) without a declarator,
  // we don't set varName since no variable is created
  const isNoReturn = NoReturnMacros.has(macroName);
  if (!varName && !(isNoReturn && !declarator)) {
    varName = box.info.varName;
  }

  switch (macroName) {
    case "defineModel": {
      const name = getModelName(node);
      const accessor = ctx.prefix("models");

      box.box();

      // override `varName` since it should be based on defineModel
      if (declarator) {
        if (
          declarator.type === "VariableDeclaration" &&
          declarator.declarations.length > 0
        ) {
          const decl = declarator.declarations[0];
          if (decl.id.type === "ArrayPattern") {
            varName = `${accessor}_${name}`;
            s.appendRight(start, `let ${varName};`);
            s.appendRight(node.start, `${varName}=`);
          }
        }
      } else {
        varName = `${accessor}_${name}`;
        s.appendRight(node.start, `const ${varName}=`);
      }
      const isType = !!node.typeArguments;

      ctx.items.push({
        type: ProcessItemType.DefineModel,
        varName: varName,
        name: name,
        node: node,

        isType,
        valueName: varName,
        typeName: isType ? box.info.type : undefined,
        objectName: node.arguments.length > 0 ? box.info.boxedName : undefined,
      });
      break;
    }
    case "withDefaults": {
      const defineProps = node.arguments[0];
      let propsBox: ReturnType<typeof boxMacro> | null = null;
      if (defineProps) {
        if (defineProps.type === "CallExpression") {
          ctx.items.push({
            type: ProcessItemType.Import,
            from: "vue",
            items: [{ name: "defineProps" }],
          });
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

          let splitFromOffset = -1;
          let splitToOffset = -1;

          propsBox = boxMacro(defineProps, start, end, null, s, ctx);

          const isType = !!defineProps.typeArguments;

          splitFromOffset = defineProps.typeArguments
            ? defineProps.typeArguments.start
            : defineProps.arguments[0].start;
          splitToOffset = defineProps.typeArguments
            ? defineProps.typeArguments.end
            : defineProps.arguments[defineProps.arguments.length - 1].end;

          if (defineProps.arguments.length > 0) {
            s.appendLeft(start, `;let ${propsBox.info.boxedName}`);
            s.appendLeft(
              defineProps.arguments[0].start,
              `${propsBox.info.boxedName}=${propsBox.info.boxName}(`
            );
            s.appendRight(
              defineProps.arguments[defineProps.arguments.length - 1].end,
              `)`
            );
          }

          // propsBox.byArguments.start(start);
          propsBox.byTypeArguments.start(end);
          // propsBox.byArguments.move(start);
          propsBox.byTypeArguments.move(end);
          // propsBox.byArguments.end(start);
          propsBox.byTypeArguments.end(end);

          ctx.items.push(createHelperImport([propsBox.info.name], ctx.prefix));

          ctx.items.push({
            type: ProcessItemType.MacroBinding,
            name: varName!,
            macro: "defineProps",
            node: defineProps,
            isType,
            valueName: varName,
            typeName: isType ? propsBox.info.type : undefined,
            objectName:
              defineProps.arguments.length > 0
                ? propsBox.info.boxedName
                : undefined,
          });
        }
      } else {
        // no props
      }

      box.box();

      // if (!declarator) {
      //   s.appendRight(node.start, `const ${varName}=`);
      // }

      // override `varName` since it should be based on defineModel
      if (declarator) {
        if (
          declarator.type === "VariableDeclaration" &&
          declarator.declarations.length > 0
        ) {
          const decl = declarator.declarations[0];
          if (
            decl.id.type === "ArrayPattern" ||
            decl.id.type === "ObjectPattern"
          ) {
            s.appendRight(start, `let ${varName};`);
            s.appendRight(node.start, `${varName}=`);
          }
        }
      } else {
        s.appendRight(node.start, `const ${varName}=`);
      }

      const isTypeModel = !!node.typeArguments;

      ctx.items.push({
        type: ProcessItemType.MacroBinding,
        name: varName ?? box.info.boxedName,
        macro: macroName,
        node: declarator ?? node,
        isType: isTypeModel,
        valueName: varName,
        objectName: node.arguments.length > 0 ? box!.info.boxedName : undefined,
        typeName: isTypeModel ? box!.info.type : undefined,
      });

      break;
    }

    default: {
      box.box();

      // For macros that don't return a value (defineOptions, defineExpose),
      // we don't create a variable assignment
      const isNoReturn = NoReturnMacros.has(macroName);

      if (declarator) {
        if (
          declarator.type === "VariableDeclaration" &&
          declarator.declarations.length > 0
        ) {
          const decl = declarator.declarations[0];
          if (
            decl.id.type === "ArrayPattern" ||
            decl.id.type === "ObjectPattern"
          ) {
            s.appendRight(start, `let ${varName};`);
            s.appendRight(node.start, `${varName}=`);
          }
        }
      } else if (!isNoReturn) {
        s.appendRight(node.start, `const ${varName}=`);
      }
      const isType = !!node.typeArguments;

      ctx.items.push({
        type: ProcessItemType.MacroBinding,
        name: varName ?? box.info.boxedName,
        macro: macroName,
        node: declarator ?? node,

        isType: isType,
        // For NoReturnMacros without a declarator, valueName should be undefined
        // since no variable is created
        valueName: isNoReturn && !declarator ? undefined : varName,
        objectName: node.arguments.length > 0 ? box!.info.boxedName : undefined,
        typeName: isType ? box!.info.type : undefined,
      });

      break;
    }
  }
}

function addMacroDependencies(
  macroName: MacroNames,
  importBox: AvailableExports,
  ctx: ScriptContext
) {
  // if (!ctx.block.lang.startsWith("ts")) return;

  ctx.items.push(createHelperImport([importBox], ctx.prefix));

  ctx.items.push({
    type: ProcessItemType.Import,
    from: "vue",
    items: [
      {
        name: macroName,
      },
    ],
  });
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

export function boxInfo(
  macroName: string,
  prependName: string | null,
  ctx: ScriptContext
) {
  const prepend = prependName ? `${prependName}_` : "";
  const boxedName = ctx.prefix(`${prepend}${macroName}_Boxed`);
  const name = `${macroName}_Box` as AvailableExports;
  const type = ctx.prefix(`${prepend}${macroName}_Type`);
  const boxName = ctx.prefix(`${name}`);
  const varName = ctx.prefix(normaliseDefineFromMacro(macroName));
  return { boxedName, boxName, name, type, varName };
}

function boxMacro(
  caller: CallExpression,
  toOffset: number,
  end: number,
  prependName: string | null,
  s: any,
  ctx: ScriptContext
) {
  const name: AvailableExports | undefined =
    caller.callee.type === "Identifier"
      ? (caller.callee.name as AvailableExports)
      : undefined;
  if (!name) {
    throw new Error("boxMacro: callee is not an identifier: " + caller.callee);
  }
  const info = boxInfo(name, prependName, ctx);
  ctx.items.push(
    createHelperImport([`${name}_Box` as AvailableExports], ctx.prefix)
  );

  const byArguments = macroBoxByArguments(caller, info, s);
  const byTypeArguments = macroBoxByTypeArguments(caller, info, s, ctx);

  function box() {
    // since we cannot move to the same offset, we need to adjust the type arguments offset
    const typeOffset =
      caller.arguments.length > 0 ||
      (caller.typeArguments?.params?.length ?? 0) > 0
        ? end
        : toOffset;
    byArguments.start(toOffset);
    byTypeArguments.start(typeOffset);

    byArguments.move(toOffset);
    byTypeArguments.move(typeOffset);

    // Call end AFTER move so the closing > appears after the moved content
    byArguments.end(toOffset);
    byTypeArguments.end(typeOffset);
  }

  return {
    box,
    info,

    byArguments,
    byTypeArguments,
  };
}

function macroBoxByTypeArguments(
  caller: CallExpression,
  info: ReturnType<typeof boxInfo>,
  s: MagicString,
  ctx: ScriptContext
): {
  start: (offset: number, method?: "appendLeft" | "prependLeft") => void;
  move: (offset: number) => void;
  end: (offset: number, method?: "appendRight" | "prependRight") => void;
} {
  function start(
    offset: number,
    method: "appendLeft" | "prependLeft" = "appendLeft"
  ) {
    const args = caller.typeArguments;
    if (!args) return;
    const arg = args.params[0];
  }
  function move(offset: number) {
    const args = caller.typeArguments;
    if (!args) return;
    let prepend = args.params.length > 1 ? "_0" : "";

    for (let i = 0; i < args.params.length; i++) {
      const arg = args.params[i];
      const argStart = arg.start!;
      const argEnd = arg.end!;
      // Additional logic for each type argument can be added here

      // Get the original type content
      // const originalTypeContent = s.original.slice(argStart, argEnd);
      const prettifyPrefix = shouldPrettifyType(arg, true)
        ? `${ctx.prefix("Prettify")}<`
        : "";
      const prettifySuffix = shouldPrettifyType(arg, true) ? ">" : "";

      // Create the complete type declaration
      const typeDeclaration = `;type ${info.type}${prepend}=${prettifyPrefix}`;

      // Insert the type declaration at offset
      s.appendRight(argStart, typeDeclaration);

      // Replace the type argument with the type alias name
      s.appendLeft(
        argStart,
        shouldPrettifyType(arg, false)
          ? `${ctx.prefix("Prettify")}<${info.type}${prepend}`
          : info.type + prepend
      );

      s.appendLeft(argEnd, `${prettifySuffix};`);

      s.move(argStart, argEnd, offset);

      prepend = `_${i + 1}`;
    }

    if (args.params.length > 1) {
      info.type += "_0";
    }
  }
  function end(
    offset: number,
    method: "appendRight" | "prependRight" = "appendRight"
  ) {
    // No longer needed - the closing > is added in start() now
    // const args = caller.typeArguments;
    // if (!args) return;
    // for (let i = 0; i < args.params.length; i++) {
    //   const arg = args.params[i];
    //   const start = arg.start;
    //   const end = arg.end;
    //   const prettifySuffix = shouldPrettifyType(arg, true) ? ">" : "";
    //   s[method](offset, `${prettifySuffix};`);
    // }

    return;
  }

  return { start, move, end };
}

function macroBoxByArguments(
  caller: CallExpression,
  info: ReturnType<typeof boxInfo>,
  s: MagicString
): {
  start: (offset: number, method?: "appendLeft" | "prependLeft") => void;
  move: (offset: number) => void;
  end: (offset: number, method?: "appendRight" | "prependRight") => void;
} {
  function start(
    offset: number,
    method: "appendLeft" | "prependLeft" = "appendLeft"
  ) {
    if (caller.arguments.length === 0) {
      // if (!caller.typeArguments) {
      //   // empty, no need to box
      //   s[method](offset, `let ${info.boxedName}=`);
      // }
      return;
    }

    // Check if this is a defineModel call with a name argument (first arg is a string literal)
    // defineModel_Box('name') returns [string, options] tuple, so we need to spread it
    const macroName =
      caller.callee.type === "Identifier" ? caller.callee.name : undefined;
    const firstArgIsStringLiteral =
      caller.arguments[0]?.type === "Literal" &&
      typeof caller.arguments[0].value === "string";
    const isNamedDefineModel =
      macroName === "defineModel" && firstArgIsStringLiteral;

    // Use tuple spread if multiple arguments OR if it's a named defineModel (always returns tuple)
    const useTupleSpread = caller.arguments.length > 1 || isNamedDefineModel;
    const start = caller.arguments[0].start;

    // Include original type arguments in the Box call to preserve type information
    let typeArgsStr = "";
    if (caller.typeArguments && caller.typeArguments.params.length > 0) {
      const typeArgStart = caller.typeArguments.params[0].start;
      const typeArgEnd =
        caller.typeArguments.params[caller.typeArguments.params.length - 1].end;
      typeArgsStr = `<${s.original.slice(typeArgStart, typeArgEnd)}>`;
    }

    s[method](
      offset,
      `;const ${info.boxedName}=${info.boxName}${typeArgsStr}(`
    );

    s[method](
      start!,
      useTupleSpread
        ? [1, 2].map((_, i) => `${info.boxedName}[${i}]`).join(",")
        : info.boxedName
    );
  }
  function move(offset: number) {
    if (caller.arguments.length === 0) return;
    const start = caller.arguments[0].start;
    const end = caller.arguments[caller.arguments.length - 1].end;
    s.move(start, end, offset);
  }
  function end(
    offset: number,
    method: "appendRight" | "prependRight" = "appendRight"
  ) {
    if (caller.arguments.length === 0) return;
    s[method](offset, `);`);
  }
  return { start, move, end };
}

function normaliseDefineFromMacro(name: string) {
  return name.startsWith("define")
    ? name[6].toLowerCase() + name.slice(7)
    : name;
}
