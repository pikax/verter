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
  ObjectProperty,
  Function,
  ArrowFunctionExpression,
  ObjectExpression,
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
  | (ParsedScriptItem<ObjectProperty> & { return: true })
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

export type OptionsAPINamedNode<T extends VerterASTNode = VerterASTNode> = {
  name: string;
  node: T;
};
export type ParseScriptOptionsAPI = ParsedScriptItem & {
  component: {
    node: VerterASTNode;
    name: OptionsAPINamedNode<ObjectProperty> | null;

    wrapper: OptionsAPINamedNode | null;

    props: OptionsAPINamedNode[];
    slots: OptionsAPINamedNode[];
    computed: OptionsAPINamedNode[];
    data: OptionsAPINamedNode[];
    methods: OptionsAPINamedNode[];
  };
};

type ParseScriptItemAll =
  | ParseScriptDeclaration
  | ParseScriptCall
  | ParseScriptExport
  | ParsedScriptItem
  | (ParsedScriptItem & {
      parent: ImportDeclaration;
    })
  | ParseScriptOptionsAPI;

export interface ParseScriptResult {
  isAsync: boolean;

  optionsComponent: ParseScriptOptionsAPI | null;

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

  let optionsComponent: ParseScriptResult["optionsComponent"] = null;

  function onShallowWalk(node: VerterASTNode, optionsAPI?: boolean) {
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
        const r = processExportDefault(node.declaration, source, (node) => {
          isAsync = node.async;
          onShallowWalk(node);
          shallowWalk(node, (v) => onShallowWalk(v, true));
        });

        optionsComponent = {
          node: node,
          content: source.slice(node.start, node.end),
          component: r,
        };

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

        break;
      }

      case "ReturnStatement": {
        // TODO check if is optionsAPI
        if (node.argument?.type === "ObjectExpression") {
          for (let i = 0; i < node.argument.properties.length; i++) {
            const prop = node.argument.properties[i];
            if (prop.type === "SpreadElement") {
              continue;
            }

            declarations.push({
              return: true,

              content: source.slice(prop.start, prop.end),
              node: prop,
              name:
                prop.key.type === "Identifier"
                  ? prop.key.name
                  : // TODO test this
                    source.slice(prop.key.start, prop.key.end),
            });
          }
        }
      }
    }
  }

  shallowWalk(ast, onShallowWalk);

  return {
    isAsync,

    optionsComponent,

    declarations,

    imports,
    calls,
    exports,
  };
}

function processExportDefault(
  node: VerterASTNode,
  source: string,
  setupWalk: (node: Function | ArrowFunctionExpression) => void
) {
  let name: OptionsAPINamedNode | null = null;
  let wrapper: OptionsAPINamedNode | null = null;

  const props = [] as OptionsAPINamedNode[];
  const slots = [] as OptionsAPINamedNode[];
  const computed = [] as OptionsAPINamedNode[];
  const data = [] as OptionsAPINamedNode[];
  const methods = [] as OptionsAPINamedNode[];

  switch (node.type) {
    case "ObjectExpression": {
      for (const objProp of node.properties) {
        if (objProp.type === "SpreadElement") continue;
        const propName =
          objProp.key.type === "Identifier" ? objProp.key.name : "";

        switch (propName) {
          case "name": {
            name = {
              name:
                objProp.value.type === "Literal"
                  ? `${objProp.value.value}`
                  : "",
              node: objProp,
            };
            break;
          }

          case "props": {
            if (objProp.value.type === "ObjectExpression") {
              for (const propProp of objProp.value.properties) {
                if (propProp.type === "SpreadElement") continue;
                props.push({
                  name:
                    propProp.key.type === "Identifier" ? propProp.key.name : "",
                  node: propProp,
                });
              }
            }
            break;
          }

          case "slots": {
            // todo later
            break;
          }

          case "data": {
            if (
              objProp.value.type === "FunctionExpression" ||
              objProp.value.type === "ArrowFunctionExpression"
            ) {
              const body = objProp.value.body;

              let objectExpressionNode: VerterASTNode | null = null;
              if (body?.statements) {
                if (
                  body.statements.length === 1 &&
                  body.statements[0].type === "ExpressionStatement"
                ) {
                  const expression = body.statements[0].expression;
                  if (expression.type === "ObjectExpression") {
                    objectExpressionNode = expression;
                  } else if (expression.type === "ParenthesizedExpression") {
                    if (expression.expression.type === "ObjectExpression") {
                      objectExpressionNode = expression.expression;
                    }
                  }
                }
                if (!objectExpressionNode) {
                  objectExpressionNode =
                    body.statements.find(
                      (x) =>
                        x.type === "ReturnStatement" &&
                        x.argument?.type === "ObjectExpression"
                    ) ?? null;
                }
              }

              if (objectExpressionNode?.type === "ObjectExpression") {
                for (const dataProp of objectExpressionNode.properties) {
                  if (dataProp.type === "SpreadElement") continue;
                  data.push({
                    name:
                      dataProp.key.type === "Identifier"
                        ? dataProp.key.name
                        : "",
                    node: dataProp,
                  });
                }
              }
            }

            // TODO add a warning!!
            if (objProp.value.type === "ObjectExpression") {
              for (const dataProp of objProp.value.properties) {
                if (dataProp.type === "SpreadElement") continue;
                data.push({
                  name:
                    dataProp.key.type === "Identifier" ? dataProp.key.name : "",
                  node: dataProp,

                  warning: "data should be a function",
                });
              }
            }
            break;
          }

          case "methods": {
            if (objProp.value.type === "ObjectExpression") {
              for (const methodProp of objProp.value.properties) {
                if (methodProp.type === "SpreadElement") continue;
                methods.push({
                  name:
                    methodProp.key.type === "Identifier"
                      ? methodProp.key.name
                      : "",
                  node: methodProp,
                });
              }
            }
            break;
          }

          case "computed": {
            if (objProp.value.type === "ObjectExpression") {
              for (const computedProp of objProp.value.properties) {
                if (computedProp.type === "SpreadElement") continue;
                computed.push({
                  name:
                    computedProp.key.type === "Identifier"
                      ? computedProp.key.name
                      : "",
                  node: computedProp,
                });
              }
            }
            break;
          }

          case "setup": {
            if (
              objProp.value.type === "FunctionExpression" ||
              objProp.value.type === "ArrowFunctionExpression"
            ) {
              setupWalk(objProp.value);
            }
            break;
          }
        }
      }

      break;
    }

    case "FunctionExpression": {
      wrapper = {
        name: node.id ? node.id.name : "",
        node: node,
      };
      if (node.body) {
        return processExportDefault(node.body, source, setupWalk);
      }
      break;
    }
  }

  return {
    node: node as ObjectExpression,
    name,
    wrapper,
    props,
    slots,
    computed,
    data,
    methods,
  } as ParseScriptOptionsAPI["component"];
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
