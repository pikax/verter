import { retrieveNodeString } from "../helpers.js";
import { LocationType, PluginOption } from "../types.js";

import babel_types from "@babel/types";

const supportedTypes = new Set([
  "VariableDeclaration",
  "FunctionDeclaration",
  "EnumDeclaration",
  "ClassDeclaration",
  "InterfaceDeclaration",
  "TSTypeAliasDeclaration",
  "ExportNamedDeclaration",
] as Array<babel_types.Node["type"]>);

const globalDeclarations = new Set([
  // "InterfaceDeclaration",
  // "TSTypeAliasDeclaration",
  "ExportNamedDeclaration",
] as Array<babel_types.Node["type"]>);

export default {
  name: "Declaration",

  walk(node, context) {
    const source = context.script?.loc.source;
    if (!source) return;

    if (!supportedTypes.has(node.type)) return;
    const content = retrieveNodeString(node, source);

    return {
      type: LocationType.Declaration,
      generated: false,
      context: globalDeclarations.has(node.type) ? "global" : undefined,
      node,
      declaration: {
        content,
      },
    };
  },
} satisfies PluginOption;
