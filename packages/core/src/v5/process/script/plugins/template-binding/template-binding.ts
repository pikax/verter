import { VerterASTNode } from "../../../../parser";
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

    // const bindings = new Set(
    //   ctx.items
    //     .filter(
    //       (x) =>
    //         x.type === ProcessItemType.Binding ||
    //         x.type === ProcessItemType.Import
    //     )
    //     .flatMap((x) =>
    //       x.type === ProcessItemType.Binding
    //         ? x.name
    //         : x.items.map((x) => x.alias ?? x.name)
    //     )
    // );
    // const bindings = new Map(
    //   ctx.items
    //     .filter(
    //       (x) =>
    //         x.type === ProcessItemType.Binding ||
    //         x.type === ProcessItemType.Import
    //     )
    //     .flatMap((x) => (x.type === ProcessItemType.Binding ? x : x.items))
    //     .map((x) => '  [ x.name, x])
    // );

    const bindings = new Map<string, VerterASTNode>();
    for (const item of ctx.items) {
      switch (item.type) {
        case ProcessItemType.Binding: {
          bindings.set(item.name, item.node);
          break;
        }
        // case ProcessItemType.Import: {
        //   for (const importItem of item.items) {
        //     bindings.set(importItem.alias ?? importItem.name, importItem.node);
        //   }
        //   break;
        // }
      }
    }

    const ModelAccessor = ctx.prefix("models");
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
    const usedBindings = ctx.templateBindings
      .map((x) => {
        if (!x.name) return;
        const b = bindings.get(x.name);
        if (!b) return;
        // rebind path
        return {
          name: x.name,
          start: b.start,
          end: b.end,
        };
      })
      // .map((x) => (x.name ? bindings.get(x.name) : undefined))
      .filter((x) => !!x);

    // .filter((x) => x.name && bindings.has(x.name));

    s.prependLeft(
      tag.pos.close.start,
      `;return{${usedBindings
        .map(
          (x) =>
            `${x.name}/*${x.start},${x.end}*/: ${
              isTS ? `${x.name} as typeof ${x.name}` : x.name
            }`
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
          .map((x) => `${x.name}:typeof ${x.name}`)
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
