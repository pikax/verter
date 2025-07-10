import { DirectiveNode, NodeTypes } from "@vue/compiler-core";
import { VerterNode } from "../../../walk";
import { TemplateLoop, TemplateTypes } from "../../types";
import { retrieveBindings } from "../../utils";

export type LoopsContext = {
  ignoredIdentifiers: string[];
  inFor: boolean;
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

  const toAddIgnoredIdentifiers = [
    ...valueBindings
      .filter((x) => x.type === TemplateTypes.Binding)
      .map((x) => x.name),
    ...keyBindings
      .filter((x) => x.type === TemplateTypes.Binding)
      .map((x) => x.name),
    ...indexBindings
      .filter((x) => x.type === TemplateTypes.Binding)
      .map((x) => x.name),
  ];

  const context =
    toAddIgnoredIdentifiers.length > 0
      ? {
          ...parentContext,
          ignoredIdentifiers: [
            ...parentContext.ignoredIdentifiers,
            ...toAddIgnoredIdentifiers,
          ],
          inFor: true,
        }
      : {
          ...parentContext,
          inFor: true,
        };

  const loop: TemplateLoop = {
    type: TemplateTypes.Loop,
    node: forProp,
    element: node,
    parent,
    context: {
      ...parentContext,
      // blockDirection: "Right",
    },
  };

  items.push(loop);
  items.push(...sourceBindings);

  return {
    items,

    loop,
    context,
  };
}
