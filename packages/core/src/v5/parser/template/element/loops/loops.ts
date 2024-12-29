import { DirectiveNode, NodeTypes } from "@vue/compiler-core";
import { VerterNode } from "../../../walk";
import { TemplateLoop, TemplateTypes } from "../../types";
import { retrieveBindings } from "../../utils";

export type LoopsContext = {
  ignoredIdentifiers: string[];
};
export function handleLoopProp<T extends LoopsContext>(
  node: VerterNode,
  parent: VerterNode,
  parentContext: T
): null | {
  context: T;
  loop: TemplateLoop;
  items: any[];
} {
  if (node.type !== NodeTypes.ELEMENT) {
    return null;
  }

  const forProp = node.props.find(
    (x) => x.type === NodeTypes.DIRECTIVE && "forParseResult" in x
  ) as
    | (DirectiveNode & Required<Pick<DirectiveNode, "forParseResult">>)
    | undefined;

  if (!forProp) {
    return null;
  }

  const sourceBindings = forProp.forParseResult.source
    ? retrieveBindings(forProp.forParseResult.source, parentContext)
    : [];
  const keyBindings = forProp.forParseResult.key
    ? retrieveBindings(forProp.forParseResult.key, parentContext)
    : [];
  const valueBindings = forProp.forParseResult.value
    ? retrieveBindings(forProp.forParseResult.value, parentContext)
    : [];
  const indexBindings = forProp.forParseResult.index
    ? retrieveBindings(forProp.forParseResult.index, parentContext)
    : [];

  const items = [];

  const context = {
    ...parentContext,
    ignoredIdentifiers: [
      ...parentContext.ignoredIdentifiers,
      ...valueBindings.map((x) => x.name),
      ...keyBindings.map((x) => x.name),
      ...indexBindings.map((x) => x.name),
    ],
  };

  const loop: TemplateLoop = {
    type: TemplateTypes.Loop,
    node: forProp,
    element: node,
    parent,
  };

  items.push(loop);
  items.push(...sourceBindings);

  return {
    items,

    loop,
    context,
  };
}
