import { MacrosPlugin } from "../../../process/script/plugins/macros";
import { ObjectExpression, VerterASTNode } from "../../ast";
import { shallowWalk } from "../../walk";
import { handleSetupNode } from "../setup";
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
                  // walk the setup body to find any declarations
                  shallowWalk(value.body, (node) => {
                    const result = handleSetupNode(node, true);
                    if (Array.isArray(result)) {
                      items.push(...result);
                    } else {
                      items.push(...result.items);
                    }
                  });
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
