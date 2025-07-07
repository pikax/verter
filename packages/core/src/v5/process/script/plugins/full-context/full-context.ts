import { ScriptTypes } from "../../../../parser/script";
import { BlockPlugin } from "../../../template/plugins";
import { ProcessItemBinding, ProcessItemType } from "../../../types";
import { definePlugin } from "../../types";
import { generateTypeString } from "../utils";

export const FullContextPlugin = definePlugin({
  name: "VerterFullContext",
  enforce: "post",
  pre(s, ctx) {
    const importItem = ctx.isTS
      ? { name: "UnwrapRef", alias: ctx.prefix("UnwrapRef") }
      : { name: "unref", alias: ctx.prefix("unref") };
    ctx.items.push({
      type: ProcessItemType.Import,
      from: "vue",
      asType: ctx.isTS,
      items: [importItem],
    });
  },
  post(s, ctx) {
    const isTS = ctx.block.lang === "ts";
    const isAsync = ctx.isAsync;
    const fullContext = ctx.prefix("FullContext");

    const unref = ctx.prefix("unref");
    const unwrapRef = ctx.prefix("UnwrapRef");

    const bindings = ctx.items.filter(
      (x) => x.type === ProcessItemType.Binding && x.item.node
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
            content.add(source.slice(node.start, node.end));
          }
        }
        case ScriptTypes.FunctionCall: {
        }
      }
    }

    const typeStr = generateTypeString(
      fullContext,
      {
        from: `${fullContext}FN`,
        isFunction: true,
      },
      ctx
    );

    const str = `;${isAsync ? "async " : ""}function ${fullContext}FN${
      ctx.generic ? `<${ctx.generic.source}>` : ""
    }() {${[...content].join("\n")};return{${[...names]
      .map(
        (x) =>
          `${x}${
            isTS ? `: {} as ${unwrapRef}<typeof ${x}>` : `: ${unref}(${x})"`
          }`
      )
      .join(",")}}};${typeStr}`;

    s.prependRight(ctx.block.block.tag.pos.close.end, str);
  },
});
