import { MagicString } from "@vue/compiler-sfc";
import { checkForSetupMethodCall, retrieveNodeString } from "../helpers.js";
import {
  LocationType,
  PluginOption,
  TypeLocationTemplate,
  WalkResult,
} from "../types.js";

import { parse } from "./parse";
import { process } from "./process";

export default {
  name: "Template",

  process: (context): TypeLocationTemplate => {
    const template = context.template;
    if (!template) return;

    const ast = template.ast;

    if (!ast) return;

    const parsed = parse(ast);
    // const result = build(parsed, [], declarations);
    const { declarations } = process(parsed, context.s, false);

    return [
      {
        type: LocationType.Template,
        node: parsed.node,
      },
      ...declarations,
    ];
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
