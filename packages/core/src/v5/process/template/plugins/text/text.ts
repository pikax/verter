import { TemplatePlugin } from "../../template";

export const TextPlugin = {
  name: "VerterText",
  transformText(item, s) {
    const content = item.content.trim();

    // ignore <, because it can be just the start of the tag
    // NOTE maybe we could recognise this in Language Server and gracefully
    // point to the empty string
    if (content === "<") {
      return;
    }

    // should replace " with \"
    s.replace(/\"/g, '\\"');

    s.appendRight(item.node.loc.start.offset, '{ "');
    s.appendLeft(item.node.loc.end.offset, '" }');
  },
} as TemplatePlugin;
