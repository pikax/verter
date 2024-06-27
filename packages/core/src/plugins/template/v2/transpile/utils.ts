import {
  NodeTypes,
  walkIdentifiers,
  type ExpressionNode,
} from "@vue/compiler-core";
import { walk } from "@vue/compiler-sfc";
import { VerterNode, WalkOptions } from "../walk/index.js";
import type { TranspileContext } from "./types.js";

import * as babel_types from "@babel/types";
import { isGloballyAllowed, isString, makeMap } from "@vue/shared";
import {
  isExpression as isExpressionBabel,
  isNode as isNodeBabel,
} from "@babel/types";

export type TranspilerOptions<T extends NodeTypes> = {
  enter?: <ParentContext extends Record<string, any> = Record<string, any>>(
    node: VerterNode & { type: T },
    parent: VerterNode,
    context: TranspileContext,
    parentContext: ParentContext
  ) => TranspileContext | void;
  leave?: <ParentContext extends Record<string, any> = Record<string, any>>(
    node: VerterNode & { type: T },
    parent: VerterNode,
    context: TranspileContext,
    parentContext: ParentContext
  ) => TranspileContext | void;
};
export type TranspilerPlugin<T extends NodeTypes = NodeTypes> = [
  T,
  TranspilerOptions<T>
];

export function createTranspiler<T extends NodeTypes>(
  type: T,
  options: TranspilerOptions<T>
): TranspilerPlugin<T> {
  return [type, options];
}

export function reducePlugins<T extends TranspilerPlugin>(plugins: T[]) {
  return plugins.reduce((prev, [type, options]) => {
    prev[type] = options;
    return prev;
  }, {} as Record<NodeTypes, WalkOptions<TranspileContext>>);
}

export function generateNarrowCondition(
  context: TranspileContext,
  isBlock: boolean
) {
  const toNegate = context.conditions.ifs.join(" && ");
  const conditions = [
    toNegate ? `!(${toNegate})` : "",
    ...context.conditions.elses,
  ]
    .filter(Boolean)
    .join(" || ");

  if (!conditions) return "";

  if (isBlock) {
    return `if(${conditions}) { return; } `;
  }
  return `${conditions} ? undefined : `;
}
export function withNarrowCondition(
  text: string | string[],
  context: TranspileContext,
  siblingsConditions: string[] = []
) {
  return [
    ...[text].flat(),
    generateNarrowCondition(
      siblingsConditions.length > 0
        ? {
            ...context,
            conditions: {
              ...context.conditions,
              elses: [...context.conditions.elses, ...siblingsConditions],
            },
          }
        : context,
      true
    ),
  ]
    .filter(Boolean)
    .join("\n");
}

const isLiteralWhitelisted = /*#__PURE__*/ makeMap("true,false,null,this,");
export function appendCtx(
  node: ExpressionNode | babel_types.Expression | string,
  context: TranspileContext,

  /**
   * if true magic string wont be updated
   */
  dry = false,
  /**
   * only used for the offset when using babel
   */
  __offset: number = 0,

  __extraPrepend: string = ""
) {
  let content = "";

  let start = -1;
  let end = -1;

  if (isString(node)) {
    content = node;
  } else if (isNodeBabel(node)) {
    if (__offset === 0) {
      debugger;
      console.warn("no offset has been passed");
    }
    if (isExpressionBabel(node)) {
      switch (node.type) {
        case "Identifier": {
          content = node.name;

          // fix the offset, because whitespaces are not counted
          start = context.s.original.indexOf(
            content,
            __offset + node.loc.start.index
          );
          end = start + content.length;

          break;
        }
        default: {
          console.error("VERTER unknonw node type ", node.type);
          debugger;
          break;
        }
      }
    }
  } else {
    content =
      node.type == NodeTypes.SIMPLE_EXPRESSION
        ? node.content
        : node.identifiers[0];
    if (!content) {
      console.log('no content', node)
    }

    start = node.loc.start.offset;
    end = node.loc.end.offset;
  }
  const accessor = context.accessors.ctx;

  if (
    isGloballyAllowed(content) ||
    isLiteralWhitelisted(content) ||
    ~context.ignoredIdentifiers.indexOf(content) ||
    !accessor
  ) {
    return content;
  }
  if (!dry && !isString(node)) {
    if (start === end) {
      console.warn("VERTER same start/end", content, start, end);
      return;
    }
    // there's a case where the offset is off
    if (context.s.original.slice(start, end) !== content) {
      context.s.appendRight(start + 1, `${__extraPrepend}${accessor}.`);
    } else {
      // if (context.s.original[start - 1] === "[") {
      //   context.s.prependLeft(start, `${accessor}.`);
      // } else {
      context.s.appendRight(start, `${__extraPrepend}${accessor}.`);
      // }
    }
  }
  return `${__extraPrepend}${accessor}.${content}`;
}

export function processExpression(
  exp: ExpressionNode,
  context: TranspileContext,
  dry = false,
  append = true
) {
  switch (exp.type) {
    case NodeTypes.SIMPLE_EXPRESSION: {
      if (exp.ast) {
        const ast = exp.ast;
        walkIdentifiers(ast, (id, parent) => {
          const extraPrepend =
            parent?.type === "ObjectProperty" && parent.shorthand
              ? `${id.name}:`
              : "";

          appendCtx(
            id,
            context,
            false,
            exp.loc.start.offset - ast.start,
            extraPrepend
          );
        });

        walk(ast as any, {
          enter(node) {
            // note if we want we can walk AST to patch all the function/blocks
            // but let's start just by just patching the first
            switch (node.type) {
              case "FunctionExpression":
              case "ArrowFunctionExpression": {
                const inblock = node.body.type === "BlockStatement";
                const condition = generateNarrowCondition(context, inblock);

                // append condition
                if (condition) {
                  const start =
                    exp.loc.start.offset -
                    ast.start +
                    (node.body.type === "BlockStatement"
                      ? (node.body.body[0] as any).start
                      : (node.body as any).start);

                  context.s.prependRight(start, condition);
                }
                break;
              }
            }
          },
        });

        return;
      }
      if (exp.isStatic || !append) {
        return exp.content;
      }

      return appendCtx(exp, context, dry);
    }
    case NodeTypes.COMPOUND_EXPRESSION: {
      if (exp.ast) {
        // todo retrieve ast
        return;
      }
      return exp.identifiers[0];
    }
  }
}
