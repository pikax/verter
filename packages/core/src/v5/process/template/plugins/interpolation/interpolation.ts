import { TemplatePlugin } from "../../template";

export const InterpolationPlugin = {
  name: "VerterInterpolation",
  transformInterpolation(item, s) {
    // convert {{ to {
    s.remove(item.node.loc.start.offset, item.node.loc.start.offset + 1);

    // convert }} to }
    s.remove(item.node.loc.end.offset - 1, item.node.loc.end.offset);
  },
} as TemplatePlugin;
