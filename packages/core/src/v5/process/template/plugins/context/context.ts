import {
  ParsedBlockScript,
  TemplateBinding,
  TemplateItem,
  TemplateTypes,
} from "../../../../parser";
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

    s.prependLeft(ctx.block.block.block.loc.start.offset, 'fioooo');
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
