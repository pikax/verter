import { VerterASTNode } from "../../ast";
import { TemplateBinding, TemplateTypes } from "../../template/types";
import { getASTBindings, retrieveBindings } from "../../template/utils";
import { ScriptDeclaration, ScriptItem, ScriptTypes } from "../types";

export function handleSetupNode(
  node: VerterASTNode,
  isOptions = false
):
  | ScriptItem[]
  | {
      items: ScriptItem[];
      isAsync: boolean;
    } {
  switch (node.type) {
    case "ExpressionStatement": {
      let asyncItem:
        | {
            items: ScriptItem[];
            isAsync: boolean;
          }
        | undefined = undefined;
      let expression = node.expression;
      if (expression.type === "AwaitExpression") {
        asyncItem = {
          isAsync: true,
          items: [
            {
              type: ScriptTypes.Async,
              isAsync: true,
              node: expression,
            },
          ],
        };
        expression = expression.argument;
      }

      if (expression.type === "CallExpression") {
        const item: ScriptItem = {
          type: ScriptTypes.FunctionCall,
          node: expression,
          parent: node,
          name:
            expression.callee.type === "Identifier"
              ? expression.callee.name
              : "",
        };
        if (asyncItem) {
          asyncItem.items.push(item);
          return asyncItem;
        }
        return [item];
      }
      return asyncItem ?? [];
    }
    case "VariableDeclaration": {
      let asyncItem:
        | {
            items: ScriptItem[];
            isAsync: boolean;
          }
        | undefined = undefined;

      const items = node.declarations.flatMap((x) => {
        const bindings = getASTBindings(x.id, {
          ignoredIdentifiers: [],
        });

        if (x.init?.type === "AwaitExpression") {
          asyncItem = {
            isAsync: true,
            items: [
              {
                type: ScriptTypes.Async,
                isAsync: true,
                node: x.init,
              },
            ],
          };
        }
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
              declarator: node,
              parent: x,
              rest: false,
            } as ScriptDeclaration;
          });
      });

      if (asyncItem) {
        (
          asyncItem as {
            items: ScriptItem[];
          }
        ).items.push(...items);
        return asyncItem;
      }
      return items;
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
      if (isOptions) {
        // TODO return bindings from return statement
        return [];
      }
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
