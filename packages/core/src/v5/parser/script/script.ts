import type {
  Node,
  ImportSpecifier,
  ImportDefaultSpecifier,
  ImportNamespaceSpecifier,
  VariableDeclaration,
  FunctionDeclaration,
  ClassDeclaration,
  //   TsTypeAliasDeclaration,
  ExpressionStatement,
  ExportSpecifier,
  ExportAllDeclaration,
  ExportDefaultDeclaration,
  ExportNamedDeclaration,
  FunctionExpression,
  VariableDeclarator,
  CallExpression,
  AssignmentProperty,
  ObjectPattern,
  RestElement,
  Pattern,
  AnyNode,
  ArrayPattern,
  Identifier,
} from "acorn";
import type { VerterAST } from "../ast/index.js";
import { shallowWalk } from "../walk/index.js";
import type { VerterASTNode } from "../ast/ast.js";

export type ParsedScriptItem<T extends VerterASTNode = VerterASTNode> = {
  node: T;
  content: string;
};

export type ParseScriptDeclaration = (
  | (ParsedScriptItem<VariableDeclarator> & {
      parent: VariableDeclaration;
    })
  | (ParsedScriptItem<AssignmentProperty | RestElement> & {
      parent: VariableDeclaration;
    })
  | ParsedScriptItem<
      FunctionDeclaration | ClassDeclaration | ExpressionStatement
    >
) & { name: string };

export interface ParseScriptResult {
  isAsync: boolean;

  declarations: Array<ParseScriptDeclaration>;

  imports: Array<
    ParsedScriptItem<
      ImportSpecifier | ImportDefaultSpecifier | ImportNamespaceSpecifier
    >
  >;
  exports: Array<
    ParsedScriptItem<
      | ExportSpecifier
      | ExportAllDeclaration
      | ExportDefaultDeclaration
      | ExportNamedDeclaration
    >
  >;
  macros: Array<ParsedScriptItem<VariableDeclaration | FunctionExpression>>;
}

export function parseScript(ast: VerterAST, source: string) {
  let isAsync = false;

  const declarations: ParseScriptResult["declarations"] = [];
  const imports: ParseScriptResult["imports"] = [];

  function onWalk(node: AnyNode) {
    switch (node.type) {
      case "ExpressionStatement": {
        if (node.expression) {
          switch (node.expression.type) {
            case "AwaitExpression": {
              isAsync = true;
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
          name:
            node.id.type === "Identifier"
              ? node.id.name
              : // TODO test this
                source.slice(node.id.start, node.id.end),
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
                declarations.push(...processPattern(n.id, source, node));
                break;
              }
            }
          }
        }
        break;
      }
      case "ImportDeclaration": {
        for (let i = 0; i < node.specifiers.length; i++) {
          const n = node.specifiers[i];
          switch (n.type) {
            case "ImportSpecifier":
            case "ImportDefaultSpecifier":
            case "ImportNamespaceSpecifier": {
              const name = n.local.name;
              const content = source.slice(n.start, n.end);
              const item = { node: n, content, name };
              imports.push(item);
              break;
            }
          }
        }

        break;
      }
    }
  }

  shallowWalk(ast, onWalk);

  return {
    isAsync,

    declarations,

    imports,
  };
}

function* processPattern(
  node: AnyNode,
  source: string,
  parent: AnyNode,
  overrideNode: AnyNode = node
): Generator<ParseScriptDeclaration> {
  switch (node.type) {
    case "Identifier": {
      const content = source.slice(overrideNode.start, overrideNode.end);
      const name = node.name;
      yield { content, name, node: overrideNode, parent };
      break;
    }
    case "ObjectPattern": {
      for (let i = 0; i < node.properties.length; i++) {
        const prop = node.properties[i];
        if (prop.type === "Property" || prop.type === "BindingProperty") {
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
        yield* processPattern(el, source, parent, el);
      }
      break;
    }
    case "RestElement": {
      yield* processPattern(node.argument, source, parent, overrideNode);
      break;
    }
    case "AssignmentPattern": {
      const rightitems = processPattern(
        node.right,
        source,
        parent,
        overrideNode
      );

      let hasYielded = false;
      for (const right of rightitems) {
        hasYielded = true;
        yield right;
      }
      if (!hasYielded) {
        yield* processPattern(node.left, source, parent, overrideNode);
      }
      break;
    }
  }
}
