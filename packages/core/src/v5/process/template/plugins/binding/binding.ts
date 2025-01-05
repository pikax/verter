import { TemplatePlugin } from "../../template";

export const BindingPlugin = {
  name: "VerterBinding",
  transformBinding(item, s, ctx) {
    if (item.ignore) {
      return;
    }

    const accessor = ctx.retrieveAccessor("ctx");

    if (item.parent?.type === "ObjectProperty" && item.parent.shorthand) {
      s.prependRight(item.node.loc.start.offset, `${item.name}: ${accessor}.`);
    } else {
      s.prependRight(item.node.loc.start.offset, `${accessor}.`);
    }
  },
} as TemplatePlugin;
