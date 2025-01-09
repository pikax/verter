import {
  DirectiveNode,
  ElementNode,
  ElementTypes,
  NodeTypes,
} from "@vue/compiler-core";
import { VerterNode } from "../../../walk";
import {
  TemplateDirective,
  TemplateProp,
  TemplateRenderSlot,
  TemplateSlot,
  TemplateTypes,
} from "../../types";
import { handleProps, propToTemplateProp } from "../props";

export type SlotsContext = {
  ignoredIdentifiers: string[];
};

/**
 * Handles slot declaration <slot/>
 * @param node
 * @param parentContext
 * @returns
 */
export function handleSlotDeclaration<T extends SlotsContext>(
  node: VerterNode,
  parent: ElementNode,
  parentContext: SlotsContext
): {
  context: T;
  slot: TemplateSlot;
  items: any[];
} | null {
  if (node.type !== NodeTypes.ELEMENT) {
    return null;
  }
  if (node.tagType !== ElementTypes.SLOT) {
    return null;
  }

  const items = [];

  const propItems = handleProps(node, parentContext) ?? [];
  const props = propItems.filter(
    (x) => x.type === TemplateTypes.Prop
  ) as TemplateProp[];

  const name =
    (propItems.find((prop) => (prop as any).name === "name") as
      | TemplateProp
      | undefined) ?? null;
  // const name =
  //   (props.find((prop) => prop.name === "name") as TemplateProp | undefined) ??
  //   propItems.find(
  //     (x) => x.type === TemplateTypes.Binding && x.name === "name"
  //   ) ??
  //   null;

  const slot: TemplateSlot = {
    type: TemplateTypes.SlotDeclaration,
    node,
    name,
    props,
    parent,
  };

  const context = {
    ...parentContext,
    ignoredIdentifiers: [...parentContext.ignoredIdentifiers],
  } as T;

  items.push(slot);

  items.push(...propItems);

  return {
    slot,

    context,
    items,
  };
}

/**
 * Handles v-slot directive
 * @param node
 * @param parent
 * @param parentContext
 * @returns
 */
export function handleSlotProp<T extends SlotsContext>(
  node: VerterNode,
  parent: VerterNode,
  parentContext: T
): {
  context: T;
  slot: TemplateRenderSlot;
  items: any[];
} | null {
  if (node.type !== NodeTypes.ELEMENT) {
    return null;
  }

  const propDirective = node.props.find(
    (x) => x.type === NodeTypes.DIRECTIVE && x.name === "slot"
  ) as DirectiveNode | undefined;

  if (!propDirective) {
    return null;
  }

  const [prop] = propToTemplateProp(propDirective, parentContext) as [
    TemplateDirective
  ];

  const context = {
    ...parentContext,
    ignoredIdentifiers: [
      ...parentContext.ignoredIdentifiers,
      ...(prop.exp?.map((x) => x.type === TemplateTypes.Binding && x.name) ??
        []),
    ],
  };

  const slot: TemplateRenderSlot = {
    type: TemplateTypes.SlotRender,
    prop,
    parent,
    element: node,
    name: prop.arg,
  };

  const items: any[] = [slot];

  if (prop.type === TemplateTypes.Directive) {
    if (prop.arg && prop.arg.length) {
      // if there's any bindings
      const b = prop.arg.filter(
        (x) => x.type === TemplateTypes.Binding && !x.ignore
      );
      items.push(...b);
    }
  }

  return {
    slot,
    items,
    context,
  };
}
