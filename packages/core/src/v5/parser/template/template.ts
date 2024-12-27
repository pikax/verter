import type {
  CommentNode,
  InterpolationNode,
  RootNode,
} from "@vue/compiler-core";
import { NodeTypes } from "@vue/compiler-core";
import { templateWalk, type TemplateWalkContext } from "../walk/index.js";
import { type CommmentResult, handleComment } from "./comment/comment.js";
import { TemplateBinding } from "./types.js";

export type ParsedTemplateResult = {
  root: RootNode;
  comments: CommmentResult[];
  bindings: TemplateBinding[];
  text: [];
  elements: [];
};

export function parseTemplate(ast: RootNode, source: string) {
  const comments: CommmentResult[] = [];

  templateWalk(
    ast,
    {
      enter(node, parent, context) {
        switch (node.type) {
          case NodeTypes.COMMENT: {
            const comment = handleComment(node);
            if (comment) {
              comments.push(comment);
            }
            break;
          }
          case NodeTypes.INTERPOLATION: {
            break;
          }
        }
      },
      leave(node, parent, context) {},
    },
    {
      conditions: [],
      inFor: false,
      ignoredIdentifiers: [],
    }
  );

  return {
    root: ast,
    source,
    comments,
  };
}
