import { definePlugin, ScriptContext } from "../../types";
import type {
  CallExpression,
  VariableDeclarator,
} from "../../../../parser/ast/types";
import { ProcessItemMacroBinding, ProcessItemType } from "../../../types";
import { ScriptBinding, ScriptTypes } from "../../../../parser";
import type { MagicString } from "@vue/compiler-sfc";

export const SupportedMacros = [
  "withDefaults",
  "defineProps",
  "defineEmits",
  "defineSlots",
  "defineExpose",
  "defineOptions",
  "defineModel",
] as const;

type SupportedMacrosType = typeof SupportedMacros extends readonly (infer T)[]
  ? T
  : never;

const Macros = new Set<string>(SupportedMacros);

const NonReturningMacros = new Set<string>([
  "defineExpose",
  "defineOptions",
] as SupportedMacrosType[]);

export const MacrosPlugin = definePlugin({
  name: "VerterMacro",

  transformDeclaration(item, s, ctx) {
    if (
      !ctx.isSetup ||
      !(
        item.parent.type === "VariableDeclarator" &&
        item.parent.init?.type === "CallExpression" &&
        item.parent.init.callee.type === "Identifier"
      )
    ) {
      return;
    }

    const name = item.parent.init.callee.name;
    const varName =
      item.parent.id.type === "Identifier" ? item.parent.id.name : undefined;

    if (handleOptionsMacro(name, item.parent, ctx)) {
      return;
    }

    this._handleMacro(
      name,
      item.parent.init,
      s,
      ctx,
      item.declarator!.start,
      false,
      varName
    );
  },

  transformFunctionCall(item, s, ctx) {
    if (!ctx.isSetup) {
      return;
    }

    const varName =
      isValidMacro(item.name) && item.name !== "defineModel"
        ? ctx.prefix(
            item.name === "withDefaults"
              ? "PropsValue"
              : item.name.slice("define".length) + "Value"
          )
        : undefined;

    if (handleOptionsMacro(item.name, item.node, ctx)) {
      return;
    }

    const hasValueVariable = !NonReturningMacros.has(item.name);

    this._handleMacro(
      item.name,
      item.node,
      s,
      ctx,
      item.parent.start,
      hasValueVariable,
      hasValueVariable ? varName : undefined
    );
  },

  _handleMacro(
    macro: string,
    node: CallExpression,
    s: MagicString,
    ctx: ScriptContext,
    toPos: number,
    needsDeclarator: boolean,
    varName?: string
  ) {
    if (
      !isValidMacro(macro) ||
      ctx.items.some(
        (x) =>
          (x.type === ProcessItemType.MacroBinding ||
            x.type === ProcessItemType.DefineModel) &&
          // check if is inside another macro
          x.node.start <= node.start &&
          x.node.end >= node.end
      ) ||
      hasOutsideMacroDeclaration(macro, ctx)
    ) {
      return;
    }

    if (needsDeclarator && varName) {
      s.prependRight(node.start, `const ${varName}=`);
    }

    let processDeclarationType = true;

    const declarationType =
      node.typeArguments?.type === "TSTypeParameterInstantiation" &&
      node.typeArguments.params.length > 0
        ? "type"
        : node.arguments.length > 0
        ? "object"
        : "empty";

    if (macro === "defineModel") {
      if (!varName) {
        const modelName = getModelName(node);
        const accessor = ctx.prefix("models");
        varName = `${accessor}_${modelName}`;

        s.prependRight(node.start, `const ${varName}=`);
      }

      ctx.items.push({
        type: ProcessItemType.DefineModel,
        varName,
        name: getModelName(node),
        node: node,
      });

      createRawBinding(macro, declarationType, node, toPos, s, ctx);

      return;
    }

    if (macro === "withDefaults") {
      const definePropsNode = node.arguments[0];
      // if is defaults we don't need to transform the declaration
      if (
        definePropsNode.type === "CallExpression" &&
        definePropsNode.callee.type === "Identifier" &&
        definePropsNode.callee.name === "defineProps"
      ) {
        this._handleMacro("defineProps", definePropsNode, s, ctx, toPos, false);
        processDeclarationType = false;
      }
    }

    const binding = {
      type: ProcessItemType.MacroBinding,
      name: varName,
      macro,
      node,

      declarationType,
    } as ProcessItemMacroBinding;

    ctx.items.push(binding);

    if (!processDeclarationType) {
      return;
    }

    binding.declarationName = createRawBinding(
      macro,
      declarationType,
      node,
      toPos,
      s,
      ctx
    );
  },
});

function isValidMacro(name: string): name is SupportedMacrosType {
  return Macros.has(name);
}

function createRawBinding(
  macro: SupportedMacrosType,
  declarationType: "object" | "type" | "empty",
  node: CallExpression,
  toPos: number,
  s: MagicString,
  ctx: ScriptContext
) {
  switch (declarationType) {
    case "type": {
      const typeName = ctx.prefix(macro.slice("define".length) + "Type");

      const typeNode = node.typeArguments!.params[0];
      s.move(typeNode.start, typeNode.end, toPos);

      s.prependRight(typeNode.start, `type ${typeName}=`);
      s.prependLeft(typeNode.end, `;`);
      // also prettify the type, otherwise it will be define*<typename>
      s.prependLeft(typeNode.start, typeName + "&{}");

      return typeName;
    }
    case "object": {
      const helperName = ctx.prefix(macro);
      const extractValue = ctx.prefix("extractValue");
      const varName = ctx.prefix(macro.slice("define".length) + "Raw");

      const argNode = node.arguments[0];
      s.move(argNode.start, argNode.end, toPos);

      s.prependRight(argNode.start, `const ${varName}=${helperName}(`);
      s.prependLeft(argNode.end, `);`);
      s.prependLeft(argNode.start, `${extractValue}(${varName})`);

      return varName;
    }
    default:
      return undefined;
  }
}

function hasOutsideMacroDeclaration(
  macro: SupportedMacrosType,
  ctx: ScriptContext
) {
  const items = ctx.processedItems.filter(
    (x) =>
      (x.type === ScriptTypes.Import &&
        (x.bindings as ScriptBinding[]).some((x) => x.name === macro) &&
        x.node.source.value !== "vue") ||
      (x.type === ScriptTypes.Declaration && x.name === macro)
  );

  return items.length > 0;
}

// Options are a bit different because they must be moved outside the setup
function handleOptionsMacro(
  macro: string,
  node: CallExpression | VariableDeclarator,
  ctx: ScriptContext
) {
  if (macro === "defineOptions") {
    // don't add new item if there's already an warning
    if (
      ctx.items.some(
        (x) =>
          (x.type === ProcessItemType.Warning &&
            x.message === "INVALID_DEFINE_OPTIONS") ||
          (x.type === ProcessItemType.Options &&
            x.node.start <= node.start &&
            x.node.end >= node.end)
      )
    ) {
      return;
    }
    const callNode =
      node.type === "CallExpression"
        ? node
        : node.init?.type === "CallExpression"
        ? node.init
        : undefined;

    if (callNode && callNode.arguments.length > 0) {
      const [arg] = callNode.arguments;
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
      if (callNode.arguments.length > 1) {
        ctx.items.push({
          type: ProcessItemType.Warning,
          message: "INVALID_DEFINE_OPTIONS",
          node: node,
          start: arg.end,
          end: node.end,
        });
      }
    } else {
      ctx.items.push({
        type: ProcessItemType.Warning,
        message: "INVALID_DEFINE_OPTIONS",
        node: node,
        start: node.start,
        end: node.end,
      });
    }

    return true;
  }

  return false;
}

function getModelName(node: CallExpression) {
  const nameArg = node.arguments[0];
  const modelName =
    nameArg?.type === "Literal"
      ? nameArg.value?.toString() ?? "modelValue"
      : "modelValue";

  return modelName;
}
