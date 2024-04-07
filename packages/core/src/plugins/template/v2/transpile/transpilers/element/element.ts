import {
  BaseElementNode,
  Node,
  CommentNode,
  ComponentNode,
  DirectiveNode,
  ElementTypes,
  ExpressionNode,
  NodeTypes,
  SimpleExpressionNode,
  TemplateChildNode,
  TemplateNode,
  extractIdentifiers,
} from "@vue/compiler-core";
import {
  appendCtx,
  createTranspiler,
  generateNarrowCondition,
  withNarrowCondition,
} from "../../utils";
import { TranspileContext } from "../../types";
import type * as babel_types from "@babel/types";
import {
  isExpression as isExpressionBabel,
  isNode as isNodeBabel,
} from "@babel/types";
import { VerterNode } from "../../../walk";
import { camelize, capitalize, isString } from "@vue/shared";

export default createTranspiler(NodeTypes.ELEMENT, {
  enter(node, parent, context) {
    const tagOffset = node.loc.source.indexOf(node.tag, 0);
    const tag = resolveTag(node, context);

    const isWebComponent =
      node.tagType === ElementTypes.COMPONENT &&
      checkWebComponent(node.tag, tag, context);

    const tagIndex = node.loc.start.offset + tagOffset;
    if (!isWebComponent && tag !== node.tag) {
      context.s.overwrite(tagIndex, tagIndex + node.tag.length, tag);
    }

    switch (node.tagType) {
      case ElementTypes.COMPONENT: {
        if (!isWebComponent && context.accessors.comp) {
          context.s.appendLeft(tagIndex, context.accessors.comp + ".");
        }
        break;
      }
      case ElementTypes.ELEMENT: {
        break;
      }
      case ElementTypes.SLOT: {
        break;
      }
      case ElementTypes.TEMPLATE: {
        break;
      }
    }
  },
  leave(node, parent, context) {
    const tag = resolveTag(node, context);

    const isWebComponent =
      node.tagType === ElementTypes.COMPONENT &&
      checkWebComponent(node.tag, tag, context);
    // update the endTag
    if (!node.isSelfClosing) {
      if (!isWebComponent) {
        const endTagIndex =
          node.loc.start.offset + node.loc.source.lastIndexOf(node.tag);
        if (tag !== node.tag) {
          context.s.overwrite(endTagIndex, endTagIndex + node.tag.length, tag);
        }
        if (node.tagType === ElementTypes.COMPONENT && context.accessors.comp) {
          context.s.appendLeft(endTagIndex, context.accessors.comp + ".");
        }
      }
    }

    switch (node.tagType) {
      case ElementTypes.COMPONENT: {
        if (isWebComponent || node.children.length === 0) break;

        const tagBlockEnd =
          node.loc.start.offset +
          node.loc.source
            .slice(0, node.children[0].loc.start.offset - node.loc.start.offset)
            .lastIndexOf(">");

        const slotDirective = node.props.find(
          (x) => x.type === NodeTypes.DIRECTIVE && x.name === "slot"
        );

        if (slotDirective) {
        } else {
          const slots = retrieveSlotNamed(node);

          context.s.prependLeft(
            tagBlockEnd,
            withNarrowCondition(
              [
                " v-slot={(ComponentInstance)=>{",
                "const $slots = ComponentInstance.$slots;",
              ],
              context
            )
          );

          const orphans: TemplateChildNode[] = [];
          for (const slot in slots) {
            const definition = slots[slot];
            if (!definition.templateNode) {
              orphans.push(...definition.items);
              continue;
            }

            renderSlot(
              context,
              tagBlockEnd,
              definition.items,
              definition.templateNode
            );
          }

          if (orphans.length) {
            renderSlot(context, tagBlockEnd, orphans);
          }
          context.s.appendRight(tagBlockEnd, "\n}}");
        }
      }
    }
  },
});

function resolveTag(
  node: VerterNode & { type: NodeTypes.ELEMENT },
  context: TranspileContext
) {
  switch (node.tagType) {
    case ElementTypes.COMPONENT: {
      const camel = camelize(node.tag);
      // if not camel just return
      if (camel === node.tag) {
        return node.tag;
      }
      // NOTE probably this is not 100% correct, maybe we could check if the component exists
      // by passing in the context
      return capitalize(camel);
    }
    case ElementTypes.TEMPLATE: {
      return context.accessors.template;
    }
    case ElementTypes.SLOT: {
      return context.accessors.slot;
    }
    default: {
      return node.tag;
    }
  }
}

function checkWebComponent(
  tag: string,
  resolvedTag: string,
  context: TranspileContext
) {
  const webComponents = context.webComponents;
  return webComponents.includes(tag) || webComponents.includes(resolvedTag);
}

function retrieveSlotNamed(node: ComponentNode) {
  const map: Record<
    number,
    {
      templateNode?: TemplateNode;
      items: TemplateChildNode[];
    }
  > = {};

  let s = 1;

  const buffer: TemplateChildNode[] = [];
  for (let i = 0; i < node.children.length; i++) {
    const element = node.children[i];
    if (element.type === NodeTypes.COMMENT) {
      buffer.push(element);
      continue;
    }
    if (element.type !== NodeTypes.ELEMENT) {
      buffer.push(element);
      continue;
    }
    if (element.tagType !== ElementTypes.TEMPLATE) {
      buffer.push(element);
      continue;
    }
    // todo resolve name, probably from arg
    const slotProp = getSlotProp(element);
    if (!slotProp) {
      buffer.push(element);
      continue;
    }

    const items: TemplateChildNode[] = [];
    while (buffer.length > 0) {
      if (buffer.at(-1).type === NodeTypes.COMMENT) {
        items.push(buffer.pop());
      } else {
        sealBuffer();
      }
    }
    sealBuffer(s++, element, items);
  }

  function sealBuffer(
    index: number = 0,
    templateNode?: TemplateNode,
    items: TemplateChildNode[] = []
  ) {
    if (!map[index]) {
      map[index] = {
        templateNode,
        items,
      };
    }

    if (!templateNode) {
      map[index].items.push(...buffer);
    }
    // clear the buffer
    buffer.length = 0;
  }

  // NO slots found, default to `default`
  if (buffer.length) {
    sealBuffer();
  }

  return map;
}

function processExpression(exp: ExpressionNode, context: TranspileContext) {
  switch (exp.type) {
    case NodeTypes.SIMPLE_EXPRESSION: {
      if (exp.ast) {
        debugger;
        // todo retrieve ast
        return;
      }
      if (exp.isStatic) {
        return exp.content;
      }

      return appendCtx(exp, context);
    }
    case NodeTypes.COMPOUND_EXPRESSION: {
      if (exp.ast) {
        // todo retrieve ast
        return;
      }
      return exp.identifiers[0];
    }
  }
}
function getSlotProp(node: BaseElementNode): DirectiveNode | null | undefined {
  return node.props.find(
    (x) => x.type === NodeTypes.DIRECTIVE && x.name === "slot"
  ) as DirectiveNode | undefined;
}

function renderSlot(
  context: TranspileContext,
  insertAt: number,
  items: TemplateChildNode[],
  template?: TemplateNode | null
) {
  const { s } = context;

  const narrowCondition = generateNarrowCondition(context, true);
  if (template) {
    const slotProp = getSlotProp(template);
    if (!slotProp) {
      // TODO log error
      console.error("VERTER  v-slot or # expected!");
      return;
    }

    // context.s.appendLeft(
    //   insertAt,
    //   ["", `{${context.accessors.slotCallback}(`].join("\n")
    // );

    const prepend = ["", `{${context.accessors.slotCallback}(`].join("\n");

    // s.prependLeft(
    //   slotProp.loc.start.offset,
    //   ["", `{${context.accessors.slotCallback}(`].join("\n")
    // );

    if (slotProp.arg) {
      processExpression(slotProp.arg, context);
    }

    if (slotProp.rawName.startsWith("#")) {
      s.overwrite(
        slotProp.loc.start.offset,
        slotProp.loc.start.offset + 1,
        `$slots${slotProp.rawName[1] === "[" ? "" : "."}`
      );

      s.appendRight(slotProp.loc.start.offset, prepend);

      s.move(slotProp.loc.start.offset, slotProp.loc.end.offset, insertAt);

      s.prependLeft(
        slotProp.loc.end.offset,
        `)${slotProp.exp ? " " : `(()=>{\n${narrowCondition}\n`}`
      );
    } else {
      // v-slot:
      s.overwrite(
        slotProp.loc.start.offset,
        slotProp.loc.start.offset + "v-slot".length + (slotProp.arg ? 1 : 0),
        `$slots${
          slotProp.arg
            ? slotProp.rawName["v-slot".length + 1] === "["
              ? ""
              : "."
            : ".default"
        }`
      );
      s.appendRight(slotProp.loc.start.offset, prepend);

      s.move(slotProp.loc.start.offset, slotProp.loc.end.offset, insertAt);

      s.prependLeft(
        slotProp.loc.end.offset,
        `)${slotProp.exp ? " " : `(()=>{\n${narrowCondition}`}`
      );
    }
    if (slotProp.exp) {
      // replace = with )(
      s.overwrite(
        slotProp.exp.loc.start.offset - 2,
        slotProp.exp.loc.start.offset - 1,
        ")("
      );

      // replace delimeters with ( )
      s.overwrite(
        slotProp.exp.loc.start.offset - 1,
        slotProp.exp.loc.start.offset,
        "("
      );
      s.overwrite(
        slotProp.exp.loc.end.offset,
        slotProp.exp.loc.end.offset + 1,
        ")"
      );

      s.appendLeft(slotProp.exp.loc.end.offset + 1, `=>{\n${narrowCondition}`);
    }
  } else {
    context.s.appendLeft(
      insertAt,
      [
        "",
        `{${context.accessors.slotCallback}($slots.default)(()=>{`,
        narrowCondition,
        "",
      ].join("\n")
    );
  }

  const start = Math.min(
    ...items.map((x) => x.loc.start.offset),
    template?.loc.start.offset ?? Number.MAX_VALUE
  );
  const end = Math.max(
    ...items.map((x) => x.loc.end.offset),
    template?.loc.end.offset ?? Number.MIN_VALUE
  );

  if (start === end) {
    console.warn("NOT expected slots to be the same!!!");
    return;
  }

  s.move(start, end, insertAt);

  // context.s.appendRight(insertAt, `\n})}\n`);
  context.s.prependLeft(end, `\n})}\n`);
}

// SLOT CALLBACK DEFINIITON
// declare function ___VERTER___SLOT_CALLBACK<T>(slot: (...args: T[]) => any): (cb: ((...args: T[]) => any))
