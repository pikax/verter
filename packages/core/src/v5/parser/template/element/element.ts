import { ElementNode, NodeTypes } from "@vue/compiler-core";
import { handleConditions } from "./conditions";
import { TemplateCondition, TemplateTypes, TemplateElement } from "../types";
import { handleLoopProp } from "./loops";
import { handleProps } from "./props";
import { handleSlotDeclaration } from "./slots";

export type ElementContext = {
  conditions: TemplateCondition[];
  inFor: boolean;
  ignoredIdentifiers: string[];
};

export function handleElement(
  node: ElementNode,
  parent: ElementNode,
  parentContext: ElementContext
) {
  if (node.type !== NodeTypes.ELEMENT) {
    return null;
  }

  let context = parentContext;

  const conditions = handleConditions(node, parent, context);
  if (conditions) {
    context = conditions.context;
  }

  const loop = handleLoopProp(node, parent, context);
  if (loop) {
    context = loop.context;
  }

  const propBindings = handleProps(node, context);
  const props =
    propBindings?.filter((x) => x.type === TemplateTypes.Prop) ?? [];
  const slot = handleSlotDeclaration(node, context);

  const element: TemplateElement = {
    type: TemplateTypes.Element,
    tag: node.tag,

    node,
    parent,

    ref: props?.find((x) => x.name === "ref") ?? null,
    props: props ?? [],

    condition: conditions?.condition ?? null,
    loop: loop?.loop ?? null,
    slot: slot?.slot ?? null,
  };

  const items = [
    ...(conditions?.items ?? []),
    ...(loop?.items ?? []),
    element,
    ...(propBindings ?? []),
    ...(slot?.items ?? []),
  ];

  return {
    element,
    context,

    items,

    conditions,
    loop,
    props,
    slot,
  };
}
