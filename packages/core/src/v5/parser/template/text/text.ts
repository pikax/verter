import { NodeTypes, type TextNode } from "@vue/compiler-core";

export type TextResult = {
  content: string;
  node: TextNode;
};

export function handleText(node: TextNode): TextResult | null {
  if (node.type !== NodeTypes.TEXT) {
    return null;
  }
  return {
    content: node.content,
    node,
  };
}
