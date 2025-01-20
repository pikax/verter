import { definePlugin } from "../../types";
import type {
  CallExpression,
  ObjectExpression,
} from "../../../../parser/ast/types";
import { ProcessContext, ProcessItemType } from "../../../types";

const Macros = new Set([
  "defineProps",
  "defineEmits",
  "defineExpose",
  "defineOptions",
  "defineModel",
  "defineSlots",
  "withDefaults",

  // useSlots()/useAttrs()
]);

export const MacrosPlugin = definePlugin({
  name: "VerterMacro",

  transformDeclaration(item, s, ctx) {
    if (
      item.parent.type === "VariableDeclarator" &&
      item.parent.init?.type === "CallExpression"
    ) {
      if (item.parent.init.callee.type === "Identifier") {
        const macroName = item.parent.init.callee.name;
        if (!Macros.has(macroName)) {
          return;
        }

        let varName = "";
        let originalName: string | undefined = undefined;
        if (macroName === "defineModel") {
          const accessor = ctx.prefix("models");
          const modelName = getModelName(item.parent.init);
          varName = `${accessor}_${modelName}`;
          originalName = modelName;
        } else if (macroName === "defineOptions") {
          return handleDefineOptions(item.parent.init, ctx);
        } else {
          varName = ctx.prefix(
            (macroName === "withDefaults" ? "defineProps" : macroName).replace(
              "define",
              ""
            )
          );
        }

        if (ctx.isSetup) {
          s.prependLeft(item.declarator.start, `let ${varName};`);
          s.prependLeft(item.parent.id.end, `=${varName}`);
          ctx.items.push({
            type: ProcessItemType.MacroBinding,
            name: varName,
            macro: macroName,
            originalName,
          });
        } else {
          ctx.items.push({
            type: ProcessItemType.Warning,
            message: "MACRO_NOT_IN_SETUP",
            node: item.node,
            start: item.node.start,
            end: item.node.end,
          });
        }
      }
    }
  },
  transformFunctionCall(item, s, ctx) {
    if (!Macros.has(item.name)) {
      return;
    }
    let varName = "";
    let originalName: string | undefined = undefined;
    if (item.name === "defineModel") {
      const modelName = getModelName(item.node);
      const accessor = ctx.prefix("models");
      varName = `${accessor}_${modelName}`;
      originalName = modelName;
    } else if (item.name === "defineOptions") {
      return handleDefineOptions(item.node, ctx);
    } else {
      varName = ctx.prefix(
        (item.name === "withDefaults" ? "defineProps" : item.name).replace(
          "define",
          ""
        )
      );
    }

    if (ctx.isSetup) {
      s.prependLeft(item.node.start, `const ${varName}=`);
      ctx.items.push({
        type: ProcessItemType.MacroBinding,
        name: varName,
        macro: item.name,
        originalName,
      });
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
    nameArg?.type === "Literal" ? nameArg.value?.toString() : "modelValue";

  return modelName;
}
