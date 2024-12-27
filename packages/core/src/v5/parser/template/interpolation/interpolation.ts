import { type InterpolationNode, NodeTypes } from "@vue/compiler-core";
import { TemplateBinding } from "../types";
import * as babel_types from "@babel/types";

import { walk } from "@vue/compiler-sfc";
import { patchBabelNodeLoc } from "../../../utils/node";

export type InterpolationContext = {
  ignoredIdentifiers: string[];
};

export function handleInterpolation<
  Context extends InterpolationContext = InterpolationContext
>(node: InterpolationNode, context: Context): TemplateBinding[] | null {
  if (node.type !== NodeTypes.INTERPOLATION) {
    return null;
  }

  const bindings: TemplateBinding[] = [];

  const content = node.content;
  switch (content.type) {
    case NodeTypes.SIMPLE_EXPRESSION: {
      if (content.ast) {
        walk(content.ast, {
          enter(node: babel_types.Node, parent: babel_types.Node) {
            switch (node.type) {
              case "Identifier": {
                const name = node.name;
                if (
                  parent.type === "MemberExpression" &&
                  parent.property === node
                ) {
                  this.skip();
                  return;
                }
                const pNode = patchBabelNodeLoc(node, content);
                bindings.push({
                  node: pNode,
                  name,
                  ignore: context.ignoredIdentifiers.includes(name),
                });
                break;
              }
            }
          },
        });
      } else {
        const name = content.content;
        bindings.push({
          node,
          name,
          ignore: context.ignoredIdentifiers.includes(name),
        });
      }
      break;
    }
    case NodeTypes.COMPOUND_EXPRESSION: {
      // TODO handle compound expression
      break;
    }
  }

  return bindings;
}
