import { VerterASTNode } from "../../ast";
import { TemplateBinding, TemplateTypes } from "../../template/types";
import { getASTBindings, retrieveBindings } from "../../template/utils";
import { ScriptDeclaration, ScriptItem, ScriptTypes } from "../types";

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
      return [];
    }
    case "VariableDeclaration": {
      return node.declarations.flatMap((x) => {
        const bindings = getASTBindings(x.id, {
          ignoredIdentifiers: [],
        });

        return bindings
          .filter(
            (x) =>
              x.type === TemplateTypes.Binding &&
              !x.parent?.type.startsWith("TS")
          )
          .map((b) => {
            return {
              type: ScriptTypes.Declaration,
              node: b.node as VerterASTNode,
              name: (b as TemplateBinding).name,
              parent: x,
              rest: false,
            } as ScriptDeclaration;
          });
      });
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
