import { NodeTypes } from "@vue/compiler-core";
import { createTranspiler } from "../../utils";

export default createTranspiler(NodeTypes.ROOT, {
  enter(node, _, { s }) {
    if (node.children.length > 0) {
      // const firstChild = node.children[0];
      // const lastChild = node.children[node.children.length - 1];
      // s.overwrite(node.loc.start.offset, firstChild.loc.start.offset, "<>");
      // s.overwrite(
      //   lastChild.loc.end.offset,
      //   node.loc.end.offset + node.source.length,
      //   "</>"
      // );

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
