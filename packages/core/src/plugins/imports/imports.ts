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

    const supportedTypes = new Set(["ImportDeclaration"] as Array<
      babel_types.Node["type"]
    >);

    if (!supportedTypes.has(node.type)) return;
    const content = retrieveNodeString(node, source);

    return {
      type: LocationType.Import,
      offset: getContextOffset(context),

      generated: false,
      node,
      content: content,
    };
  },
} satisfies PluginOption;
