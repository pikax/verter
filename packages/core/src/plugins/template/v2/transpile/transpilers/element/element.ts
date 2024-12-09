import {
  BaseElementNode,
  ComponentNode,
  DirectiveNode,
  ElementTypes,
  NodeTypes,
  TemplateChildNode,
  TemplateNode,
  AttributeNode,
  ElementNode,
  isFunctionType,
  walkIdentifiers,
} from "@vue/compiler-core";
import {
  appendCtx,
  createTranspiler,
  generateNarrowCondition,
  processExpression,
  withNarrowCondition,
} from "../../utils.js";
import { TranspileContext } from "../../types.js";
import { VerterNode } from "../../../walk/index.js";
import { camelize, capitalize } from "@vue/shared";
import { ElementType, LocationType } from "../../../../../types.js";

export default createTranspiler(NodeTypes.ELEMENT, {
  enter(node, parent, context, parentContext) {
    const { s, declarations } = context;

    const tagOffset = node.loc.source.indexOf(node.tag, 0);
    const tag = resolveTag(node, context);

    const isWebComponent =
      node.tagType === ElementTypes.COMPONENT &&
      checkWebComponent(node.tag, tag, context);

    const tagIndex = node.loc.start.offset + tagOffset;
    if (
      !isWebComponent &&
      node.tagType !== ElementTypes.SLOT &&
      tag !== node.tag
    ) {
      s.overwrite(tagIndex, tagIndex + node.tag.length, tag);
    }

    const overrideContext = processProps(
      node,
      context,
      node,
      parent,
      parentContext
    );

    switch (node.tagType) {
      case ElementTypes.COMPONENT: {
        if (!isWebComponent && context.accessors.comp) {
          s.appendLeft(tagIndex, context.accessors.comp + ".");
        }
        break;
      }
      case ElementTypes.ELEMENT: {
        break;
      }
      case ElementTypes.SLOT: {
        processSlot(node, context, parentContext);
        break;
      }
      case ElementTypes.TEMPLATE: {
        break;
      }
    }

    // SLOT HANDLING
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
        ) as DirectiveNode | undefined;

        if (slotDirective?.exp) {
          const positionIndex = slotDirective.exp.loc.end.offset;

          const slots = retrieveSlotNamed(node);
          const orphans: TemplateChildNode[] = [];
          const slotsEnd: number[] = [];

          for (const slot in slots) {
            const definition = slots[slot];
            if (!definition.templateNode) {
              orphans.push(...definition.items);
              continue;
            }

            const slotEnd = renderSlot(
              overrideContext,
              positionIndex,
              definition.items,
              parentContext,
              definition.templateNode
            );
            slotsEnd.push(slotEnd);
          }

          if (orphans.length) {
            const slotEnd = renderSlot(
              overrideContext,
              positionIndex,
              orphans,
              parentContext,
              slotDirective
            );
            slotsEnd.push(slotEnd);
          }
          s.appendRight(positionIndex, "\n}}");
          // @ts-expect-error
          parentContext.slotsEnd = slotsEnd;
        } else {
          const slots = retrieveSlotNamed(node);

          const positionIndex = slotDirective?.loc.end.offset || tagBlockEnd;

          if (slotDirective) {
            s.appendLeft(
              positionIndex,
              withNarrowCondition(
                [
                  `={(${context.accessors.componentInstance}): any=>{`,
                  `const $slots = ${context.accessors.componentInstance}.$slots;`,
                ],
                context
              )
            );
          } else {
            s.appendLeft(
              positionIndex,
              withNarrowCondition(
                [
                  ` v-slot={(${context.accessors.componentInstance}): any=>{`,
                  `const $slots = ${context.accessors.componentInstance}.$slots;`,
                ],
                context
              )
            );
          }

          const orphans: TemplateChildNode[] = [];
          const slotsEnd: number[] = [];

          for (const slot in slots) {
            const definition = slots[slot];
            if (!definition.templateNode) {
              orphans.push(...definition.items);
              continue;
            }

            const slotEnd = renderSlot(
              context,
              positionIndex,
              definition.items,
              parentContext,
              definition.templateNode
            );
            slotsEnd.push(slotEnd);
          }

          if (orphans.length) {
            const slotEnd = renderSlot(
              context,
              positionIndex,
              orphans,
              parentContext
            );
            slotsEnd.push(slotEnd);
          }
          s.appendRight(positionIndex, "\n}}");
          // @ts-expect-error
          parentContext.slotsEnd = slotsEnd;
        }
      }
    }

    // declartion
    const directives = node.props.filter((x) => x.type === NodeTypes.DIRECTIVE);

    const props = node.props.reduce((p, c) => {
      const name =
        c.type === NodeTypes.ATTRIBUTE || c.name === "slot"
          ? c.name
          : c.arg && c.arg.type == NodeTypes.SIMPLE_EXPRESSION
          ? c.name === "on"
            ? "on" + capitalize(c.arg.content)
            : c.arg.content
          : c.name;
      p[name] = c;
      return p;
    }, {});

    const refProp = props["ref"];

    declarations.push({
      type: LocationType.Element,
      // TODO fix this type
      node: node as ElementNode as any,
      tag,
      elementType:
        tag === "component"
          ? ElementType.dynamic
          : getElementType(node, isWebComponent),

      multiple:
        !!directives.find((x) => x.name === "for") || parentContext.inFor,
      conditional:
        !!directives.find(
          (x) => x.name === "if" || x.name === "else" || x.name === "else-if"
        ) || parentContext.conditions?.length > 0,

      ref:
        !refProp || refProp.type === NodeTypes.DIRECTIVE
          ? undefined
          : refProp.value.content,
      refExp:
        !refProp || refProp.type === NodeTypes.ATTRIBUTE
          ? undefined
          : refProp.exp,

      props,
    });

    return {
      ...context,
      ...overrideContext,
    };
  },
  leave(node, parent, context, parentContext) {
    const { s } = context;
    const tag = resolveTag(node, context);

    const isWebComponent =
      node.tagType === ElementTypes.COMPONENT &&
      checkWebComponent(node.tag, tag, context);
    // update the endTag
    if (!node.isSelfClosing) {
      if (node.tagType === ElementTypes.SLOT) {
        const endTagIndex =
          node.loc.start.offset + node.loc.source.lastIndexOf(node.tag);
        if (tag !== node.tag) {
          s.overwrite(
            endTagIndex,
            endTagIndex + node.tag.length,
            "RENDER_SLOT"
          );
        }
      } else if (!isWebComponent) {
        const endTagIndex =
          node.loc.start.offset + node.loc.source.lastIndexOf(node.tag);

        // check if the component has a closing tag, otherwise we don't need to update
        if (endTagIndex > node.loc.start.offset + 1) {
          if (tag !== node.tag) {
            s.overwrite(endTagIndex, endTagIndex + node.tag.length, tag);
          }
          if (
            node.tagType === ElementTypes.COMPONENT &&
            context.accessors.comp
          ) {
            s.appendLeft(endTagIndex, context.accessors.comp + ".");
          }
        }
      }
    }

    if (
      node.tagType === ElementTypes.SLOT &&
      !parentContext.conditionBlock &&
      !parentContext.for
    ) {
      s.prependLeft(node.loc.end.offset, "}}");
    }
    // if we are in a condition block and are the last element close the block
    if (
      parentContext.conditionBlock &&
      "children" in parent &&
      parent.children.at(-1) === node
    ) {
      s.appendLeft(node.loc.end.offset, "}}");
    }

    if (parentContext.slotsEnd?.length) {
      for (const slotEnd of parentContext.slotsEnd) {
        closeSlot(context, slotEnd);
      }
      // clear slotsEnd to prevent from being resolved multiple times
      parentContext.slotsEnd.length = 0;
    }
    // if (parentContext.orphansEnd) {
    //   closeSlot(context, parentContext.orphansEnd);
    // }
  },
});

function getElementType(node: ElementNode, isWebComponent: boolean) {
  if (isWebComponent) {
    return ElementType.webcomponent;
  }
  switch (node.tagType) {
    case ElementTypes.COMPONENT: {
      return ElementType.component;
    }
    case ElementTypes.ELEMENT: {
      return ElementType.native;
    }
    case ElementTypes.SLOT: {
      return ElementType.slot;
    }
    case ElementTypes.TEMPLATE: {
      return ElementType.template;
    }
  }
}

function processSlot(
  node: VerterNode & { type: NodeTypes.ELEMENT; tagType: ElementTypes.SLOT },
  context: TranspileContext,
  parentContext: Record<string, any>
) {
  const { s } = context;
  const name = node.props.find((x) =>
    x.type === NodeTypes.DIRECTIVE
      ? x.arg && x.arg.type === NodeTypes.SIMPLE_EXPRESSION
        ? x.arg.content === "name"
        : undefined
      : x.name === "name"
  );
  s.remove(node.loc.start.offset, node.loc.start.offset + 1);

  if (parentContext.conditionBlock) {
    s.prependLeft(
      node.loc.start.offset + 1,
      `const RENDER_SLOT = ${context.accessors.slotToComponent}(${context.accessors.slot}`
    );
  } else if (parentContext.for) {
    s.prependLeft(
      node.loc.start.offset + 1,
      `const RENDER_SLOT = ${context.accessors.slotToComponent}(${context.accessors.slot}`
    );
  } else {
    // append { ()=>
    s.prependLeft(
      node.loc.start.offset + 1,
      [
        "{()=>{",
        generateNarrowCondition(
          parentContext.conditions?.length > 0
            ? {
                ...context,
                conditions: {
                  ...context.conditions,
                  ifs: [...context.conditions.ifs, ...parentContext.conditions],
                },
              }
            : context,
          true
        ),
        `const RENDER_SLOT = ${context.accessors.slotToComponent}(${context.accessors.slot}`,
      ].join("\n")
    );
  }

  if (name) {
    s.move(
      name.loc.start.offset,
      name.loc.end.offset,
      node.loc.start.offset + 1
    );
    s.appendLeft(node.loc.start.offset + 1, "[");
    if (name.type === NodeTypes.ATTRIBUTE) {
      s.remove(name.loc.start.offset, name.loc.start.offset + "name=".length);
    } else {
      s.remove(
        name.loc.start.offset,
        name.loc.start.offset + "name={".length + 1
      );

      s.remove(name.exp.loc.end.offset, name.exp.loc.end.offset + 1);
    }

    s.appendRight(node.loc.start.offset + 1, "]);\n");
  } else {
    s.appendRight(node.loc.start.offset + 1, ".default);\n");
  }

  s.appendRight(node.loc.start.offset + 1, "return <");

  s.overwrite(
    node.loc.start.offset + 1,
    node.loc.start.offset + 1 + "slot".length,
    "RENDER_SLOT",
    {
      contentOnly: true,
    }
  );
}

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

function getSlotProp(node: BaseElementNode): DirectiveNode | null | undefined {
  return node.props.find(
    (x) => x.type === NodeTypes.DIRECTIVE && x.name === "slot"
  ) as DirectiveNode | undefined;
}

function renderSlot(
  context: TranspileContext,
  insertAt: number,
  items: TemplateChildNode[],
  parentContext: Record<string, any>,
  template?: TemplateNode | DirectiveNode | null
) {
  const { s } = context;

  const narrowCondition = generateNarrowCondition(
    parentContext.conditions?.length > 0
      ? {
          ...context,
          conditions: {
            ...context.conditions,
            ifs: [...context.conditions.ifs, ...parentContext.conditions],
          },
        }
      : context,
    true
  );
  if (template?.type === NodeTypes.DIRECTIVE) {
    // processExpression(template.exp, context, true, false);

    const identifiers = new Set<string>();
    if (template.exp.ast) {
      walkIdentifiers(
        template.exp.ast,
        (id, parent, _, isReference, isLocal) => {
          if (id.type === "Identifier" && isReference && isLocal) {
            identifiers.add(id.name);
          }
        },
        true
      );
    }
    context.ignoredIdentifiers = [
      ...context.ignoredIdentifiers,
      ...identifiers,
    ];
    const slotProp = template;

    // remove delimiters
    s.remove(slotProp.exp.loc.start.offset - 1, slotProp.exp.loc.start.offset);
    s.remove(slotProp.exp.loc.end.offset, slotProp.exp.loc.end.offset + 1);

    s.appendLeft(
      slotProp.exp.loc.start.offset,
      [
        `{(${context.accessors.componentInstance}): any=>{`,
        `const $slots = ${context.accessors.componentInstance}.$slots;`,
        `{${context.accessors.slotCallback}($slots.default)((`,
      ].join("\n")
    );

    s.appendLeft(slotProp.exp.loc.end.offset, `)=>{ <> \n${narrowCondition}`);
  } else if (template) {
    const slotProp = getSlotProp(template);
    if (!slotProp) {
      // TODO log error
      console.error("VERTER  v-slot or # expected!");
      return;
    }

    const prepend = ["", `{${context.accessors.slotCallback}(`].join("\n");

    if (slotProp.arg) {
      processExpression(slotProp.arg, context);
    }

    const shouldWrapName = slotProp.rawName.startsWith("#")
      ? slotProp.rawName.indexOf("-") >= 0 && slotProp.rawName[1] !== "["
      : slotProp.arg && "content" in slotProp.arg
      ? slotProp.arg.content.indexOf("-") >= 0 &&
        slotProp.arg.content[1] !== "["
      : false;
    slotProp.rawName.indexOf("-") >= 0 && slotProp.rawName[1] !== "[";
    if (slotProp.rawName.startsWith("#")) {
      s.overwrite(
        slotProp.loc.start.offset,
        slotProp.loc.start.offset + 1,
        `$slots${
          slotProp.rawName[1] === "[" ? "" : shouldWrapName ? "['" : "."
        }`
      );

      s.appendRight(slotProp.loc.start.offset, prepend);

      s.move(slotProp.loc.start.offset, slotProp.loc.end.offset, insertAt);

      s.prependLeft(
        slotProp.loc.end.offset,
        `${shouldWrapName ? "']" : ""})${
          slotProp.exp ? " " : `(()=>{<>\n${narrowCondition}\n`
        }`
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
              : shouldWrapName
              ? "['"
              : "."
            : ".default"
        }`
      );
      s.appendRight(slotProp.loc.start.offset, prepend);

      // prevent moving inside
      if (
        insertAt < slotProp.loc.start.offset ||
        insertAt > slotProp.loc.end.offset
      ) {
        s.move(slotProp.loc.start.offset, slotProp.loc.end.offset, insertAt);
      }

      s.prependLeft(
        slotProp.loc.end.offset,
        `${shouldWrapName ? "']" : ""})${
          slotProp.exp ? " " : `(()=>{<>\n${narrowCondition}`
        }`
      );
    }
    if (slotProp.exp) {
      // replace = with )(
      s.overwrite(
        slotProp.exp.loc.start.offset - 2,
        slotProp.exp.loc.start.offset - 1,
        `${shouldWrapName ? "']" : ""})(`
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

      s.appendLeft(
        slotProp.exp.loc.end.offset + 1,
        `=>{ <> \n${narrowCondition}`
      );
    }
  } else {
    context.s.appendLeft(
      insertAt,
      [
        "",
        `{${context.accessors.slotCallback}($slots.default)(()=>{ <>`,
        narrowCondition,
        "",
      ].join("\n")
    );
  }

  const start = Math.min(
    ...items.map((x) => x.loc.start.offset),
    template?.type === NodeTypes.DIRECTIVE
      ? Number.MAX_VALUE
      : template?.loc.start.offset ?? Number.MAX_VALUE
  );
  const end = Math.max(
    ...items.map((x) => x.loc.end.offset),
    template?.type === NodeTypes.DIRECTIVE
      ? Number.MIN_VALUE
      : template?.loc.end.offset ?? Number.MIN_VALUE
  );

  if (start === end) {
    console.warn("NOT expected slots to be the same!!!");
    return;
  }
  // prevent moving inside
  s.move(start, end, insertAt);

  // context.s.appendRight(end, `\n})}\n`);

  return end;
}

function closeSlot(context: TranspileContext, end: number) {
  const { s } = context;
  s.appendLeft(end, `\n</>})}\n`);
}

function sanitiseAttributeName(name: string, context: TranspileContext) {
  if (context.attributes.camelWhitelist.some((x) => name.startsWith(x))) {
    return name;
  }
  return camelize(name);
}

function processProps(
  node: VerterNode & { type: NodeTypes.ELEMENT },
  context: TranspileContext,
  parent: ElementNode,
  parentParent: VerterNode,
  parentContext: Record<string, any> & {
    conditionBlock?: DirectiveNode | AttributeNode | null;
    conditions?: Array<string> | null;
  }
) {
  const shouldCamel = node.tagType !== ElementTypes.ELEMENT;

  const toNormalise = {
    styles: [] as Array<DirectiveNode | AttributeNode>,
    classes: [] as Array<DirectiveNode | AttributeNode>,
  };

  let overrideContext = {
    ...context,
    conditions: {
      ...context.conditions,
      ifs: [...context.conditions.ifs],
    },
  };

  const IfMap = {
    if: "if",
    else: "else",
    "else-if": "else if",
  };

  const sorted = [...node.props].sort((a, b) => {
    let av =
      a.type === NodeTypes.DIRECTIVE
        ? IfMap[a.name]
          ? 9000
          : a.name === "for"
          ? 100
          : 1
        : 1;
    let bv =
      b.type === NodeTypes.DIRECTIVE
        ? IfMap[b.name]
          ? 9000
          : b.name === "for"
          ? 100
          : 1
        : 1;
    return bv - av;
  });

  const conditionDirective = sorted.find(
    (x) => x.type === NodeTypes.DIRECTIVE && IfMap[x.name]
  );
  // add initial block
  if (conditionDirective?.name === "if") {
    if (parentContext.conditionBlock && "children" in parentParent) {
      // get sibling

      const i = parentParent.children.indexOf(parent);

      const prevSibbling = parentParent.children[i - 1];
      // already in block close the previous one
      context.s.prependLeft(prevSibbling.loc.end.offset, "}}");
    }

    parentContext.conditions = [];
    parentContext.conditionBlock = conditionDirective;

    // // wrap { }
    // s.prependRight(
    //   prop.loc.start.offset,
    //   withNarrowCondition("{ ()=> {", context)
    // );
    // s.prependLeft(parent.loc.end.offset, "}}}");
  } else if (conditionDirective) {
    parentContext.conditionBlock = conditionDirective;
  } else if (parentContext.conditionBlock && "children" in parentParent) {
    // already in block close the previous one
    const i = parentParent.children.indexOf(parent);
    const prevSibbling = parentParent.children[i - 1];
    // already in block close the previous one
    context.s.prependLeft(prevSibbling.loc.end.offset, "}}");

    // since we handle set the condition to undefined
    parentContext.conditionBlock = undefined;
  }

  const conditions: string[] = [];

  for (let i = 0; i < sorted.length; i++) {
    const prop = sorted[i];
    const currentContext = mergeConditionToContext(
      overrideContext ?? context,
      conditions,
      parentContext.conditions
    );

    const name =
      "arg" in prop && prop.arg
        ? processExpression(prop.arg, currentContext, true)
        : prop.name;

    switch (name) {
      default: {
        const c = processProp(
          prop,
          currentContext,
          parent,
          parentParent,
          parentContext,
          shouldCamel
        );
        conditions.push(...c.conditions);

        if (c.ignoredIdentifiers.length) {
          overrideContext = {
            ...overrideContext,
            ignoredIdentifiers: [
              ...overrideContext.ignoredIdentifiers,
              ...c.ignoredIdentifiers,
            ],
          };
        }

        break;
      }

      case "slot": {
        break;
      }

      case "class": {
        toNormalise.classes.push(prop);
        break;
      }
      case "style": {
        toNormalise.styles.push(prop);
        break;
      }
    }
  }

  normaliseProps(
    "class",
    toNormalise.classes,
    overrideContext,
    parent,
    parentParent,
    parentContext
  );
  normaliseProps(
    "style",
    toNormalise.styles,
    overrideContext,
    parent,
    parentParent,
    parentContext
  );

  if (conditionDirective?.name === "if") {
    // wrap if
    context.s.prependRight(
      conditionDirective.loc.start.offset,
      withNarrowCondition("{ (): any => {", context)
    );

    parentContext.inConditionBlock = true;
  }

  if (parentContext.conditions) {
    if (conditionDirective?.name?.startsWith("else")) {
      parentContext.conditions = parentContext.conditions.map((x) => `(!${x})`);
    } else {
      parentContext.conditions.push(...conditions);
    }
  } else {
    parentContext.conditions = conditions;
  }

  return overrideContext;
}

function processProp(
  prop: AttributeNode | DirectiveNode,
  context: TranspileContext,
  parent: ElementNode,
  _parentParent: VerterNode,
  parentContext: Record<string, any>,
  camelise = false
) {
  const { s } = context;
  const identifiers: string[] = [];

  const returnItem: { ignoredIdentifiers: string[]; conditions: string[] } = {
    ignoredIdentifiers: [],
    conditions: [],
  };

  if (prop.type === NodeTypes.ATTRIBUTE) {
    if (camelise) {
      const cameled = sanitiseAttributeName(prop.name, context);
      if (cameled !== prop.name) {
        s.overwrite(
          prop.nameLoc.start.offset,
          prop.nameLoc.end.offset,
          cameled
        );
      }
    }
    return returnItem;
  }

  switch (prop.name) {
    case "bind": {
      const name = prop.arg ? processExpression(prop.arg, context) : undefined;
      const sanitisedName = name ? sanitiseAttributeName(name, context) : name;

      if (sanitisedName !== name) {
        s.overwrite(
          prop.arg.loc.start.offset,
          prop.arg.loc.end.offset,
          sanitisedName
        );
      }

      if (prop.exp) {
        processExpression(prop.exp, context);

        // update delimeters
        s.overwrite(
          prop.exp.loc.start.offset - 1,
          prop.exp.loc.start.offset,
          "{"
        );
        s.overwrite(prop.exp.loc.end.offset, prop.exp.loc.end.offset + 1, "}");
      } else if (prop.rawName.startsWith(":")) {
        if (prop.arg) {
          s.remove(prop.loc.start.offset, prop.loc.start.offset + 1);
          // short sugar syntax
          s.prependLeft(prop.loc.start.offset + 1, `${sanitisedName}={`);
          s.prependLeft(prop.loc.end.offset, "}");

          // append ctx to name
          appendCtx(prop.arg, context);
        } else {
          // this handles the empty bind ":"
          s.overwrite(prop.loc.start.offset, prop.loc.start.offset + 1, "{");
          s.appendRight(prop.loc.start.offset + 1, "}");
        }
      }

      if (prop.rawName.startsWith(":") && prop.exp) {
        s.remove(prop.loc.start.offset, prop.loc.start.offset + 1);
      } else if (prop.loc.source.startsWith("v-bind:")) {
        s.remove(
          prop.loc.start.offset,
          prop.loc.start.offset + "v-bind:".length
        );
      } else if (prop.loc.source.startsWith("v-bind=")) {
        s.overwrite(
          prop.loc.start.offset,
          prop.loc.start.offset + "v-bind='".length,
          "{..."
        );
      }

      break;
    }
    case "on": {
      const name = prop.arg ? processExpression(prop.arg, context) : "";
      const sanitisedName = capitalize(
        name ? sanitiseAttributeName(name, context) : name
      );

      if (sanitisedName !== name) {
        s.overwrite(
          prop.arg.loc.start.offset,
          prop.arg.loc.end.offset,
          sanitisedName
        );
      }

      if (name) {
        s.overwrite(prop.loc.start.offset, prop.loc.start.offset + 1, "on");
      } else {
        s.remove(prop.loc.start.offset, prop.loc.start.offset + 1);
        s.appendRight(prop.loc.start.offset + 1, "on");
      }

      // add callback

      if (prop.exp) {
        const exp = prop.exp;
        // update delimeters
        s.overwrite(exp.loc.start.offset - 1, exp.loc.start.offset, "{");
        s.overwrite(exp.loc.end.offset, exp.loc.end.offset + 1, "}");

        // append $event=>
        const has$event =
          s.original
            .slice(exp.loc.start.offset, exp.loc.end.offset)
            .indexOf("$event") >= 0;
        s.prependLeft(
          exp.loc.start.offset,
          `// @ts-ignore prevent "args" being considered any[]\n(...args)=>\n${
            context.accessors.eventCb
          }(args,${
            !exp.ast || !isFunctionType(exp.ast)
              ? `(${has$event ? "$event" : ""})=>`
              : ""
          }`
        );

        s.prependLeft(exp.loc.end.offset, ")");

        processExpression(exp, {
          ...context,
          ignoredIdentifiers: [...context.ignoredIdentifiers, "$event"],
        });
      }

      break;
    }
    case "slot": {
      // Handled by the parent
      break;
    }

    case "for": {
      if (!("forParseResult" in prop) || !prop.forParseResult) {
        console.error("expected valid for");
        debugger;
        break;
      }

      const { source, value, key, index } = prop.forParseResult;
      parentContext.for = prop.forParseResult;

      if (key) {
        const v = processExpression(key, context, true, false);
        identifiers.push(v);
      }

      if (value) {
        const v = processExpression(value, context, true, false);
        identifiers.push(v);
      }

      if (source) {
        processExpression(source, context);
      }

      returnItem.ignoredIdentifiers.push(...identifiers);

      if (index) {
        processExpression(value, {
          ...context,
          ignoredIdentifiers: [...context.ignoredIdentifiers, ...identifiers],
        });
      }

      // move v-for to beginning
      s.move(
        prop.loc.start.offset,
        prop.loc.end.offset,
        parent.loc.start.offset
      );

      // update delimiters
      s.overwrite(
        prop.exp.loc.start.offset - 1,
        prop.exp.loc.start.offset,
        "("
      );

      //remove end delimiter because is not needed
      s.remove(prop.exp.loc.end.offset, prop.exp.loc.end.offset + 1);
      // s.overwrite(prop.exp.loc.end.offset, prop.exp.loc.end.offset + 1, ")");

      // update v-for with renderList
      s.overwrite(
        prop.loc.start.offset,
        prop.exp.loc.start.offset,
        `${context.accessors.renderList}(`
      );

      // move source to the beginning
      s.move(
        source.loc.start.offset,
        source.loc.end.offset,
        prop.exp.loc.start.offset
      );

      // find in or of and replace with ,
      let inOfIndex = -1;
      const tokens = ["in", "of"];

      const fromIndex = Math.max(
        key?.loc.end.offset ?? 0,
        index?.loc.end.offset ?? 0,
        value?.loc.end.offset ?? 0
      );
      const condition = s.original.slice(fromIndex, source.loc.start.offset);

      for (let i = 0; i < tokens.length; i++) {
        const token = tokens[i];
        const index = condition.indexOf(token);
        if (index !== -1) {
          inOfIndex = index;
          break;
        }
      }

      // move , to start of expression
      s.move(
        fromIndex + inOfIndex,
        fromIndex + inOfIndex + 2,
        prop.exp.loc.start.offset
      );

      s.overwrite(fromIndex + inOfIndex, fromIndex + inOfIndex + 2, ",");

      // add => { after the exp

      s.appendRight(
        prop.exp.loc.end.offset,
        withNarrowCondition(" =>{ ", context)
      );

      // check if it needs to be wrapped
      if (prop.exp.loc.source.startsWith("{")) {
        s.prependRight(value.loc.start.offset, "(");
        s.appendLeft(value.loc.end.offset, ")");
      }

      // wrap { }
      s.prependRight(prop.loc.start.offset, "{");
      s.prependLeft(parent.loc.end.offset, "})}");

      break;
    }

    case "else":
    case "else-if":
    case "if": {
      // move v-if/else-if/else to the beginning
      s.move(
        prop.loc.start.offset,
        prop.loc.end.offset,
        parent.loc.start.offset
      );

      // remove v-
      s.remove(prop.loc.start.offset, prop.loc.start.offset + 2);

      if (prop.exp) {
        // remove =
        s.remove(
          prop.loc.start.offset + prop.rawName.length,
          prop.loc.start.offset + prop.rawName.length + 1
        );

        // update delimiters
        s.overwrite(
          prop.exp.loc.start.offset - 1,
          prop.exp.loc.start.offset,
          "("
        );
        s.overwrite(prop.exp.loc.end.offset, prop.exp.loc.end.offset + 1, ")");

        processExpression(prop.exp, context);
        s.prependLeft(prop.exp.loc.end.offset + 1, "{");

        const condition = s
          .snip(prop.exp.loc.start.offset, prop.exp.loc.end.offset)
          .toString();

        returnItem.conditions.push(condition);

        // overrideContext = {
        //   ...context,
        //   conditions: {
        //     ...context.conditions,
        //     elses: [...context.conditions.elses, ...parentContext.conditions],
        //     ifs: [...context.conditions.ifs, condition],
        //   },
        // };

        // (parentContext.conditions ?? (parentContext.conditions = [])).push(
        //   condition
        // );
      }

      // if (prop.name === "else") {
      //   // s.prependLeft(prop.loc.end.offset, withNarrowCondition("{\n", context));
      //   s.prependLeft(prop.loc.end.offset, "{\n");
      //   s.prependLeft(parent.loc.end.offset, "\n}");
      // } else {
      //   // // wrap { }
      //   // s.prependRight(
      //   //   prop.loc.start.offset,
      //   //   withNarrowCondition("{ ()=> {", context)
      //   // );
      //   // s.prependLeft(parent.loc.end.offset, "}}}");

      //   s.appendRight(parent.loc.end.offset, "}");

      //   // s.appendLeft(parentParent.loc.end.offset, "}");

      //   if (prop.name === "else-if") {
      //     const hiphenIndex = prop.loc.start.offset + "v-else".length;
      //     s.overwrite(hiphenIndex, hiphenIndex + 1, " ");
      //   }
      // }

      switch (prop.name) {
        case "if": {
          // if(parentContext.)``
          s.appendLeft(parent.loc.end.offset, "}");

          break;
        }
        case "else-if": {
          s.prependLeft(parent.loc.end.offset, "}");

          const hiphenIndex = prop.loc.start.offset + "v-else".length;
          s.overwrite(hiphenIndex, hiphenIndex + 1, " ");
          break;
        }
        case "else": {
          s.prependLeft(prop.loc.end.offset, "{\n");
          s.prependLeft(parent.loc.end.offset, "\n}");
          break;
        }
      }
      break;
    }

    case "show": {
      // TODO v-show
      break;
    }

    default: {
      // unknown
      // debugger;
      break;
    }
  }
  return returnItem;
}

function normaliseProps(
  type: "style" | "class",
  props: Array<DirectiveNode | AttributeNode>,
  context: TranspileContext,
  parent: ElementNode,
  parentParent: VerterNode,
  parentContext: Record<string, any>
) {
  if (props.length === 0) {
    return;
  }
  const { s } = context;
  const firstDirective = props.find(
    (x) => x.type === NodeTypes.DIRECTIVE
  ) as DirectiveNode;

  const accessor =
    type === "class"
      ? context.accessors.normalizeClass
      : context.accessors.normalizeStyle;

  let end = 0;

  if (firstDirective) {
    processProp(firstDirective, context, parent, parentParent, parentContext);
    if (props.length === 1) return;

    const start =
      firstDirective.exp?.loc.start.offset ?? firstDirective.arg.loc.end.offset;
    end =
      firstDirective.exp?.loc.end.offset ?? firstDirective.arg.loc.end.offset;

    if (firstDirective.exp) {
      s.prependLeft(start, `${accessor}([`);
      s.prependRight(end, "])");
    } else {
      // sugar

      s.overwrite(
        firstDirective.loc.start.offset,
        firstDirective.loc.start.offset + 1,
        `class={${accessor}([`,
        {}
      );

      s.prependRight(end, "])");
    }
  } else {
    // just attribute]
    return;
  }
  try {
    for (let i = 0; i < props.length; i++) {
      const prop = props[i];
      if (prop === firstDirective) continue;

      const loc =
        prop.type === NodeTypes.DIRECTIVE
          ? (prop.exp ?? prop.arg).loc
          : prop.value.loc;

      if (loc) {
        // tried to append, but the moving was breaking things
        s.overwrite(
          loc.start.offset,
          loc.start.offset + 1,
          `,${loc.source[0]}`
        );
        s.move(loc.start.offset, loc.end.offset, end);

        s.remove(prop.loc.start.offset, loc.start.offset);
        s.remove(loc.end.offset, prop.loc.end.offset);
      }

      if (prop.type === NodeTypes.DIRECTIVE) {
        processExpression(prop.exp, context);
      }
    }
  } catch (e) {
    console.error(e);
  }

  context.declarations.push({
    type: LocationType.Import,
    from: "vue",
    generated: true,
    node: undefined,
    items: [
      {
        name: type === "class" ? "normalizeClass" : "normalizeStyle",
        alias: accessor,
      },
    ],
  });
}

function mergeConditionToContext(
  context: TranspileContext,
  conditions: Array<string>,
  sibblingsConditions?: Array<string>
) {
  sibblingsConditions = sibblingsConditions ?? [];

  let { ifs, elses } = context.conditions;

  ifs = ifs.filter((x) => !sibblingsConditions.includes(x)).concat(conditions);
  elses = [...elses, ...sibblingsConditions];

  return {
    ...context,
    conditions: {
      ifs,
      elses,
    },
  };
}

// SLOT CALLBACK DEFINIITON
// declare function ___VERTER___SLOT_CALLBACK<T>(slot: (...args: T[]) => any): (cb: ((...args: T[]) => any))
