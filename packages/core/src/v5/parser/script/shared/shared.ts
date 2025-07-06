import { VerterASTNode } from "../../ast";
import { ScriptBinding, ScriptImport, ScriptItem, ScriptTypes } from "../types";

export function handleShared(node: VerterASTNode): ScriptItem[] | false {
  switch (node.type) {
    // case "ExpressionStatement": {
    //   const expression = node.expression;

    //   if (expression.type === "CallExpression") {
    //     return [
    //       {
    //         type: ScriptTypes.FunctionCall,
    //         node: expression,
    //         parent: node,
    //         name:
    //           expression.callee.type === "Identifier"
    //             ? expression.callee.name
    //             : "",
    //       },
    //     ];
    //   }

    //   return false;
    // }

    case "ExportAllDeclaration": {
      return [
        {
          type: ScriptTypes.Export,
          node: node,
        },
      ];
    }

    case "ImportDeclaration": {
      return [
        {
          type: ScriptTypes.Import,
          node: node,
          bindings:
            node.specifiers
              ?.map((x) => {
                switch (x.type) {
                  case "ImportSpecifier":
                  case "ImportDefaultSpecifier":
                  case "ImportNamespaceSpecifier": {
                    const name = x.local.name;
                    return {
                      type: ScriptTypes.Binding,
                      name,
                      node: x,
                    } as ScriptBinding;
                  }
                }
                return undefined;
              })
              .filter((x) => !!x) ?? [],
        },
      ];
    }

    default:
      return false;
  }
}

export function createSharedContext(opts: {
  lang: string | "ts" | "tsx" | "js" | "jsx";
}) {
  const isTs = opts.lang === "ts" || opts.lang === "tsx";

  function visit(
    node: VerterASTNode,
    parent: VerterASTNode | null,
    key?: string
  ): void | ScriptItem | ScriptItem[] {
    switch (node.type) {
      case "ExportNamedDeclaration":
      case "ExportAllDeclaration": {
        return {
          type: ScriptTypes.Export,
          node: node,
        };
      }
      case "ImportDeclaration": {
        const importItem: ScriptImport = {
          type: ScriptTypes.Import,
          node: node,
          bindings:
            node.specifiers
              ?.map((x) => {
                switch (x.type) {
                  case "ImportSpecifier":
                  case "ImportDefaultSpecifier":
                  case "ImportNamespaceSpecifier": {
                    const name = x.local.name;
                    return {
                      type: ScriptTypes.Binding,
                      name,
                      node: x,
                    } as ScriptBinding;
                  }
                }
              })
              .filter((x) => !!x) ?? [],
        };

        return [importItem, ...importItem.bindings];
      }

      case "TSTypeAssertion": {
        return {
          type: ScriptTypes.TypeAssertion,
          node,
        };
      }
    }
  }

  function leave(
    node: VerterASTNode,
    parent: VerterASTNode | null,
    key?: string
  ) {}

  return {
    visit,
    leave,
  };
}
