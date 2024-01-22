import { retrieveNodeString } from "../helpers.js";
import { LocationType, PluginOption } from "../types.js";

import babel_types from "@babel/types";

export default {
  name: "Declaration",

  walk(node, context) {
    // if (!context.isSetup) return;

    const source = context.script?.loc.source;
    if (!source) return;

    const content = retrieveNodeString(node, source);

    const declaration = node as babel_types.VariableDeclaration;

    // TODO add more
    const supportedTypes = new Set([
      "VariableDeclaration",
      "FunctionDeclaration",
      "EnumDeclaration",
      "ClassDeclaration",
    ] as Array<babel_types.Node["type"]>);

    if (!supportedTypes.has(node.type)) return;

    return {
      type: LocationType.Declaration,
      generated: false,
      node,
      declaration: {
        content,
      },
    };
  },
} satisfies PluginOption;
