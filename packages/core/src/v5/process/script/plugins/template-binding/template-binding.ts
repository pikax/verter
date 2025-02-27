import { ProcessItemType } from "../../../types";
import { definePlugin } from "../../types";
import { generateTypeString } from "../utils";

export const TemplateBindingPlugin = definePlugin({
  name: "VerterTemplateBinding",

  post(s, ctx) {
    const isTS = ctx.block.lang === "ts";
    const isAsync = ctx.isAsync;
    const tag = ctx.block.block.tag;
    const name = ctx.prefix("TemplateBinding");

    const bindings = new Set(
      ctx.items
        .filter((x) => x.type === ProcessItemType.Binding)
        .map((x) => x.name)
    );

    const macroBindings = ctx.items
      .filter((x) => x.type === ProcessItemType.MacroBinding)
      .reduce((acc, x) => {
        const n = ctx.prefix(
          x.macro === "withDefaults" ? "defineProps" : x.macro
        );
        if (x.macro === "defineModel") {
          if (!acc[n]) {
            acc[n] = [];
          }
          (acc[n] as string[]).push(x.name);
        } else {
          acc[n] = x.name;
        }
        return acc;
      }, {} as Record<string, string | string[]>);
    const usedBindings = ctx.templateBindings.filter(
      (x) => x.name && bindings.has(x.name)
    );

    console.log("sadssa");
    s.prependLeft(
      tag.pos.close.start,
      `;return{${usedBindings
        .map(
          (x) =>
            `${x.name}/*${x.node.loc.start.offset},${
              x.node.loc.end.offset
            }*/: ${isTS ? `${x.name}  as typeof ${x.name}` : x.name}`
        )
        .concat(
          Object.entries(macroBindings).map(
            ([k, x]) =>
              `${k}:${
                Array.isArray(x)
                  ? `{${x
                      .map((x) => `${x}: ${isTS ? `${x} as typeof ${x}` : x}`)
                      .join(",")}}`
                  : `${isTS ? `${x} as typeof ${x}` : ""}`
              }`
          )
          // Object.entries(macroBindings).map(([k, v]) =>
          //   v.map((x) => `${k}:${k}`)
          // )
        )
        .join(",")}}`
    );

    if (!isTS) {
      s.prependLeft(
        tag.pos.open.start,
        `/** @returns {{${usedBindings
          .map((x) => `${x}:typeof ${x}`)
          .join(",")}}} */`
      );
    }

    const typeStr = generateTypeString(
      name,
      {
        from: `${name}FN`,
        isFunction: true,
      },
      ctx
    );

    s.prependRight(tag.pos.close.end, typeStr);

    s.overwrite(tag.pos.open.start + 1, tag.pos.content.start, `${name}FN`);
  },
});
