import {
  ParsedBlockScript,
  TemplateBinding,
  TemplateItem,
  TemplateTypes,
} from "../../../../parser";
import { ResolveOptionsFilename } from "../../../script";
import { generateImport } from "../../../utils";
import { TemplatePlugin } from "../../template";

export const ContextPlugin = {
  name: "VerterContext",
  pre(s, ctx) {
    const isSetup =
      ctx.blocks.find(
        (x) => x.type === "script" && x.block.tag.attributes.setup
      ) !== undefined;

    const options = ResolveOptionsFilename(ctx);

    const TemplateBindingName = ctx.prefix("TemplateBinding");
    const FullContextName = ctx.prefix("FullContext");
    const DefaultName = ctx.prefix("default");
    const ComponentInstanceName = ctx.prefix("ComponentInstance");

    const macros = isSetup
      ? [
          [ctx.prefix("defineProps"), "$props"],
          [ctx.prefix("defineEmits"), "$emit"],
          [ctx.prefix("defineSlots"), "$slots"],
        ]
      : [];

    const importStr = generateImport([
      {
        from: `./${options}`,
        items: [
          { name: TemplateBindingName },
          { name: FullContextName },
          {
            name: DefaultName,
          },
          ...macros.map(([name]) => ({ name })),
        ],
      },
    ]);

    s.prepend(`${importStr}\n`);

    const instanceStr = `const ${ComponentInstanceName} = new ${DefaultName}();`;
    const CTX = ctx.retrieveAccessor("ctx");

    // todo add generic information
    const ctxItems = [FullContextName, TemplateBindingName].map((x) =>
      ctx.isTS ? `...({} as ${x})` : `...${x}`
    );
    const ctxStr = `const ${CTX} = {${[
      `...${ComponentInstanceName}`,
      `...${DefaultName}.components`,
      // `...${macros.map(([name, prop]) => `${name}(${prop})`).join(",")}`,
      ...macros.map(
        ([name, prop]) => `${prop}: ${ctx.isTS ? `{} as ${name}` : name}`
      ),
      ...ctxItems,
    ].join(",")}};`;

    s.prependLeft(
      ctx.block.block.block.loc.start.offset,
      [instanceStr, ctxStr].join("\n")
    );
  },
} as TemplatePlugin;
