import { definePlugin, ScriptContext } from "../../types";
import type {
  CallExpression,
  ObjectExpression,
} from "../../../../parser/ast/types";
import { ProcessContext, ProcessItemType } from "../../../types";
import { generateTypeString } from "../utils";

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

const HelperLocation = "$verter/options.helper.ts";

const MacroDependencies = new Map([
  [
    "defineEmits",
    ["UnionToIntersection", "EmitMapToProps", "OverloadParameters"],
  ],
  ["defineModel", ["ModelToProps", "UnionToIntersection", "ModelToEmits"]],
  ["defineOptions", ["DefineOptions"]],
]);

export const MacrosPlugin = definePlugin({
  name: "VerterMacro",

  post(s, ctx) {
    const isTS = ctx.block.lang.startsWith("ts");
    const macroBindinds = ctx.items.filter(
      (x) => x.type === ProcessItemType.MacroBinding
    );
    if (macroBindinds.length === 0) return;

    for (const macro of Macros) {
      const name = ctx.prefix(macro === "withDefaults" ? "defineProps" : macro);
      const itemMacro = macroBindinds.find((x) => x.macro === macro);
      const TemplateBinding = ctx.prefix("TemplateBinding");

      if (isTS) {
        if (itemMacro) {
          const str = generateTypeString(name, {
            from: TemplateBinding,
            key: name,
            isType: true,
          }, ctx);
          s.append(str);
        } else {
        }
      } else {
      }
    }
  },

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
        addMacroDependencies(macroName, ctx);

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
    addMacroDependencies(item.name, ctx);
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

function addMacroDependencies(macroName: string, ctx: ScriptContext) {
  if (!ctx.block.lang.startsWith("ts")) return;
  const dependencies = MacroDependencies.get(macroName);
  if (dependencies) {
    ctx.items.push({
      type: ProcessItemType.Import,
      asType: true,
      from: HelperLocation,
      items: dependencies.map((dep) => ({ name: ctx.prefix(dep) })),
    });
  }
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
    nameArg?.type === "Literal" ? nameArg.value?.toString() : "modelValue";

  return modelName;
}
