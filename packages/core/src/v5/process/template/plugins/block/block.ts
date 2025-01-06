import { MagicString } from "@vue/compiler-sfc";
import {
  declareTemplatePlugin,
  TemplateContext,
  TemplatePlugin,
} from "../../template";
import {
  CAMELIZE,
  DirectiveNode,
  ExpressionNode,
  NodeTypes,
  SourceLocation,
  Node,
} from "@vue/compiler-core";
import { camelize } from "vue";
import { PropsContext } from "../../../../parser/template/element/props";
import { ParseTemplateContext } from "../../../../parser/template";

export const BlockPlugin = declareTemplatePlugin({
  name: "VerterBlock",

  // blocks: new Set<Node>(),
  conditions: new Map<Node, Node[]>(),

  pre() {
    // this.blocks.clear();
    this.conditions.clear();
  },

  post(s) {
    for (const [parent, block] of this.conditions) {
      const first = block.shift()!;
      const last = block.pop() ?? first;

      s.prependRight(first.loc.start.offset, "{()=>{");
      s.prependLeft(last.loc.end.offset, "}}");

      // s.prependRight(block.loc.start.offset, "{()=>{");
      // s.prependLeft(block.loc.end.offset, "}}");
    }
  },

  transformCondition(item) {
    let parentBlock = this.conditions.get(item.parent);
    if (parentBlock) {
      parentBlock.push(item.element);
    } else {
      this.conditions.set(item.parent, [item.element]);
    }
  },

  // transformProp(prop, s, ctx) {
  //   if (prop.node === null || !("context" in prop)) {
  //     return;
  //   }

  //   const conditions = (prop.context as ParseTemplateContext).conditions;
  //   if (conditions.length === 0) {
  //     return;
  //   }

  //   const exp = prop.node.exp;
  //   if (!exp) {
  //     return;
  //   }

  //   debugger;
  // },
});
