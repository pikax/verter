import { NodeTypes } from "@vue/compiler-core";
import { createTranspiler } from "../../utils";
import { processExpression } from "../../utils";

export default createTranspiler(NodeTypes.INTERPOLATION, {
  enter(node, parent, context) {
    // replace {{ }}  with { }
    context.s.overwrite(node.loc.start.offset, node.loc.start.offset + 2, "{");
    context.s.overwrite(node.loc.end.offset - 2, node.loc.end.offset, "}");

    processExpression(node.content, context);
  },
  // leave(node, parent, context) {},
});
