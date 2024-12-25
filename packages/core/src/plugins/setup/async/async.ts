import { isFunctionType } from "@vue/compiler-core";
import { PluginOption } from "../../types.js";

import { walk } from "@vue/compiler-sfc";

export default {
  name: "Async",

  process(context) {
    if (!context.isSetup) return;
    // similar implementation as vue 3
    // https://github.com/vuejs/core/blob/34a97edd2c8273c213599c44770accdb0846da8e/packages/compiler-sfc/src/compileScript.ts#L618-L652
    walk(context.script.scriptSetupAst, {
      enter(node) {
        if (isFunctionType(node)) {
          this.skip();
        }

        if (node.type === "AwaitExpression") {
          context.isAsync = true;
          this.skip();
        }
      },
    });
  },
} satisfies PluginOption;
