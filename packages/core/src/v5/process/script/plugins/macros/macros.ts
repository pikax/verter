import { definePlugin, ScriptContext } from "../../types";
import type { CallExpression } from "../../../../parser/ast/types";
import { ProcessContext, ProcessItemType } from "../../../types";
import { generateTypeDeclaration, generateTypeString } from "../utils";
import type { AvailableExports } from "@verter/types/string";

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

const HelperLocation = "$verter/types$";

const MacroDependencies = new Map<string, AvailableExports[]>([
  ["defineEmits", ["EmitsToProps"]],
  ["defineModel", ["ModelToProps", "ModelToEmits"]],
  ["defineOptions", ["DefineOptions"]],
]);

export const MacrosPlugin = definePlugin({
  name: "VerterMacro",
  hasWithDefaults: false,
  pre(s, ctx) {
    this.hasWithDefaults = false;
  },

  post(s, ctx) {
    const isTS = ctx.block.lang.startsWith("ts");
    const macroBindinds = ctx.items.filter(
      (x) => x.type === ProcessItemType.MacroBinding
    );
    for (const macro of Macros) {
      if (macro === "withDefaults") continue;
      if (macro === "defineOptions") {
        continue;
      }
      const name = ctx.prefix(macro);
      const itemMacro =
        macro === "defineModel"
          ? ctx.items.find((x) => x.type === ProcessItemType.DefineModel)
          : macroBindinds.find((x) => x.macro === macro);
      const TemplateBinding = ctx.prefix("TemplateBinding");

      if (itemMacro) {
        const str = generateTypeString(
          name,
          {
            from: TemplateBinding,
            key: name,
            isType: true,
          },
          ctx
        );
        s.append(str);
      } else {
        const str = generateTypeDeclaration(
          name,
          "{}",
          ctx.generic?.source,
          isTS
        );
        s.append(str);
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
                macro: "defineProps",
                node: defineProps,
              });
            }
            // prepend props
            // @ts-expect-error TODO improve this, this shouldn't be necessary
            s.appendLeft(item.declarator.start, `const ${pName}=`);
            s.appendLeft(defineProps.end, ";");
            s.appendRight(defineProps.end, pName);
            // @ts-expect-error TODO improve this, this shouldn't be necessary
            s.move(defineProps.start, defineProps.end, item.declarator.start);
            this.hasWithDefaults = true;

            // ctx.items.push({
            //   type: ProcessItemType.MacroBinding,
            //   name: varName!,
            //   macro: "defineProps",
            //   node: item.parent,
            // });
          } else {
            if (macroName === "defineProps" && this.hasWithDefaults) {
              return;
            }
            ctx.items.push({
              type: ProcessItemType.MacroBinding,
              name: varName!,
              macro: macroName,
              node: item.parent,
            });
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
    nameArg?.type === "Literal"
      ? nameArg.value?.toString() ?? "modelValue"
      : "modelValue";

  return modelName;
}
