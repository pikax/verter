import { ElementTypes, NodeTypes } from "@vue/compiler-core";
import { VerterNode } from "../../../walk";
import { TemplateSlot, TemplateTypes } from "../../types";
import { propToTemplateProp } from "../props";

export type SlotsContext = {
  ignoredIdentifiers: string[];
};
export function handleSlot<T extends SlotsContext>(
  node: VerterNode,
  parentContext: SlotsContext
): {
  context: T;
  slot: TemplateSlot;
  items: any[];
} {
  if (node.type !== NodeTypes.ELEMENT) {
    return null;
  }
  if (node.tagType !== ElementTypes.SLOT) {
    return null;
  }

  const items = [];

  let nameProp = null;

  {
    const prop = node.props.find((prop) =>
      prop.type === NodeTypes.DIRECTIVE
        ? prop.arg.type === NodeTypes.SIMPLE_EXPRESSION &&
          prop.arg.isStatic &&
          prop.arg.content === "name"
        : prop.name === "name"
    );
    if (prop) {
      nameProp = propToTemplateProp(prop, parentContext);
    }
  }

  const [name = null, ...nameBindings] = nameProp || [];

  const slot: TemplateSlot = {
    type: TemplateTypes.Slot,
    node,
    name,
    props: null,
    parent: null,
  };

  const context = {
    ...parentContext,
    ignoredIdentifiers: [...parentContext.ignoredIdentifiers],
  } as T;

  items.push(slot);

  items.push(...nameBindings);

  return {
    slot,

    context,
    items,
  };
}
