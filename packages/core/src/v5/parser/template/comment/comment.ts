import { CommentNode, NodeTypes } from "@vue/compiler-core";

export type CommmentResult = {
  content: string;
  node: CommentNode;
};

export function handleComment(node: CommentNode): CommmentResult | null {
  if (node.type !== NodeTypes.COMMENT) {
    return null;
  }
  return {
    content: node.content,
    node,
  };
}
