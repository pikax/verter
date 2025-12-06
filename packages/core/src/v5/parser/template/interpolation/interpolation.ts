import { type InterpolationNode, NodeTypes } from "@vue/compiler-core";
import {
  TemplateBinding,
  TemplateBrokenExpression,
  TemplateFunction,
  TemplateInterpolation,
  TemplateLiteral,
  TemplateTypes,
} from "../types";

import { retrieveBindings } from "../utils";

export type InterpolationContext = {
  ignoredIdentifiers: string[];
};

export function handleInterpolation<
  Context extends InterpolationContext = InterpolationContext
>(
  node: InterpolationNode,
  context: Context
):
  | [
      TemplateInterpolation,
      ...Array<TemplateBinding | TemplateFunction | TemplateLiteral | TemplateBrokenExpression>
    ]
  | null {
  if (node.type !== NodeTypes.INTERPOLATION) {
    return null;
  }

  const interpolation = {
    type: TemplateTypes.Interpolation,
    node,
  } as TemplateInterpolation;

  return [interpolation, ...retrieveBindings(node.content, context)];
}
