import { DirectiveNode, NodeTypes } from "@vue/compiler-core";
import { TemplatePlugin } from "../../template";

export const SlotPlugin = {
  name: "VerterSlot",

  /**
     * 
     * Example:
const $slots = defineSlots<{
    default: () => HTMLDivElement[],

    input: (a: { name: string }) => HTMLInputElement
}>()

declare function PatchSlots<T extends Record<string, (...args: any[]) => any>>(slots: T): {
    [K in keyof T]: T[K] extends () => any ? (props: {}) => JSX.Element : (...props: Parameters<T[K]>) => JSX.Element
}

const $slots = PatchSlots(c.$slots);





     * @param slot 
     * @param s 
     * @param ctx 
     */

  transformSlotDeclaration(item, s, ctx) {
    const $slots = ctx.retrieveAccessor("$slot");
    const renderSlot = ctx.retrieveAccessor("slotComponent");

    const node = item.node;

    if (node.type === NodeTypes.ELEMENT) {
      //<slot>

      // move tag to before <
      s.move(
        node.loc.start.offset + 1,
        node.loc.start.offset + 1 + node.tag.length,
        node.loc.start.offset
      );

      // handle name
      if (item.name) {
        //remove name=""
        const nameNode = item.name.node!;
        if ("nameLoc" in nameNode) {
          // move name to the beginning
          s.move(
            nameNode.loc.start.offset,
            nameNode.loc.end.offset,
            node.loc.start.offset
          );

          if (nameNode.value) {
            s.overwrite(
              nameNode.nameLoc.start.offset,
              nameNode.value.loc.start.offset + 1,
              '["'
            );

            s.overwrite(
              nameNode.value.loc.end.offset - 1,
              nameNode.value.loc.end.offset,
              '"]'
            );
          }
        } else if ("directive" in item.name && item.name.directive) {
          const directive = item.name.directive as DirectiveNode;

          // move name to the beginning
          s.move(
            directive.loc.start.offset,
            directive.loc.end.offset,
            node.loc.start.offset
          );
          // s.move(
          //   node.loc.start.offset + 5,
          //   directive.loc.start.offset,
          //   directive.loc.end.offset
          // );

          if (directive.arg) {
            s.remove(
              directive.arg.loc.start.offset,
              directive.arg.loc.end.offset
            );
          }

          if (directive.exp) {
            s.overwrite(
              directive.exp.loc.start.offset - 2,
              directive.exp.loc.start.offset,
              "["
            );
            s.overwrite(
              directive.exp.loc.end.offset,
              directive.exp.loc.end.offset + 1,
              "]"
            );
          }

          //   if (directive.exp) {
          //     s.remove(
          //       directive.loc.start.offset,
          //       directive.exp.loc.start.offset
          //     );
          //   } else {
          //   }

          //   s.remove(directive.loc.start.offset, directive.loc.end.offset);
        }
      } else {
        s.prependLeft(node.loc.start.offset + node.tag.length + 1, `.default`);
      }

      // rename slot to $slot
      s.prependRight(node.loc.start.offset + 1, $slots.slice(0, -4));

      // add at the end of the slot;
      s.prependRight(node.loc.start.offset, ";");

      // add const renderSlot = at the beginning
      s.prependLeft(node.loc.start.offset, `const ${renderSlot} =`);

      //replace the tag with the newly added renderSlot
      s.prependLeft(node.loc.start.offset + 1, renderSlot);
    } else {
      // v-slot
    }
  },
  transformSlotRender(slot, s, ctx) {},
} as TemplatePlugin;
