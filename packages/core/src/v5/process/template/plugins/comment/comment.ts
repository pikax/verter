import { TemplatePlugin } from "../../template";

export const CommentPlugin = {
  name: "VerterComment",
  transformComment(item, s) {
    const relativeStart = item.node.loc.source.indexOf(item.content);
    const relativeEnd = relativeStart + item.content.length;

    const wrap =
      item.content.indexOf(">") >= 0 ? true : item.content.indexOf("<") >= 0;

    // replace <!-- with /*
    s.overwrite(
      item.node.loc.start.offset,
      item.node.loc.start.offset + relativeStart,
      `${wrap ? "{" : ""}/*`
    );

    // replace --> with */
    s.overwrite(
      item.node.loc.start.offset + relativeEnd,
      item.node.loc.end.offset,
      `*/${wrap ? "}" : ""}`
    );
  },
} as TemplatePlugin;
