import { VerterASTNode } from "../ast";
import { ScriptDeclaration, ScriptTypes } from "./types";

export function retrieveDeclarations(
  node: VerterASTNode,
  parent: VerterASTNode,
  overrideNode: VerterASTNode = node
): ScriptDeclaration[] {
  const items: ScriptDeclaration[] = [];
  switch (node.type) {
    case "Identifier": {
      const name = node.name;
      items.push({
        type: ScriptTypes.Declaration,
        name,
        node: overrideNode,
        parent,
        declarator: parent,
        rest: false,
      });
      break;
    }
    case "RestElement": {
      items.push({
        type: ScriptTypes.Declaration,
        name: null,
        node: overrideNode,
        parent,
        declarator: parent,
        rest: node,
      });
      break;
    }
    case "ObjectPattern": {
      for (const prop of node.properties) {
        if (
          // @ts-expect-error only for acorn
          prop.type === "Property" ||
          prop.type === "BindingProperty"
        ) {
          if (prop.key === prop.value) {
            items.push(...retrieveDeclarations(prop.key, parent, prop));
          } else {
            items.push(...retrieveDeclarations(prop.value, parent, prop));
          }
        } else {
          items.push(...retrieveDeclarations(prop, parent, prop));
        }
      }
      break;
    }
    case "ArrayPattern": {
      for (const element of node.elements) {
        if (element) {
          items.push(...retrieveDeclarations(element, parent, element));
        }
      }
      break;
    }
    case "AssignmentPattern": {
      items.push(...retrieveDeclarations(node.left, parent, node));
      break;
    }
  }

  return items;
}
