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

    const usedBindings = ctx.templateBindings
      .filter((x) => x.name && bindings.has(x.name))
      .map((x) => x.name!);

    s.prependLeft(
      tag.pos.close.start,
      `;return{${usedBindings
        .map((x) => `${x}${isTS ? `:${x} as typeof ${x}` : ""}`)
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
