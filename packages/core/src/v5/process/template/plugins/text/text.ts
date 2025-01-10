import { TemplatePlugin } from "../../template";

export const TextPlugin = {
  name: "VerterText",
  transformText(item, s) {
    const content = item.content.trim();
    if(!content) {
      return
    }

    // ignore <, because it can be just the start of the tag
    // NOTE maybe we could recognise this in Language Server and gracefully
    // point to the empty string
    if (content === "<") {
      return;
    }

    // should replace " with \"

    for (let i = 0; i < content.length; i++) {
      if (content[i] === '"') {
        s.overwrite(
          item.node.loc.start.offset + i,
          item.node.loc.start.offset + i + 1,
          '\\"'
        );
      }
    }

    const start =
      item.node.loc.source.indexOf(content) + item.node.loc.start.offset;

    const end = start + content.length;

    s.prependRight(start, '{"');
    s.prependLeft(end, '"}');

    // s.appendRight(item.node.loc.start.offset, '{ "');
    // s.appendLeft(item.node.loc.end.offset, '" }');

    // s.prependRight(item.node.loc.start.offset, '{ "');
    // s.prependLeft(item.node.loc.end.offset, '" }');
  },
} as TemplatePlugin;
