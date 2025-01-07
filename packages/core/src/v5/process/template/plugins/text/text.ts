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

    for (let i = 0; i < content.length; i++) {
      if (content[i] === '"') {
        s.overwrite(item.node.loc.start.offset + i, item.node.loc.start.offset + i + 1, '\\"');
      }
    }

    s.appendRight(item.node.loc.start.offset, '{ "');
    s.appendLeft(item.node.loc.end.offset, '" }');
  },
} as TemplatePlugin;
