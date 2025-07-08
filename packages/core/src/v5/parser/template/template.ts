import type {
  CommentNode,
  InterpolationNode,
  RootNode,
} from "@vue/compiler-core";
import { NodeTypes } from "@vue/compiler-core";
import { templateWalk, type TemplateWalkContext } from "../walk/index.js";
import { handleComment } from "./comment/comment.js";
import {
  type TemplateBinding,
  type TemplateText,
  type TemplateComment,
  type TemplateCondition,
  type TemplateItem,
  TemplateTypes,
  TemplateProp,
  TemplateElement,
  TemplateDirective,
  TemplateRenderSlot,
  TemplateSlot,
  TemplateLoop,
} from "./types.js";
import { handleInterpolation } from "./interpolation/interpolation.js";
import { handleElement } from "./element/element.js";
import { handleText } from "./text/text.js";

export type ParsedTemplateResult = {
  root: RootNode;
  source: string;
  items: TemplateItem[];
};

export type ParseTemplateContext = {
  conditions: TemplateCondition[];
  inFor: boolean;
  ignoredIdentifiers: string[];

  blockDirection?: "Left" | "Right";
};

export function parseTemplate(
  ast: RootNode,
  source: string,
  ignoredIdentifiers: string[] = []
): ParsedTemplateResult {
  const items: TemplateItem[] = [];

  templateWalk(
    ast,
    {
      enter(node, parent, context) {
        let list: TemplateItem[] | null = null;
        let overrideContext: typeof context | null = null;
        switch (node.type) {
          case NodeTypes.COMMENT: {
            const comment = handleComment(node);
            if (comment) {
              // map[TemplateTypes.Comment].push(comment);
              list = [comment];
            }
            break;
          }
          case NodeTypes.INTERPOLATION: {
            list = handleInterpolation(node, context);
            break;
          }
          case NodeTypes.TEXT: {
            const t = handleText(node);
            if (t) {
              list = [t];
            }
            break;
          }
          case NodeTypes.ELEMENT: {
            const e = handleElement(node, parent!, context);
            if (e) {
              list = e.items;
              overrideContext = e.context;
            }
            break;
          }
        }

        items.push(...(list ?? []));

        // if (list) {
        //   for (let i = 0; i < list.length; i++) {
        //     const item = list[i];
        //     map[item.type].push(item as any);
        //     items.push(item);
        //   }
        // }
        if (overrideContext) {
          return overrideContext;
        }
      },
    },
    {
      conditions: [],
      inFor: false,
      ignoredIdentifiers,
    } as ParseTemplateContext
  );

  return {
    root: ast,
    source,
    items,
  };
}
