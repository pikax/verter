import {
  AttributeNode,
  DirectiveNode,
  ElementNode,
  NodeTypes,
  SimpleExpressionNode,
} from "@vue/compiler-core";
import {
  declareTemplatePlugin,
  TemplatePlugin,
  TemplateContext,
} from "../../template";
import {
  TemplateProp,
  TemplateTypes,
  TemplateRenderSlot,
} from "../../../../parser/template/types";
import { MagicString } from "@vue/compiler-sfc";
import { createHelperImport } from "../../../utils";

/**
 * Patch types
 * 
// patching HTML Elements
declare module 'vue' {
    interface HTMLAttributes {
        'v-slot'?: (c: {$slots: { default: ()=> any[]}})=>void
    }
}

// patching elements
declare global {
    namespace JSX {
        export interface IntrinsicClassAttributes<T> {
            'v-slot'?: (T extends { $slots: infer S } ? S : undefined) | ((c: T) => T extends { $slots: infer S } ? S : undefined)
        }
    }
}
 */

export const SlotPlugin = declareTemplatePlugin({
  name: "VerterSlot",

  processedParent: new Set<ElementNode>(),
  pre() {
    this.processedParent.clear();
  },

  // slots: new Set<ElementNode>(),
  // pre() {
  //   this.slots.clear();
  // },
  // post(s, ctx) {
  //   const slotInstance = ctx.retrieveAccessor("slotInstance");
  //   // const $slots = ctx.retrieveAccessor("$slot");

  //   // move children to v-slot and initialise $slots
  //   for (const element of this.slots) {
  //     const children = element.children;

  //     const first = children.shift();
  //     // nothing to do if there's no children
  //     if (!first) {
  //       break;
  //     }
  //     const last = children.pop() ?? first;

  //     const pos =
  //       element.loc.start.offset +
  //       element.loc.source
  //         .slice(0, first.loc.start.offset - element.loc.start.offset)
  //         .lastIndexOf(">");

  //     const endPos =
  //       element.loc.source
  //         .slice(last.loc.end.offset - element.loc.start.offset)
  //         .indexOf("<") + last.loc.end.offset;

  //     if (pos === -1 || pos >= first.loc.start.offset) {
  //       console.log("should not happen");
  //       continue;
  //     }

  //     // TODO narrow
  //     s.prependLeft(pos, ` v-slot={(${slotInstance})=>{`);

  //     s.move(pos + 1, endPos, pos);

  //     s.prependRight(pos, "}}");
  //   }
  // },

  handleSlotRender(
    slot: TemplateRenderSlot,
    s: MagicString,
    parent: ElementNode,
    ctx: TemplateContext
  ) {
    if (this.processedParent.has(parent)) return;
    const slotInstance = ctx.retrieveAccessor("slotInstance");
    const isTemplateSlot = !!slot.parent;

    const children = [...parent.children];

    // const pos = isTemplateSlot
    //   ? parent.loc.start.offset + parent.tag.length + 1
    //   : (slot.prop.node?.exp?.loc.start.offset ??
    //       slot.prop.node?.arg.loc.end.offset) + 1;

    // const first = children.shift();
    // // nothing to do if there's no children
    // if (!first) {
    //   this.processedParent.add(parent);
    //   return;
    // }
    // const last = children.pop() ?? first;
    // const tagEnd =
    //   parent.loc.start.offset +
    //   parent.loc.source
    //     .slice(0, first.loc.start.offset - parent.loc.start.offset)
    //     .lastIndexOf(">");

    // const endPos =
    //   parent.loc.source
    //     .slice(last.loc.end.offset - parent.loc.start.offset)
    //     .indexOf("<") + last.loc.end.offset;

    // if (pos === -1 || pos >= first.loc.start.offset) {
    //   console.log("should not happen");
    //   debugger;
    //   return;
    // }

    const parentSource = parent.loc.source;

    const endTagPos = parentSource.lastIndexOf("</") + parent.loc.start.offset;
    const tagEnd =
      parent.loc.start.offset +
      parentSource
        .slice(
          0,
          parent.children.length === 0
            ? endTagPos - parent.loc.start.offset
            : children[0].loc.start.offset - parent.loc.start.offset
        )
        .lastIndexOf(">");
    let pos = -1;

    if (isTemplateSlot) {
      const first = children.shift();
      // nothing to do if there's no children
      if (!first) {
        this.processedParent.add(parent);
        return;
      }
      const last = children.pop() ?? first;

      if (!parent.tag) {
        // TODO mark error, this is because the parent is the AST Root
        throw new Error("Parent tag is missing");
        return;
      }

      pos = parent.loc.start.offset + parent.tag.length + 1;

      if (pos === -1 || pos >= first.loc.start.offset) {
        console.log("should not happen");
        debugger;
        return;
      }
    } else {
      pos = slot.prop.node!.loc.start.offset;
    }
    const slotProp = 'v-slot' // ctx.prefix("v-slot");
    s.prependLeft(pos, ` ${slotProp}={(${slotInstance})=>{`);

    if (ctx.doNarrow && slot.context.conditions.length > 0) {
      ctx.doNarrow(
        {
          index: pos,
          inBlock: true,
          conditions: slot.context.conditions,
          type: "append",
          // type: "prepend",
          // direction: "right",
          condition: slot.condition,
        },
        s
      );
    }

    if (isTemplateSlot) {
      if (tagEnd < endTagPos - 2) {
        // if (tagEnd !== endTagPos) {
        s.move(tagEnd + 1, endTagPos, pos);
      }
      s.prependRight(pos, "}}");
    } else {
      if (tagEnd < endTagPos - 2) {
        s.move(tagEnd + 1, endTagPos, slot.prop.node!.loc.end.offset);
      }
      s.prependRight(slot.prop.node!.loc.end.offset, "}}");
    }

    this.processedParent.add(parent);
    return pos;
  },

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
      // <slot>

      // rename tag to be renderSlot
      s.overwrite(
        node.loc.start.offset + 1,
        node.loc.start.offset + 5,
        renderSlot
      );
      // rename end tag
      if (!node.isSelfClosing) {
        const nameIndex =
          node.loc.source.indexOf("slot", node.loc.source.lastIndexOf("</")) +
          node.loc.start.offset;

        s.overwrite(nameIndex, nameIndex + 4, renderSlot);
      }

      s.prependRight(node.loc.start.offset + 1, `<`);

      // this is where it will be added
      // we insert everything after '<' because it causes
      // issues with v-if and v-for handling
      // since '<' can be added again and is not important
      // for intellisense
      const insertIndex = node.loc.start.offset + 1;

      s.prependRight(insertIndex, ";");

      if (item.name) {
        if (!Array.isArray(item.name) && item.name.node) {
          // if(item.name.type === TemplateTypes.Binding) {

          // }

          if (item.name.node.type === NodeTypes.ATTRIBUTE) {
            const prop = item.name.node as AttributeNode;
            // move name
            s.move(prop.loc.start.offset, prop.loc.end.offset, insertIndex);

            if (prop.value) {
              s.overwrite(
                prop.nameLoc.start.offset,
                prop.value.loc.start.offset + 1,
                '["'
              );

              s.overwrite(
                prop.value.loc.end.offset - 1,
                prop.value.loc.end.offset,
                '"]'
              );
            }
          } else if ("directive" in item.name && item.name.directive) {
            const directive = item.name.directive;
            // move name to the beginning
            s.move(
              directive.loc.start.offset,
              directive.loc.end.offset,
              insertIndex
            );

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
          }
        }
      } else {
        // default slot
        s.prependLeft(insertIndex, `.default`);
      }

      s.prependLeft(insertIndex, `const ${renderSlot}=${$slots}`);

      // s.prependRight(insertIndex, "<");

      s.update(node.loc.start.offset, node.loc.start.offset + 1, "");
    } else {
      // v-slot
    }
  },

  /**
   *
   *
   * slotRender aka ___VERTER___SLOT_CALLBACK
declare function ___VERTER___SLOT_CALLBACK<T>(slot?: (...args: T[]) => any): (cb: ((...args: T[]) => any))=>void;


<div v-slot={(ci):any=>{
  const $slots = ci.$slots;
  ___VERTER___SLOT_CALLBACK($slots.default)(({})=>{
    return <div></div>
  })
}}
   * @param slot
   * @param s
   * @param ctx
   */
  transformSlotRender(slot, s, ctx) {
    // const $slots = ctx.retrieveAccessor("$slot");
    // StrictRenderSlot
    ctx.items.push(createHelperImport(["StrictRenderSlot"], ctx.prefix));
    const slotRender = ctx.retrieveAccessor("StrictRenderSlot");
    const slotInstance = ctx.retrieveAccessor("slotInstance");

    // <template v-slot
    if (slot.parent) {
      // this.slots.add(slot.parent);
      this.handleSlotRender(slot, s, slot.parent, ctx);

      const node = slot.element;
      if (node.type === NodeTypes.ELEMENT) {
        const insertIndex = node.loc.start.offset + 1;

        // remove `<` we will be adding this later
        s.update(node.loc.start.offset, node.loc.start.offset + 1, "");

        const tagEnd = node.isSelfClosing
          ? node.loc.end.offset - 2
          : node.loc.source
              .slice(
                0,
                (node.children[0]?.loc.start.offset ??
                  node.loc.end.offset - "</template>".length) -
                  node.loc.start.offset
              )
              .lastIndexOf(">") + node.loc.start.offset;

        // move props to after `<`
        s.move(node.loc.start.offset + "<template".length, tagEnd, insertIndex);

        const prop = slot.prop;

        if (prop.type === TemplateTypes.Directive) {
          // replace v-slot or # with ___VERTER___$slot
          const start = prop.node.loc.start.offset;
          const end =
            start + (prop.node.rawName?.startsWith("v-slot:") ? 7 : 1);

          s.prependLeft(start, `${slotRender}(${slotInstance}.`);
          s.overwrite(start, end, `$slots`);
          if (prop.node.arg) {
            if (
              prop.node.arg.type === NodeTypes.SIMPLE_EXPRESSION &&
              prop.node.arg.isStatic
            ) {
              // check if we can use . notation or if we need to use wrap in ['']
              if (/^\w+$/.test(prop.node.arg.content)) {
                s.prependLeft(end, ".");
              } else {
                s.prependLeft(end, `['`);
                s.appendLeft(prop.node.arg.loc.end.offset, `']`);
              }
            }
          }

          if (prop.node.exp) {
            //todo

            // update = to )
            s.overwrite(
              prop.node.exp.loc.start.offset - 2,
              prop.node.exp.loc.start.offset - 1,
              ")"
            );

            s.prependLeft(prop.node.exp.loc.start.offset - 1, "(");

            // update delimiters " to (
            s.overwrite(
              prop.node.exp.loc.start.offset - 1,
              prop.node.exp.loc.start.offset,
              "("
            );
            s.overwrite(
              prop.node.exp.loc.end.offset,
              prop.node.exp.loc.end.offset + 1,
              ")"
            );
            s.prependLeft(prop.node.exp.loc.end.offset + 1, "=>{");
          } else {
            s.appendLeft(tagEnd, `)(()=>{`);
          }

          if (ctx.doNarrow && slot.context.conditions.length > 0) {
            ctx.doNarrow(
              {
                index: prop.node.exp
                  ? prop.node.exp.loc.end.offset + 1
                  : tagEnd,
                inBlock: true,
                conditions: slot.context.conditions,
                type: "append",
                // empty condition because we need the current slot condition to also apply if present
                condition: null,
              },
              s
            );
          }

          s.prependLeft(node.loc.end.offset, "});");
        } else {
          throw new Error("TODO: handle slot directive");
        }

        s.prependRight(node.loc.start.offset + 1, "<");
      } else {
        throw new Error("TODO handle slot not being an element");
      }
    } else {
      // <Comp v-slot="slot">
      const element = slot.element as ElementNode;
      this.handleSlotRender(slot, s, element as ElementNode, ctx);

      const endPropIndex = slot.prop.node!.loc.end.offset;

      const directive = slot.prop.node as DirectiveNode;

      s.overwrite(
        slot.prop.node!.loc.start.offset,
        slot.prop.node!.loc.start.offset +
          (directive.rawName!.startsWith("#") ? 1 : "v-slot".length),
        `$slots`
      );

      s.prependRight(
        directive.loc.start.offset,
        `${slotRender}(${slotInstance}.`
      );

      // update exp delimiters
      if (directive.exp) {
        s.overwrite(
          directive.exp.loc.start.offset - 1,
          directive.exp.loc.start.offset,
          ""
        );
        s.overwrite(
          directive.exp.loc.end.offset,
          directive.exp.loc.end.offset + 1,
          ""
        );
      }

      if (directive.arg) {
        const arg = directive.arg as SimpleExpressionNode;
        if (arg.isStatic) {
          // the # was overriden to be $slots, this is to allow over in intellisense
          if (directive.rawName!.startsWith("v-")) {
            s.update(arg.loc.start.offset - 1, arg.loc.start.offset, ".");
          } else {
            s.prependLeft(arg.loc.start.offset, ".");
          }
        } else if (directive.rawName!.startsWith("v-")) {
          s.remove(arg.loc.start.offset - 1, arg.loc.start.offset);
        }

        if (directive.exp) {
          s.update(
            directive.arg.loc.end.offset,
            directive.exp.loc.start.offset,
            ""
          );
          s.prependLeft(directive.exp.loc.start.offset, `)((`);

          s.prependLeft(directive.exp.loc.end.offset, `)=>{`);
        } else {
          s.prependLeft(directive.arg.loc.end.offset, ")(()=>{");
        }
      } else {
        if (directive.exp) {
          s.update(
            directive.loc.start.offset +
              (directive.rawName!.startsWith("#") ? 1 : "v-slot".length),
            directive.loc.start.offset +
              (directive.rawName!.startsWith("#") ? 2 : "v-slot=".length),
            ""
          );

          s.prependLeft(directive.exp.loc.start.offset, `.default)((`);

          s.prependLeft(directive.exp.loc.end.offset, `)=>{`);
        } else {
          s.appendLeft(directive.loc.end.offset, `.default`);
          s.appendLeft(directive.loc.end.offset, `)(()=>{`);
        }
      }

      if (ctx.doNarrow && slot.context.conditions.length > 0) {
        ctx.doNarrow(
          {
            index: directive.loc.end.offset,
            inBlock: true,
            conditions: slot.context.conditions,
            type: "append",
            // empty condition because we need the current slot condition to also apply if present
            condition: null,
          },
          s
        );
      }
      // content

      // closing

      s.prependRight(directive.loc.end.offset, "})");
    }
  },
});
