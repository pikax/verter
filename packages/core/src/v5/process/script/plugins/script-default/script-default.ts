import { ScriptTypes, VerterASTNode } from "../../../../parser";
import { ProcessItemType } from "../../../types";
import { definePlugin } from "../../types";
import { generateTypeString } from "../utils";

export const ScriptDefaultPlugin = definePlugin({
  name: "VerterScriptDefault",
  // enforce: "pre",

  post(s, ctx) {
    const isTS = ctx.block.lang === "ts";
    const isSetup = ctx.isSetup;
    const isAsync = ctx.isAsync;
    const tag = ctx.block.block.tag;
    const name = ctx.prefix("default_Component");

    const defineComponent = ctx.prefix("defineComponent");

    if (isSetup) {
      ctx.items.push({
        type: ProcessItemType.Import,
        from: "vue",
        items: [
          {
            name: "defineComponent",
            alias: defineComponent,
          },
        ],
      });

      const optionsMacro = ctx.items.find(
        (x) => x.type === ProcessItemType.Options
      );

      let options = "{}";
      if (optionsMacro) {
        // TODO this should be referenced directly to the optionsMacro
        const start = optionsMacro.expression.start;
        const end = optionsMacro.expression.end;

        options = s.slice(start, end);
      }

      s.append(`\n;export const ${name}=${defineComponent}(${options});`);
    } else {
      const componentExport = ctx.block.result?.items.find(
        (x) => x.type === ScriptTypes.DefaultExport
      );

      if (!componentExport) {
        ctx.items.push({
          type: ProcessItemType.Import,
          from: "vue",
          items: [
            {
              name: "defineComponent",
              alias: defineComponent,
            },
          ],
        });
        s.append(`;export const ${name}=${defineComponent}({});`);
        return;
      }

      const defaultStartPos = componentExport.node.start + 7;
      const defaultEndPos = componentExport.node.declaration.start;

      s.overwrite(defaultStartPos, defaultEndPos, `const ${name}=`);

      switch (componentExport.node.declaration.type) {
        // if does not have a wrapper
        case "ObjectExpression": {
          ctx.items.push({
            type: ProcessItemType.Import,
            from: "vue",
            items: [
              {
                name: "defineComponent",
                alias: defineComponent,
              },
            ],
          });

          s.appendRight(
            componentExport.node.declaration.start,
            `${defineComponent}(`
          );
          s.appendLeft(componentExport.node.declaration.end, ");");
          return;
        }
        default: {
        }
      }

      // const componentExport = ctx.items.find(x=>x.type === ProcessItemType.)
    }
  },
});
