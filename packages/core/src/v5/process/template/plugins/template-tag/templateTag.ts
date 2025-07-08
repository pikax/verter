import { TemplatePlugin } from "../../template";

export const TemplateTagPlugin = {
  name: "VerterTemplateTag",
  pre(s, ctx) {
    const pos = ctx.block.block.tag.pos;
    // replace <
    s.overwrite(
      pos.open.start,
      pos.open.start + 1,
      `export ${ctx.isAsync ? "async " : ""}function `
    );
    s.update(pos.open.end - 1, pos.open.end, `(){`);

    //     s.prependLeft(pos.open.start, `import 'vue/jsx';\n
    // // patching elements
    // declare global {
    //     namespace JSX {
    //         export interface IntrinsicClassAttributes<T> {
    //             'v-slot'?: (T extends { $slots: infer S } ? S : undefined) | ((c: T) => T extends { $slots: infer S } ? S : undefined)
    //         }
    //     }
    // }
    // \n`);

    // move generic to template
    if (ctx.generic) {
      s.prependLeft(pos.open.end - 1, `<`);
      s.move(
        ctx.generic.position.start,
        ctx.generic.position.end,
        pos.open.end - 1
      );
      s.prependRight(pos.open.end - 1, `>`);
    }

    // replace closing tag
    s.overwrite(pos.close.start, pos.close.end, "</>}");

    s.appendLeft(pos.open.end, `\n<>`);

    // s.appendRight(pos.close.start, `\n</>`)
    // s.prependRight(ctx.block.block.block.loc.end.offset, "\n</>");
    // s.prependRight(ctx.block.block.block.loc.start.offset, "\n<>");
  },
} as TemplatePlugin;
