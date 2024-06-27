import { NodeTypes } from "@vue/compiler-core";
import { createTranspiler } from "../../utils.js";

export default createTranspiler(NodeTypes.TEXT, {
  enter(node, _, { s }) {
    const content = node.content.trim();
    // not handle <, because it can be just the start of the tag
    if (content === "<") {
    } else if (content) {
      s.overwrite(
        node.loc.start.offset,
        node.loc.end.offset,
        `{ ${JSON.stringify(content)} }`
      );
    }
  },
});
