import { TemplatePlugin } from "../../template";

export const CommentPlugin = {
  name: "VerterComment",
  transformComment(item, s) {
    const relativeStart = item.node.loc.source.indexOf(item.content);
    const relativeEnd = relativeStart + item.content.length;

    // replace <!-- with /*
    s.overwrite(
      item.node.loc.start.offset,
      item.node.loc.start.offset + relativeStart,
      "/*"
    );

    // replace --> with */
    s.overwrite(
      item.node.loc.start.offset + relativeEnd,
      item.node.loc.end.offset,
      "*/"
    );
  },
} as TemplatePlugin;
