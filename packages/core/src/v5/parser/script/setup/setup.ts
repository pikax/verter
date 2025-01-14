import { VerterASTNode } from "../../ast";
import { ScriptItem, ScriptTypes } from "../types";

export function handleSetupNode(node: VerterASTNode):
  | ScriptItem[]
  | {
      items: ScriptItem[];
      isAsync: boolean;
    } {
  switch (node.type) {
    case "ExpressionStatement": {
      const expression = node.expression;
      if (expression.type === "AwaitExpression") {
        return {
          isAsync: true,
          items: [
            {
              type: ScriptTypes.Async,
              isAsync: true,
              node: expression,
            },
          ],
        };
      }

      if (expression.type === "CallExpression") {
        return [
          {
            type: ScriptTypes.FunctionCall,
            node: expression,
            parent: node,
            name:
              expression.callee.type === "Identifier"
                ? expression.callee.name
                : "",
          },
        ];
      }
    }

    case "ExportDefaultDeclaration": {
      return [
        {
          type: ScriptTypes.Error,
          node: node,
          message: "EXPORT_DEFAULT_SETUP",
          loc: null,
        },
      ];
    }

    case "ReturnStatement": {
      return [
        {
          type: ScriptTypes.Error,
          node: node,
          message: "NO_RETURN_IN_SETUP",
          loc: null,
        },
      ];
    }
    default:
      return [];
  }
}
