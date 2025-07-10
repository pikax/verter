import { NodeTypes, type TextNode } from "@vue/compiler-core";
import { type TemplateText, TemplateTypes } from "../types.js";

export function handleText(node: TextNode): TemplateText | null {
  if (node.type !== NodeTypes.TEXT) {
    return null;
  }
  return {
    type: TemplateTypes.Text,
    content: node.content,
    node,
  };
}
