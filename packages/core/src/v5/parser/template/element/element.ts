import { ElementNode, ElementTypes, NodeTypes } from "@vue/compiler-core";
import { handleConditions } from "./conditions";
import {
  TemplateCondition,
  TemplateTypes,
  TemplateElement,
  TemplateBinding,
  TemplateProp,
} from "../types";
import { handleLoopProp } from "./loops";
import { handleProps } from "./props";
import { handleSlotDeclaration, handleSlotProp } from "./slots";
import { VerterNode } from "../../walk";

export type ElementContext = {
  conditions: TemplateCondition[];
  ignoredIdentifiers: string[];
  inFor: boolean;
};

export function handleElement(
  node: VerterNode,
  parent: VerterNode,
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
    (propBindings?.filter(
      (x) => x.type === TemplateTypes.Prop
    ) as TemplateProp[]) ?? ([] as TemplateProp[]);
  const slot = handleSlotDeclaration(node, parent as ElementNode, context);

  const propSlot = handleSlotProp(node, parent, context, conditions?.condition);
  if (propSlot) {
    context = propSlot.context;
  }

  const element: TemplateElement = {
    type: TemplateTypes.Element,
    tag: node.tag,

    node,
    parent,

    ref:
      props?.find((x) => {
        if (x.name === "ref") {
          return true;
        }
        const node = x.node;

        if (node?.type === NodeTypes.DIRECTIVE) {
          return node.rawName === ":ref";
        }
        return false;
      }) ?? null,
    props: props ?? [],

    condition: conditions?.condition ?? null,
    loop: loop?.loop ?? null,
    slot: slot?.slot ?? propSlot?.slot ?? null,

    context,
  };

  const items = [
    ...(conditions?.items ?? []),
    ...(loop?.items ?? []),
    ...(node.tagType === ElementTypes.COMPONENT
      ? [
          {
            type: TemplateTypes.Binding,
            name: node.tag.split(".")[0],
            node,
            isComponent: true,
            directive: null,
            exp: null,
            parent: null,
          } as TemplateBinding,
        ]
      : []),

    element,
    ...(propBindings ?? []),
    ...(propSlot?.items ?? []),
    ...[slot?.slot].filter((x) => x),
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
