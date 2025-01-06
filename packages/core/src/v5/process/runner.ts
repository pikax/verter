import {
  TemplateItem,
  TemplateItemByType,
  TemplateTypes,
} from "../parser/template/types";
import { ProcessContext, ProcessPlugin } from "./types";

export function runPlugins(
  items: TemplateItem[],
  plugins: ProcessPlugin[],
  context: ProcessContext
) {
  const pluginsByType = {
    [TemplateTypes.Condition]: [],
    [TemplateTypes.Loop]: [],
    [TemplateTypes.Element]: [],
    [TemplateTypes.Prop]: [],
    [TemplateTypes.Binding]: [],
    [TemplateTypes.SlotRender]: [],
    [TemplateTypes.SlotDeclaration]: [],
    [TemplateTypes.Comment]: [],
    [TemplateTypes.Text]: [],
    [TemplateTypes.Directive]: [],
    [TemplateTypes.Interpolation]: [],
    [TemplateTypes.Function]: [],
  } as {
    [K in TemplateTypes]: Array<
      (item: TemplateItemByType[K], context: ProcessContext) => void
    >;
  };
  const PLUGIN_TYPES = Object.keys(pluginsByType) as readonly TemplateTypes[];

  const prePlugins = [] as Array<(context: ProcessContext) => void>;
  const postPlugins = [] as Array<(context: ProcessContext) => void>;

  [...plugins]
    .sort((a, b) => {
      if (a.enforce === "pre" && b.enforce === "post") {
        return -1;
      }
      if (a.enforce === "post" && b.enforce === "pre") {
        return 1;
      }
      return 0;
    })
    .forEach((x) => {
      for (const [key, value] of Object.entries(x)) {
        if (typeof value !== "function") return;
        switch (key) {
          case "pre": {
            prePlugins.push(value as any);
            break;
          }
          case "post": {
            postPlugins.push(value as any);
            break;
          }

          case "transform": {
            PLUGIN_TYPES.forEach((type) => {
              pluginsByType[type].push(value as any);
            });
            break;
          }
          default: {
            if (key.startsWith("transform")) {
              const type = key.slice(9) as TemplateTypes;
              pluginsByType[type].push(value as any);
            }
          }
        }
      }
    });

  for (const plugin of prePlugins) {
    plugin(context);
  }

  for (const item of items) {
    for (const plugin of pluginsByType[item.type]) {
      plugin(item as any, context);
    }
  }

  for (const plugin of postPlugins) {
    plugin(context);
  }
}
