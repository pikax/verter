import { MagicString } from "@vue/compiler-sfc";
import { checkForSetupMethodCall, retrieveNodeString } from "../helpers.js";
import {
  LocationType,
  PluginOption,
  TypeLocationTemplate,
  WalkResult,
} from "../types.js";

import { parse } from "./parse.js";
// import { build } from "./builder.js";
import { process } from "./process.js";

export default {
  name: "Template",

  process: (context): TypeLocationTemplate => {
    const template = context.template;
    if (!template) return;

    const ast = template.ast;
    const source = template.content;

    if (!ast) return;

    const parsed = parse(ast);
    const declarations = [];
    // const result = build(parsed, [], declarations);
    const { magicString } = process(parsed, true);


    return {
      type: LocationType.Template,
      node: ast,
      content: magicString.toString(),
      map: magicString.generateMap({ hires: true, includeContent: true }),
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
