import type {
  CommentNode,
  InterpolationNode,
  RootNode,
} from "@vue/compiler-core";
import { NodeTypes } from "@vue/compiler-core";
import { templateWalk, type TemplateWalkContext } from "../walk/index.js";
import { handleComment } from "./comment/comment.js";
import type {
  TemplateBinding,
  TemplateText,
  TemplateComment,
} from "./types.js";
import { handleInterpolation } from "./interpolation/interpolation.js";

export type ParsedTemplateResult = {
  root: RootNode;
  comments: TemplateComment[];
  bindings: TemplateBinding[];
  text: TemplateText[];
  elements: [];
};

export function parseTemplate(ast: RootNode, source: string) {
  const comments: TemplateComment[] = [];
  const bindings: TemplateBinding[] = [];

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
            const b = handleInterpolation(node, context);
            if (b) {
              bindings.push(...b);
            }
            break;
          }
          case NodeTypes.ELEMENT: {
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
