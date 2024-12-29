import { ElementNode } from "@vue/compiler-core";

export type ElementContext = {
  conditions: string[];
  inFor: boolean;
  ignoredIdentifiers: string[];
};

export function handleElement(
  node: ElementNode,
  parent: ElementNode,
  context: ElementContext
) {}
