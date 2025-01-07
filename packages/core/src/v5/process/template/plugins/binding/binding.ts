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
      // if(item.node.loc.source.indexOf(item.name)!== )

      // when there's a argument binding the name is wrapped in [{name}], but the loc
      // is to [ instead of {name} index
      const nameIndex = item.name ? item.node.loc.source.indexOf(item.name) : 0;
      s.prependRight(item.node.loc.start.offset + nameIndex, `${accessor}.`);
    }
  },
} as TemplatePlugin;
