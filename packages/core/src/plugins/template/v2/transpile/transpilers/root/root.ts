import { NodeTypes } from "@vue/compiler-core";
import { createTranspiler } from "../../utils.js";

export default createTranspiler(NodeTypes.ROOT, {
  enter(node, _, { s }) {
    if (node.children.length > 0) {
      const startOffset = node.source.indexOf("<template>");
      const endOffset = node.source.lastIndexOf("</template>");

      s.remove(
        startOffset + node.loc.start.offset + 1,
        startOffset + node.loc.start.offset + 1 + "template".length
      );
      s.remove(
        endOffset + node.loc.start.offset + 2,
        endOffset + node.loc.start.offset + 2 + "template".length
      );
    } else {
      s.replaceAll("template", "");
    }
  },
});
