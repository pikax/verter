import { retrieveNodeString } from "../helpers.js";
import { LocationType, PluginOption } from "../types.js";

import babel_types from "@babel/types";

export default {
  name: "Declaration",

  walk(node, context) {
    const source = context.script?.loc.source;
    if (!source) return;

    const supportedTypes = new Set([
      "VariableDeclaration",
      "FunctionDeclaration",
      "EnumDeclaration",
      "ClassDeclaration",
      "InterfaceDeclaration",
    ] as Array<babel_types.Node["type"]>);

    if (!supportedTypes.has(node.type)) return;
    const content = retrieveNodeString(node, source);

    return {
      type: LocationType.Declaration,
      generated: false,
      node,
      declaration: {
        content,
      },
      content: context.isSetup ? "pre" : "global",
    };
  },
} satisfies PluginOption;
