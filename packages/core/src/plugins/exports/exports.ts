import { PluginOption, LocationType } from "../types.js";

import {
  getContextOffset,
  getContextSource,
  retrieveNodeString,
} from "../helpers.js";
import babel_types from "@babel/types";

export default {
  name: "Imports",
  walk(node, context) {
    const source = getContextSource(context);
    if (!source) return;

    const supportedTypes = new Set([
      "ExportAllDeclaration",
      "ExportDefaultDeclaration",
      "ExportNamedDeclaration",
      "ExportNamespaceSpecifier",
      // "ExportSpecifier",
    ] as Array<babel_types.Node["type"]>);

    if (!supportedTypes.has(node.type)) return;
    const content = retrieveNodeString(node, source);

    return {
      type: LocationType.Export,
      offset: getContextOffset(context),

      generated: false,
      node,
      content: content,
      item: {
        named:
          node.type === "ExportNamedDeclaration"
            ? node.specifiers
                .map((x) =>
                  "name" in x.exported ? x.exported.name : x.exported.value
                )
                .join(", ")
            : undefined,
        default: node.type === "ExportDefaultDeclaration",
        content,
      },
    };
  },
} satisfies PluginOption;
