import { LocationType, PluginOption, WalkResult } from "../types.js";
import { transpile } from "./v2/transpile";

export default {
  name: "Template",

  process: (context) => {
    const template = context.template;
    if (!template) return;

    const ast = template.ast;

    if (!ast) return;

    const declarations = [] as WalkResult[];
    try {
      const { accessors } = transpile(ast, context.s, { declarations });

      declarations.push({
        type: LocationType.Import,
        from: "vue",
        generated: true,
        items: [
          {
            name: "renderList",
            alias: accessors.renderList,
          },
          {
            name: "normalizeClass",
            alias: accessors.normalizeClass,
          },
          {
            name: "normalizeStyle",
            alias: accessors.normalizeStyle,
          },
        ],
      });
    } catch (e) {
      console.error("eee", e);
    }

    return [
      {
        type: LocationType.Template,
        node: ast,
      },
      ...declarations,
    ];
  },
} satisfies PluginOption;
