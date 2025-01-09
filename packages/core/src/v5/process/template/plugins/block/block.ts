import { declareTemplatePlugin } from "../../template";
import { Node } from "@vue/compiler-core";

export const BlockPlugin = declareTemplatePlugin({
  name: "VerterBlock",
  conditions: new Map<Node, Node[]>(),

  addItem(element: Node, parent: Node) {
    let parentBlock = this.conditions.get(parent);
    if (parentBlock) {
      parentBlock.push(element);
    } else {
      this.conditions.set(parent, [element]);
    }
  },

  pre() {
    this.conditions.clear();
  },

  post(s) {
    for (const [_, block] of this.conditions) {
      const first = block.shift()!;
      const last = block.pop() ?? first;

      s.prependLeft(first.loc.start.offset, "{()=>{");
      s.prependLeft(last.loc.end.offset, "}}");
    }
  },

  transformCondition(item) {
    this.addItem(item.element, item.parent);
  },

  transformLoop(item) {
    this.addItem(item.element, item.parent);
  },

  transformSlotDeclaration(item) {
    this.addItem(item.node, item.parent);
  },

  transformSlotRender(item) {
    this.addItem(item.element, item.parent);
  },

  transformElement(item) {
    if (item.tag === "template") {
      this.addItem(item.node, item.parent);
    }
  },
});
