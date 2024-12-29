import { DirectiveNode, NodeTypes } from "@vue/compiler-core";
import { VerterNode } from "../../../walk";
import { TemplateBinding, TemplateCondition, TemplateTypes } from "../../types";
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
  const items = [];

  const bindings: TemplateBinding[] = [];
  const condition: TemplateCondition = {
    type: TemplateTypes.Condition,
    node: prop,

    bindings,
    element: node,
    parent,

    context: parentContext,
  };

  const context = {
    ...parentContext,
    conditions: [...parentContext.conditions, condition],
  };

  bindings.push(...retrieveBindings(prop.exp, context));

  items.push(condition);
  items.push(...bindings);

  return {
    items,

    condition,
    context,
  };
}
