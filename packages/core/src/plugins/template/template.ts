import { MagicString } from "@vue/compiler-sfc";
import { checkForSetupMethodCall, retrieveNodeString } from "../helpers.js";
import { LocationType, PluginOption, WalkResult } from "../types.js";

import { parse } from "./parse.js";
import { build } from "./builder.js";

export default {
  name: "Template",

  process: (context) => {
    const template = context.template;
    if (!template) return;

    const ast = template.ast;
    const source = template.content;

    if (!ast) return;

    const parsed = parse(ast);
    const declarations = [];
    const result = build(parsed, [], declarations);
    return {
      type: LocationType.Template,
      node: ast,
      content: result,
    };
    return {
      type: LocationType.Template,
      node: ast,
      generated: true,
      declaration: {
        name: "VUE_render",
        content: `${
          context.generic ? `<${context.generic},>` : ""
        }()=> { return (\n${result}\n) }`,
        type: "const",
      },
    };
  },
} satisfies PluginOption;
