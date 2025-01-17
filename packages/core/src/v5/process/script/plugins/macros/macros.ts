import { ParsedBlockScript } from "../../../../parser/types";
import { definePlugin } from "../../types";
import type { CallExpression } from "../../../../parser/ast/types";

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
    console.log("sss", item);

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
        if (macroName === "defineModel") {
          const accessor = ctx.prefix("models");
          const modelName = getModelName(item.parent.init);
          varName = `${accessor}_${modelName}`;
        } else {
          varName = ctx.prefix(
            (macroName === "withDefaults" ? "defineProps" : macroName).replace(
              "define",
              ""
            )
          );

          // const accessor = ctx.prefix("models");
          // s.prependLeft(item.parent.start, "let ___TETTERT;");
        }

        if (ctx.isSetup) {
          s.prependLeft(item.declarator.start, `let ${varName};`);
          s.prependLeft(item.parent.id.end, `=${varName}`);
        } else {
          // todo add warning
        }
      }
    }
  },
  transformBinding(item, s, ctx) {
    console.log("sss", item);
  },

  transformFunctionCall(item, s, ctx) {
    if (!Macros.has(item.name)) {
      return;
    }
    let varName = "";
    if (item.name === "defineModel") {
      const modelName = getModelName(item.node);
      varName = `${ctx.prefix("models")}_${modelName}`;
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
    } else {
      // todo add warning
    }
  },
});

function getModelName(node: CallExpression) {
  const nameArg = node.arguments[0];
  const modelName = nameArg?.type === "Literal" ? nameArg.value : "modelValue";

  return modelName;
}
