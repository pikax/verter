import type {
  RootNode,
  ParentNode,
  ExpressionNode,
  TemplateChildNode,
  CompoundExpressionNode,
} from "@vue/compiler-core";
import { isObject } from "@vue/shared";

export type VerterNode = Exclude<
  ParentNode | ExpressionNode | TemplateChildNode,
  CompoundExpressionNode
>;

export type WalkOptions<Context> = {
  enter?: (
    node: VerterNode,
    parent: VerterNode,
    context: Context,
    /**
     * Same for siblings
     */
    parentContext: Record<string, any>
  ) => Context | void;
  leave?: (
    node: VerterNode,
    parent: VerterNode,
    context: Context,
    /**
     * Same for siblings
     */
    parentContext: Record<string, any>
  ) => Context | void;
};

export function walk<Context>(
  root: RootNode,
  options: WalkOptions<Context>,
  context: Context
) {
  function visit(
    node: VerterNode | null | undefined,
    parent: VerterNode | null | undefined,
    context: Context,
    parentContext: Record<string, any>
  ) {
    if (!node) {
      return;
    }
    const returnedContext = options.enter?.(
      node,
      parent,
      context,
      parentContext
    );
    const overrideContext = returnedContext || context;

    const childContext = {} as Record<string, any>;

    if ("children" in node) {
      for (let i = 0; i < node.children.length; i++) {
        const element = node.children[i] as VerterNode;
        if (isObject(element)) {
          visit(element, node, overrideContext, childContext);
        }
      }
    }

    options.leave?.(node, parent, context, parentContext);
  }

  return visit(root, null, context, {});
}
