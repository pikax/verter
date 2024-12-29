import { type InterpolationNode, NodeTypes } from "@vue/compiler-core";
import { TemplateBinding } from "../types";

import { retrieveBindings } from "../utils";

export type InterpolationContext = {
  ignoredIdentifiers: string[];
};

export function handleInterpolation<
  Context extends InterpolationContext = InterpolationContext
>(node: InterpolationNode, context: Context): TemplateBinding[] | null {
  if (node.type !== NodeTypes.INTERPOLATION) {
    return null;
  }
  return retrieveBindings(node.content, context);
}
