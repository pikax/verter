import { NOOP } from "@vue/shared";
import {
  ExpressionStatement,
  IdentifierName,
  VerterAST,
  VerterASTNode,
} from "../../ast";
import { TemplateBinding, TemplateTypes } from "../../template/types";
import { getASTBindings, retrieveBindings } from "../../template/utils";
import { VerterNode } from "../../walk";
import { ScriptDeclaration, ScriptItem, ScriptTypes } from "../types";
import { walk } from "@vue/compiler-sfc";

export function handleSetupNode(
  pnode: VerterASTNode | VerterAST,
  shallowCb?: (node: VerterASTNode) => any,
  isOptions = false
): {
  items: ScriptItem[];
  isAsync: boolean;
} {
  const items: ScriptItem[] = [];
  let isAsync = false;

  let trackDeclarations = true;

  walk(pnode, {
    enter(node: VerterASTNode, parent: VerterASTNode | null) {
      if (parent === pnode && shallowCb) {
        const r = shallowCb(node);
        if (Array.isArray(r)) {
          items.push(...r);
        }
      }
      switch (node.type) {
        case "ImportDeclaration":
        case "FunctionDeclaration":
        case "FunctionExpression":
        case "ArrowFunctionExpression":
          this.skip();
          return;
        case "AwaitExpression": {
          isAsync = true;
          items.push({
            type: ScriptTypes.Async,
            isAsync: true,
            node: node,
          });
          break;
        }
        case "CallExpression": {
          items.push({
            type: ScriptTypes.FunctionCall,
            node: node,
            parent: parent as ExpressionStatement,
            name:
              node.callee.type === "Identifier"
                ? node.callee.name
                : node.callee.type === "MemberExpression"
                ? (node.callee.property as IdentifierName)?.name
                : "",
          });
          this.skip();
          break;
        }

        case "VariableDeclaration": {
          if (!trackDeclarations) {
            this.skip();
            break;
          }
          const declarations = node.declarations.flatMap((x) => {
            const bindings = getASTBindings(x.id, {
              ignoredIdentifiers: [],
            });

            if (x.init?.type === "AwaitExpression") {
              isAsync = true;
              items.push({
                type: ScriptTypes.Async,
                isAsync: true,
                node: x.init,
              });
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

          this.skip();

          items.push(...declarations);
          break;
        }

        case "ExportDefaultDeclaration": {
          items.push({
            type: ScriptTypes.Error,
            node: node,
            message: "EXPORT_DEFAULT_SETUP",
            loc: null,
          });
          this.skip();
          break;
        }

        case "ReturnStatement": {
          if (isOptions) {
            // TODO return bindings from return statement
            return [];
          }
          items.push({
            type: ScriptTypes.Error,
            node: node,
            message: "NO_RETURN_IN_SETUP",
            loc: null,
          });
          this.skip();
          break;
        }

        // if any variable is declared in a block or something it shouldn't be tracked
        case "ForInStatement":
        case "ForOfStatement":
        case "ForStatement":
        case "WhileStatement":
        case "DoWhileStatement":
        case "IfStatement":
        case "SwitchStatement":
        case "TryStatement":
        case "BlockStatement": {
          trackDeclarations = false;
          break;
        }
      }
    },
    leave(node: VerterASTNode, parent: VerterASTNode | null) {
      switch (node.type) {
        case "ForInStatement":
        case "ForOfStatement":
        case "ForStatement":
        case "WhileStatement":
        case "DoWhileStatement":
        case "IfStatement":
        case "SwitchStatement":
        case "TryStatement":
        case "BlockStatement": {
          trackDeclarations = true;
          break;
        }
      }
    },
  });

  return {
    items,
    isAsync,
  };
}

export function createSetupContext(opts: {
  lang: string | "ts" | "tsx" | "js" | "jsx";
}) {
  let trackDeclarations = true;
  let isAsync = false;

  let ignore = false;

  const ignoreNodeTypes = new Set<VerterASTNode["type"]>([
    "ImportDeclaration",
    "FunctionDeclaration",
    "FunctionExpression",
    "ArrowFunctionExpression",
    "CallExpression",
    "VariableDeclaration",
    "ExportDefaultDeclaration",
    "ReturnStatement",
  ]);

  function visit(
    node: VerterASTNode,
    parent: VerterASTNode | null,
    key?: string,
    context: { skip: () => void } = { skip: NOOP }
  ): void | ScriptItem | ScriptItem[] {
    if (ignore) {
      return;
    }
    switch (node.type) {
      case "ImportDeclaration":
      case "FunctionDeclaration":
      case "FunctionExpression":
      case "ArrowFunctionExpression":
        context.skip();
        return;
      case "AwaitExpression": {
        isAsync = true;
        return {
          type: ScriptTypes.Async,
          isAsync: true,
          node: node,
        };
      }
      case "CallExpression": {
        return {
          type: ScriptTypes.FunctionCall,
          node: node,
          parent: parent as ExpressionStatement,
          name:
            node.callee.type === "Identifier"
              ? node.callee.name
              : node.callee.type === "MemberExpression"
              ? node.callee.property.type === "Identifier"
                ? node.callee.property.name
                : ""
              : "",
        };
      }

      case "VariableDeclaration": {
        if (!trackDeclarations) {
          return;
        }
        const declarations = node.declarations.flatMap((x) => {
          const bindings = getASTBindings(x.id, {
            ignoredIdentifiers: [],
          });

          const items: ScriptItem[] = bindings
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

          if (x.init?.type === "AwaitExpression") {
            isAsync = true;
            items.unshift({
              type: ScriptTypes.Async,
              isAsync: true,
              node: x.init,
            });
          }
          return items;
        });

        return declarations;
      }

      case "ExportDefaultDeclaration": {
        return {
          type: ScriptTypes.Error,
          node: node,
          message: "EXPORT_DEFAULT_SETUP",
          loc: null,
        };
      }

      case "ReturnStatement": {
        // if (isOptions) {
        //   // TODO return bindings from return statement
        //   return [];
        // }
        return {
          type: ScriptTypes.Error,
          node: node,
          message: "NO_RETURN_IN_SETUP",
          loc: null,
        };
      }

      // if any variable is declared in a block or something it shouldn't be tracked
      case "ForInStatement":
      case "ForOfStatement":
      case "ForStatement":
      case "WhileStatement":
      case "DoWhileStatement":
      case "IfStatement":
      case "SwitchStatement":
      case "TryStatement":
      case "BlockStatement": {
        trackDeclarations = false;
        return;
      }
    }
    if (ignoreNodeTypes.has(node.type)) {
      ignore = true;
    }
  }

  function leave(
    node: VerterASTNode,
    parent: VerterASTNode | null,
    key?: string
  ) {
    if (ignoreNodeTypes.has(node.type)) {
      ignore = false;
    }
    switch (node.type) {
      case "ForInStatement":
      case "ForOfStatement":
      case "ForStatement":
      case "WhileStatement":
      case "DoWhileStatement":
      case "IfStatement":
      case "SwitchStatement":
      case "TryStatement":
      case "BlockStatement": {
        trackDeclarations = true;
        break;
      }
    }
  }

  return {
    visit,
    leave,

    get isAsync() {
      return isAsync;
    },
  };
}
