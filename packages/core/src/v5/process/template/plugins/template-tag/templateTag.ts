import { TemplatePlugin } from "../../template";

export const TemplateTagPlugin = {
  name: "VerterTemplateTag",
  pre(s, context) {
    const pos = context.block.block.tag.pos;
    // replace <
    s.overwrite(pos.open.start, pos.open.start + 1, "function ");
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
    if (context.generic) {
      s.prependLeft(pos.open.end - 1, `<`);
      s.move(
        context.generic.position.start,
        context.generic.position.end,
        pos.open.end - 1
      );
      s.prependRight(pos.open.end - 1, `>`);
    }

    // replace closing tag
    s.overwrite(pos.close.start, pos.close.end, "}");

    s.prependLeft(context.block.block.block.loc.start.offset, "\n<>");
    s.prependRight(context.block.block.block.loc.end.offset, "\n</>");
  },
} as TemplatePlugin;
