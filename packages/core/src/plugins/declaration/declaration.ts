import { retrieveNodeString } from "../helpers.js";
import { LocationType, PluginOption } from "../types.js";

import babel_types from "@babel/types";

export default {
  name: "Declaration",

  walk(node, context) {
    if (!context.isSetup) return;

    const source = context.script?.loc.source;
    if (!source) return;

    const content = retrieveNodeString(node, source);

    // TODO add more
    const supportedTypes = new Set([
      "VariableDeclaration",
      "FunctionDeclaration",
      "EnumDeclaration",
      "ClassDeclaration",
    ] as Array<babel_types.Node["type"]>);

    if (!supportedTypes.has(node.type)) return;

    console.log(node.type, content);

    return {
      type: LocationType.Declaration,
      generated: false,
      declaration: {
        content,
      },
    };
  },
} satisfies PluginOption;
