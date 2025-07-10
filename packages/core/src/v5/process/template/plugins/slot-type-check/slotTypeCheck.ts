import { ElementNode, ElementTypes, NodeTypes } from "@vue/compiler-core";
import { declareTemplatePlugin } from "../../template";

interface SlotTypeCheckItem {
  node: ElementNode;
  slotName: string;

  children: ElementNode[];
}

export const SlotTypeCheckPlugin = declareTemplatePlugin({
  name: "VerterSlotTypeCheck",

  items: [] as SlotTypeCheckItem[],

  pre() {
    this.items.length = 0;
  },
  post() {
    for (const item of this.items) {
    }
  },

  transformElement(element, s, ctx) {
    // TODO it would be better to just relace and mutate MagicString
    // to maintain the position of the element

    const node = element.node;
    switch (node.tagType) {
      case ElementTypes.SLOT:
      case ElementTypes.TEMPLATE:
        break;
      case ElementTypes.ELEMENT: {
        const slotProp = node.props.find(
          (x) => x.type === NodeTypes.DIRECTIVE && x.name === "slot"
        );

        if (node.children.length === 0) {
          return;
        }

        if (slotProp) {
            
        } else {
        }
      }
    }
  },
  transformSlotRender(item, s) {
    console.log("sss", item);
  },
});
