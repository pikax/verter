import { CommentNode, NodeTypes } from "@vue/compiler-core";
import { TemplateTypes, type TemplateComment } from "../types.js";

export function handleComment(node: CommentNode): TemplateComment | null {
  if (node.type !== NodeTypes.COMMENT) {
    return null;
  }
  return {
    type: TemplateTypes.Comment,
    content: node.content,
    node,
  };
}
