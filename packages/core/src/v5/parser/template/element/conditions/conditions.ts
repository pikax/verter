import { DirectiveNode, NodeTypes } from "@vue/compiler-core";
import { VerterNode } from "../../../walk";
import {
  TemplateBinding,
  TemplateCondition,
  TemplateItem,
  TemplateTypes,
} from "../../types";
import { retrieveBindings } from "../../utils";

export type ConditionsContext = {
  conditions: TemplateCondition[];
  ignoredIdentifiers: string[];
};

export function handleConditions<T extends ConditionsContext>(
  node: VerterNode,
  parent: VerterNode,
  parentContext: T
): null | {
  context: T;
  condition: TemplateCondition;
  items: any[];
} {
  if (node.type !== NodeTypes.ELEMENT) {
    return null;
  }

  const prop = node.props.find(
    (prop) =>
      prop.type === NodeTypes.DIRECTIVE &&
      (prop.name === "if" || prop.name === "else-if" || prop.name === "else")
  ) as DirectiveNode;

  if (!prop) {
    return null;
  }
  const items: TemplateItem[] = [];

  const bindings: TemplateBinding[] = [];
  const siblings: TemplateCondition[] = [];

  if (prop.name !== "if") {
    const children = "children" in parent ? parent.children : [];
    for (let i = 0; i < children.length; i++) {
      const element = children[i];
      if (element === node) {
        break;
      }
      const condition = handleConditions(element as any, parent, parentContext);
      if (condition) {
        siblings.push(condition.condition);
      }
    }
  }

  const condition: TemplateCondition = {
    type: TemplateTypes.Condition,
    node: prop,

    bindings,
    element: node,
    parent,

    context: parentContext,

    siblings,
  };

  const context = {
    ...parentContext,
    conditions: [...parentContext.conditions, condition],
  };

  items.push(condition);
  if (prop.exp) {
    const all = retrieveBindings(prop.exp, context);
    bindings.push(...all.filter((x) => x.type === TemplateTypes.Binding));
    items.push(...all);
  }

  return {
    items,

    condition,
    context,
  };
}
