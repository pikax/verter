export type ElementContext = {
  conditions: string[];
  inFor: boolean;
  ignoredIdentifiers: string[];
};

export function handleElement(node: Element, context: ParserContext) {
  // ...
}
