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
    // const items = (ctx.block.result?.items ?? ([] as TemplateItem[])).filter(
    //   (x) => x.type === TemplateTypes.Binding && !x.ignore && !x.skip && x.name
    // ) as TemplateBinding[];

    // const names = new Set(items.map((x) => x.name));

    // const jsBlock = ctx.blocks.find((x) => x.type === "script" && x.result) as
    //   | ParsedBlockScript
    //   | undefined;

    // if (!jsBlock) {
    //   debugger;
    //   return;
    // }

    // const items = jsBlock.

    // s.prependRight(
    //   ctx.block.block.block.loc.start.offset,
    //   `let {${Array.from(names).join(",")}} = {}`
    // );

    // console.log("found ", items);

    const options = ResolveOptionsFilename(ctx);

    const TemplateBindingName = ctx.prefix("TemplateBinding");
    const FullContextName = ctx.prefix("FullContext");
    const DefaultName = ctx.prefix("default");
    const ComponentInstanceName = ctx.prefix("ComponentInstance");
  

    const macros = [
      [ctx.prefix('defineProps'), '$props'],
      [ctx.prefix('defineEmits'), '$emit'],
      [ctx.prefix('defineSlots'), '$slots'],
    ]


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
      // `...${macros.map(([name, prop]) => `${name}(${prop})`).join(",")}`,
      ...macros.map(([name, prop]) => `${prop}: ${ctx.isTS ? `{} as ${name}` : name}`),
      ...ctxItems,
    ].join(",")}};`;

    s.prependLeft(
      ctx.block.block.block.loc.start.offset,
      [instanceStr, ctxStr].join("\n")
    );
  },
  //   transformBinding(item, s, ctx) {
  //     if (item.ignore || "skip" in item) {
  //       return;
  //     }

  //     const accessor = ctx.retrieveAccessor("ctx");

  //     if (item.parent?.type === "ObjectProperty" && item.parent.shorthand) {
  //       s.prependRight(item.node.loc.start.offset, `${item.name}: ${accessor}.`);
  //     } else {
  //       // if(item.node.loc.source.indexOf(item.name)!== )

  //       // when there's a argument binding the name is wrapped in [{name}], but the loc
  //       // is to [ instead of {name} index
  //       if (item.name) {
  //         let offset =
  //           item.node.loc.source.indexOf(item.name) + item.node.loc.start.offset;

  //         // on Template Literal the position is skewed
  //         if (s.original.slice(offset, offset + item.name.length) !== item.name) {
  //           offset =
  //             item.exp!.loc.source.indexOf(item.name) +
  //             item.exp!.loc.start.offset;
  //         }
  //         s.prependRight(offset, `${accessor}.`);
  //       } else {
  //         s.prependRight(item.node.loc.start.offset, `${accessor}.`);
  //       }

  //       // if (item.name) {
  //       //   if (item.exp) {
  //       //     const offset =
  //       //       item.exp.loc.source.indexOf(item.name) + item.exp.loc.start.offset;
  //       //     s.prependRight(offset, `${accessor}.`);
  //       //   } else {
  //       //     const offset =
  //       //       item.node.loc.source.indexOf(item.name) +
  //       //       item.node.loc.start.offset;
  //       //     s.prependRight(offset, `${accessor}.`);
  //       //   }
  //       // } else {
  //       //   s.prependRight(item.node.loc.start.offset, `${accessor}.`);
  //       // }
  //     }
  //   },
} as TemplatePlugin;
