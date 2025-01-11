import { declareTemplatePlugin, TemplateContext } from "../../template";
import { Node } from "@vue/compiler-core";

export const BlockPlugin = declareTemplatePlugin({
  name: "VerterBlock",
  conditions: new Map<Node, Node[]>(),
  contexts: new Map<Node, TemplateContext>(),

  addItem(element: Node, parent: Node, ctx?: TemplateContext) {
    let parentBlock = this.conditions.get(parent);
    if (parentBlock) {
      parentBlock.push(element);
    } else {
      this.conditions.set(parent, [element]);
    }
    if (ctx) this.contexts.set(element, ctx);
  },

  pre() {
    this.conditions.clear();
  },

  post(s, ctx) {
    for (const [parent, block] of this.conditions) {
      const first = block.shift()!;
      const last = block.pop() ?? first;

      s.prependLeft(first.loc.start.offset, "{()=>{");

      const conditions = this.contexts.get(first);

      // TODO this is not 100% correct, since the child might have a v-if
      if (ctx.doNarrow && conditions?.conditions.length) {
        ctx.doNarrow(
          {
            index: first.loc.end.offset,
            inBlock: true,
            conditions: conditions.conditions,
            type: "prepend",
            direction: "left",
            condition: null,
            parent,
          },
          s
        );
      }

      s.prependRight(last.loc.end.offset, "}}");
    }
  },

  transformCondition(item) {
    // slot render have special conditions and places where the v-if should be placed
    if (
      item.element.tag === "template" &&
      item.element.props.find((x) => x.name === "slot")
    ) {
      return;
    }
    this.addItem(item.element, item.parent, item.context);
  },

  transformLoop(item) {
    // this.addItem(item.element, item.parent, item.context);
  },

  transformSlotDeclaration(item) {
    this.addItem(item.node, item.parent);
  },

  transformSlotRender(item) {
    // this.addItem(item.element, item.parent);
  },

  transformElement(item) {
    if (
      item.tag === "template" &&
      !item.node.props.find((x) => x.name === "slot")
    ) {
      this.addItem(item.node, item.parent);
    }
  },
});
