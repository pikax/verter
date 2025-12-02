import { Expression } from "oxc-parser";
import { MacrosPlugin } from "../../../process/script/plugins/macros";
import {
  FunctionBody,
  FunctionDeclaration,
  ObjectExpression,
  VerterASTNode,
} from "../../ast";
import { shallowWalk } from "../../walk";
import { createSetupContext, handleSetupNode } from "../setup";
import { ScriptItem, ScriptTypes } from "../types";

export function handleOptionsNode(
  node: VerterASTNode
): ScriptItem[] | { items: ScriptItem[]; isAsync: boolean } {
  switch (node.type) {
    case "ExportDefaultDeclaration": {
      let objectExpression: ObjectExpression | null = null;
      switch (node.declaration.type) {
        case "CallExpression": {
          if (node.declaration.arguments[0].type === "ObjectExpression") {
            objectExpression = node.declaration.arguments[0];
          }
          break;
        }
        case "ObjectExpression": {
          objectExpression = node.declaration;
          break;
        }
      }
      if (objectExpression) {
        return handleObjectDeclaration(objectExpression);
      }
      return [];
    }

    default:
      return [];
  }
}

function handleObjectDeclaration(node: ObjectExpression): {
  items: ScriptItem[];
  isAsync: boolean;
} {
  const items: ScriptItem[] = [];
  let isAsync = false;

  for (const property of node.properties) {
    switch (property.type) {
      case "SpreadElement": {
        break;
      }
      case "Property": {
        const key = property.key;
        const value = property.value;

        if (key.type === "Identifier") {
          switch (key.name) {
            case "setup": {
              if (value.type === "FunctionExpression") {
                isAsync = value.async;

                if (value.body) {
                  const result = handleSetupNode(value.body, undefined, true);
                  if (Array.isArray(result)) {
                    items.push(...result);
                  } else {
                    items.push(...result.items);
                  }
                }
              }

              break;
            }
          }
        }
        // if (property.key.type === "Identifier") {
        //   items.push({
        //     type: ScriptTypes.Declaration,
        //     name: property.key.name,
        //     rest: false,
        //   });
        // }
      }
    }
    //   if (property.type === "ObjectMethod") {
    //     if (property.key.type === "Identifier") {
    //       items.push({
    //         type: ScriptTypes.Function,
    //         name: property.key.name,
    //         node: property,
    //       });
    //     }
    //   }
  }

  return { items, isAsync };
}

export function createOptionsContext(opts: {
  lang: string | "ts" | "tsx" | "js" | "jsx";
  setupCtx: ReturnType<typeof createSetupContext>;
}) {
  let track = false;
  let objectExpression: ObjectExpression | null = null;

  let setupFunction: null | FunctionBody | Expression = null;

  function visit(
    node: VerterASTNode,
    parent: VerterASTNode | null,
    key?: string
  ): void | ScriptItem | ScriptItem[] {
    if (node.type === "ExportDefaultDeclaration") {
      track = true;
      return {
        type: ScriptTypes.DefaultExport,
        node,
      };
    }

    if (!track) return;

    if (setupFunction) {
      return opts.setupCtx.visit(node, parent, key);
    }

    switch (node.type) {
      case "ObjectExpression": {
        if (objectExpression) return;
        objectExpression = node;
        break;
      }
      case "CallExpression": {
        if (!objectExpression) {
          if (node.arguments[0].type === "ObjectExpression") {
            objectExpression = node.arguments[0];
          }
        }
        return;
      }
      case "FunctionExpression": {
        if (!setupFunction && key === "setup") {
          setupFunction = node.body;
        }
        return;
      }
      case "Identifier": {
        if (
          node.name === "setup" &&
          parent?.type === "Property" &&
          (parent.value.type === "FunctionExpression" ||
            parent.value.type === "ArrowFunctionExpression")
        ) {
          setupFunction = parent.value.body;
        }
        return;
      }
    }
  }

  function leave(
    node: VerterASTNode,
    parent: VerterASTNode | null,
    key?: string
  ): void {
    switch (node.type) {
      case "ExportDefaultDeclaration": {
        track = false;
        break;
      }
      case "FunctionExpression": {
        if (node === setupFunction) {
          setupFunction = null;
        }
        break;
      }
    }

    if (setupFunction) {
      opts.setupCtx.leave(node, parent, key);
    }
    // if (node.type === "ObjectExpression") {
    //   return handleObjectDeclaration(node);
    // }
  }

  return {
    visit,
    leave,
  };
}
