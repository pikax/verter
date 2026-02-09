import { ScriptTypes } from "../../../../parser/script";
import { BlockPlugin } from "../../../template/plugins";
import { ProcessItemBinding, ProcessItemType } from "../../../types";
import { createHelperImport } from "../../../utils";
import { definePlugin } from "../../types";
import { generateTypeString } from "../utils";

export const FullContextPlugin = definePlugin({
  name: "VerterFullContext",
  enforce: "post",
  pre(s, ctx) {
    // const importItem = ctx.isTS
    //   ? { name: "UnwrapRef", alias: ctx.prefix("UnwrapRef") }
    //   : { name: "unref", alias: ctx.prefix("unref") };
    // ctx.items.push({
    //   type: ProcessItemType.Import,
    //   from: "vue",
    //   asType: ctx.isTS,
    //   items: [importItem],
    // });

    ctx.items.push(createHelperImport(["Prettify", "shallowUnwrapRef"], ctx.prefix));
  },
  post(s, ctx) {
    const isTS = ctx.block.lang === "ts";
    const isAsync = ctx.isAsync;
    const fullContext = ctx.prefix("FullContext");
    const shallowUnwrapRef = ctx.prefix("shallowUnwrapRef");

    const unref = ctx.prefix("unref");
    const unwrapRef = ctx.prefix("UnwrapRef");

    const prettify = ctx.prefix("Prettify");

    const bindings = ctx.items.filter(
      (x) => x.type === ProcessItemType.Binding && x.item.node,
    ) as ProcessItemBinding[];

    const names = new Set<string>();
    // const content = [] as string[];
    const content = new Set<string>();
    const source = s.original;
    for (const b of bindings) {
      switch (b.item.type) {
        case ScriptTypes.Declaration: {
          const name = b.item.name;
          const node = b.item.declarator;
          if (name) {
            names.add(name);
            // @ts-expect-error TODO improve this, this shouldn't be necessary
            content.add(source.slice(node.start, node.end));
          }
        }
        case ScriptTypes.FunctionCall: {
        }
      }
    }

    const importBindings =
      ctx.block.result?.items
        .filter((x) => x.type === ScriptTypes.Import)
        .flatMap((x) => x.bindings) ?? [];

    for (const b of importBindings) {
      if (b.name) {
        names.add(b.name);
      }
    }

    const typeStr = generateTypeString(
      fullContext,
      {
        from: `${fullContext}FN`,
        isFunction: true,
      },
      ctx,
    );

    const str = `;${isAsync ? "async " : ""}function ${fullContext}FN${
      ctx.generic ? `<${ctx.generic.source}>` : ""
    }() {${[...content].join("\n")};return ${shallowUnwrapRef}({${[...names]
      .map(
        (x) =>
          `${x}${
            isTS
              ? // ? `: {} as ${prettify}<${unwrapRef}<typeof ${x}>>`
                `: {} as typeof ${x}`
              : `: ${x}"`
          }`,
      )
      .join(",")}})};${typeStr}`;

    s.prependRight(ctx.block.block.tag.pos.close.end, str);
  },
});
