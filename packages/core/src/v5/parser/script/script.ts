import type {
  VerterAST,
  VerterASTNode,
  VariableDeclarator,
  VariableDeclaration,
  BindingPattern,
  FunctionDeclaration,
  ClassDeclaration,
  ExpressionStatement,
  TSTypeAliasDeclaration,
  ImportDeclaration,
  ImportDeclarationSpecifier,
  CallExpression,
  ExportNamedDeclaration,
  ExportDefaultDeclarationKind,
  ExportAllDeclaration,
  ExportDefaultDeclaration,
  ExportSpecifier,
} from "../ast/index.js";
import { shallowWalk } from "../walk/index.js";

export type ParsedScriptItem<T extends VerterASTNode = VerterASTNode> = {
  node: T;
  content: string;
};

export type ParseScriptDeclaration = (
  | (ParsedScriptItem<VariableDeclarator> & {
      parent: VariableDeclaration;
    })
  | (ParsedScriptItem<BindingPattern> & {
      parent: VariableDeclaration;
    })
  | ParsedScriptItem<
      FunctionDeclaration | ClassDeclaration | ExpressionStatement
    >
  | ParsedScriptItem<TSTypeAliasDeclaration>
) & { name: string };

export type ParseScriptCall = ParsedScriptItem<CallExpression> & {
  name: string;
  parent: ExpressionStatement;
};

export type ParseScriptExport =
  | ((
      | ParsedScriptItem<VariableDeclarator>
      | ParsedScriptItem<BindingPattern>
      | ParsedScriptItem<
          FunctionDeclaration | ClassDeclaration | ExpressionStatement
        >
      | ParsedScriptItem<TSTypeAliasDeclaration>
      | ParsedScriptItem<ExportSpecifier>
    ) & {
      parent: ExportNamedDeclaration;
      name: string;
      default: false;
    })
  | (ParsedScriptItem<ExportDefaultDeclarationKind> & {
      parent: ExportDefaultDeclaration;
      name: "default";
      default: true;
    })
  | (ParsedScriptItem<ExportAllDeclaration> & {
      name: "*";
      default: false;
    });

export interface ParseScriptResult {
  isAsync: boolean;

  declarations: Array<ParseScriptDeclaration>;
  calls: Array<ParseScriptCall>;

  imports: Array<
    ParsedScriptItem<ImportDeclarationSpecifier> & {
      parent: ImportDeclaration;
    }
  >;
  exports: Array<ParseScriptExport>;
}

export function parseScript(ast: VerterAST, source: string) {
  let isAsync = false;

  const declarations: ParseScriptResult["declarations"] = [];
  const imports: ParseScriptResult["imports"] = [];
  const calls: ParseScriptResult["calls"] = [];
  const exports: ParseScriptResult["exports"] = [];

  function onWalk(node: VerterASTNode) {
    switch (node.type) {
      case "ExpressionStatement": {
        const expression = node.expression;
        if (expression) {
          switch (expression.type) {
            case "AwaitExpression": {
              isAsync = true;
              break;
            }
            case "CallExpression": {
              const callee = expression.callee;
              if (callee.type === "Identifier") {
                calls.push({
                  content: source.slice(node.start, node.end),
                  parent: node,
                  node: expression,
                  name: callee.name,
                });
              }
              break;
            }
          }
        }
        break;
      }
      case "ClassDeclaration":
      case "FunctionDeclaration": {
        declarations.push({
          content: source.slice(node.start, node.end),
          node,
          name: node.id
            ? node.id.type === "Identifier"
              ? node.id.name
              : // TODO test this
                source.slice(node.id.start, node.id.end)
            : "",
        });
        break;
      }
      case "VariableDeclaration": {
        if (node.declarations) {
          for (let i = 0; i < node.declarations.length; i++) {
            const n = node.declarations[i];

            switch (n.id.type) {
              case "Identifier": {
                declarations.push({
                  parent: node,
                  node: n,
                  content: source.slice(n.start, n.end),
                  name: n.id.name,
                });
                break;
              }
              case "ArrayPattern":
              case "ObjectPattern": {
                // @ts-expect-error not 100% correct type
                declarations.push(...processPattern(n.id, source, node));
                break;
              }
            }
          }
        }
        break;
      }
      case "ImportDeclaration": {
        if (node.specifiers) {
          for (let i = 0; i < node.specifiers.length; i++) {
            const n = node.specifiers[i];
            switch (n.type) {
              case "ImportSpecifier":
              case "ImportDefaultSpecifier":
              case "ImportNamespaceSpecifier": {
                const name = n.local.name;
                const content = source.slice(n.start, n.end);
                const item = { node: n, content, name, parent: node };
                imports.push(item);
                break;
              }
            }
          }
        }
        break;
      }
      case "TSTypeAliasDeclaration": {
        declarations.push({
          content: source.slice(node.start, node.end),
          node,
          name:
            node.id.type === "Identifier"
              ? node.id.name
              : // TODO test this
                source.slice(node.id.start, node.id.end),
        });
        break;
      }

      case "ExportDefaultDeclaration": {
        exports.push({
          content: source.slice(node.declaration.start, node.declaration.end),
          node: node.declaration,
          name: "default",
          parent: node,
          default: true,
        });
        break;
      }
      case "ExportAllDeclaration": {
        exports.push({
          content: source.slice(node.start, node.end),
          node,
          name: "*",
          default: false,
        });
        break;
      }
      case "ExportNamedDeclaration": {
        const declaration = node.declaration;
        if (declaration) {
          switch (declaration.type) {
            case "VariableDeclaration": {
              for (let i = 0; i < declaration.declarations.length; i++) {
                const n = declaration.declarations[i];
                switch (n.id.type) {
                  case "Identifier": {
                    exports.push({
                      parent: node,
                      node: n,
                      content: source.slice(n.start, n.end),
                      name: n.id.name,
                      default: false,
                    });
                    break;
                  }
                  case "ArrayPattern":
                  case "ObjectPattern": {
                    // @ts-expect-error not 100% correct type
                    exports.push(...processPattern(n.id, source, node));
                    break;
                  }
                }
              }

              break;
            }
            case "FunctionDeclaration":
            case "ClassDeclaration": {
              exports.push({
                content: source.slice(declaration.start, declaration.end),
                node: declaration,
                name: declaration.id
                  ? declaration.id.type === "Identifier"
                    ? declaration.id.name
                    : // TODO test this
                      source.slice(declaration.id.start, declaration.id.end)
                  : "",
                parent: node,
                default: false,
              });
              break;
            }
            case "TSTypeAliasDeclaration": {
              exports.push({
                content: source.slice(declaration.start, declaration.end),
                node: declaration,
                name:
                  declaration.id.type === "Identifier"
                    ? declaration.id.name
                    : // TODO test this
                      source.slice(declaration.id.start, declaration.id.end),
                parent: node,
                default: false,
              });
              break;
            }
          }
        } else if (node.specifiers) {
          for (let i = 0; i < node.specifiers.length; i++) {
            const n = node.specifiers[i];
            switch (n.type) {
              case "ExportSpecifier": {
                exports.push({
                  content: source.slice(n.start, n.end),
                  node: n,
                  name:
                    n.exported.type === "Identifier"
                      ? n.exported.name
                      : // TODO test this
                        source.slice(n.exported.start, n.exported.end),
                  parent: node,
                  default: false,
                });
                break;
              }
            }
          }
        }
      }
    }
  }

  shallowWalk(ast, onWalk);

  return {
    isAsync,

    declarations,

    imports,
    calls,
    exports,
  };
}

function* processPattern<T extends VerterASTNode, P extends VerterASTNode>(
  node: T,
  source: string,
  parent: VerterASTNode,
  overrideNode: P = node as unknown as P
): Generator<ParseScriptDeclaration | ParseScriptExport> {
  switch (node.type) {
    case "Identifier": {
      const content = source.slice(overrideNode.start, overrideNode.end);
      const name = node.name;
      // @ts-expect-error not 100% correct type
      yield { content, name, node: overrideNode, parent };
      break;
    }
    case "ObjectPattern": {
      for (let i = 0; i < node.properties.length; i++) {
        const prop = node.properties[i];
        if (
          // @ts-expect-error only for acorn
          prop.type === "Property" ||
          prop.type === "BindingProperty"
        ) {
          if (prop.key === prop.value) {
            yield* processPattern(prop.key, source, parent, prop);
          } else {
            yield* processPattern(prop.value, source, parent, prop);
          }
        } else {
          yield* processPattern(prop, source, parent, prop);
        }
      }
      break;
    }
    case "ArrayPattern": {
      for (let i = 0; i < node.elements.length; i++) {
        const el = node.elements[i];
        if (el) {
          yield* processPattern(el, source, parent, el);
        }
      }
      break;
    }
    case "RestElement": {
      yield* processPattern(node.argument, source, parent, overrideNode);
      break;
    }
    case "AssignmentPattern": {
      // const rightitems = processPattern(
      //   node.right,
      //   source,
      //   parent,
      //   overrideNode
      // );

      // let hasYielded = false;
      // for (const right of rightitems) {
      //   hasYielded = true;
      //   yield right;
      // }
      let hasYielded = false;
      if (!hasYielded) {
        yield* processPattern(node.left, source, parent, overrideNode);
      }
      break;
    }
  }
}
