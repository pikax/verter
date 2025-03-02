import { ProcessItemType } from "../../../types";
import { definePlugin } from "../../types";

export const BindingPlugin = definePlugin({
  name: "VerterBinding",

  // add known bindings
  transformDeclaration(item, _, ctx) {
    if (!item.name) return;
    ctx.items.push({
      type: ProcessItemType.Binding,
      name: item.name,
      originalName: item.name,
      item,
      node: item.node,
    });
  },
  transformBinding(item, _, ctx) {
    if (!item.name) return;
    ctx.items.push({
      type: ProcessItemType.Binding,
      name: item.name,
      originalName: item.name,
      item,
      node: item.node,
    });
  },
});
