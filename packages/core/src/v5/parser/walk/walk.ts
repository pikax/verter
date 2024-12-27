import type {
  RootNode,
  ParentNode,
  ExpressionNode,
  TemplateChildNode,
  CompoundExpressionNode,
} from "@vue/compiler-core";
import type { VerterAST, Statement, ModuleDeclaration } from "../ast/index.js";
import { isObject } from "@vue/shared";

export function shallowWalk(
  root: VerterAST,
  cb: (node: Statement | ModuleDeclaration) => void
) {
  for (let i = 0; i < root.body.length; i++) {
    cb(root.body[i]);
  }
}

export type VerterNode = Exclude<
  ParentNode | ExpressionNode | TemplateChildNode,
  CompoundExpressionNode
>;

export type TemplateWalkContext = {
  conditions: string[];
  inFor: boolean;
  ignoredIdentifiers: string[];
};

export type WalkOptions<Context extends TemplateWalkContext> = {
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

export function templateWalk<Context extends TemplateWalkContext>(
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

    const childContext = {
      conditions:
        parentContext.conditions?.length > 0
          ? [...parentContext.conditions]
          : [],
      inFor: !!parentContext.for || !!parentContext.inFor,
    } as Context;

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
