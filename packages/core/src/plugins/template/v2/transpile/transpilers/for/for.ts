import { NodeTypes } from "@vue/compiler-core";
import { createTranspiler, processExpression } from "../../utils";
import deepmerge from "deepmerge";

export default createTranspiler(NodeTypes.FOR, {
  enter(node, _, context) {
    const { source, value, key, index } = node.parseResult;

    const identifiers: string[] = [];

    if (key) {
      const r = processExpression(key, context, true);
    }

    return deepmerge(context, {
      ignoredIdentifiers: [context.ignoredIdentifiers, identifiers],
    });
  },
  leave(node, _, context) {},
});
