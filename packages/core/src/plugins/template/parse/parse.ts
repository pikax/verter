import {
  AttributeNode,
  DirectiveNode,
  ElementNode,
  ElementTypes,
  ExpressionNode,
  ForParseResult,
  RootNode,
  TemplateChildNode,
} from "@vue/compiler-core";
import { NodeTypes } from "@vue/compiler-core";

export interface ParseContext {
  root: RootNode;

  parent: TemplateChildNode | undefined;

  prev: TemplateChildNode | undefined;
  next: TemplateChildNode | undefined;
}

interface ParsedElement {
  tag?: string;
  node: TemplateChildNode;

  children: ParsedElement[];
}

type NativeDirectives =
  | "v-text"
  | "v-html"
  | "v-show"
  | "v-if"
  | "v-else-if"
  | "v-else"
  | "v-for"
  | "v-on"
  | "v-bind"
  | "v-model"
  | "v-slot"
  | "v-pre"
  | "v-cloak"
  | "v-once";
interface ParsedDirective {
  directive: string;

  /**
   * Wraps the children on this directive, this is used for
   * v-if, v-else-if, v-else, v-for, v-slot
   */
  wrap: boolean;

  rawName: string | undefined | NativeDirectives;

  parent: ElementNode;
  node: DirectiveNode;
  argument: ExpressionNode | undefined;
  expression: ExpressionNode | undefined;

  for: ForParseResult | undefined;

  children: ParsedElement[] | undefined;
}

interface ParsedIf extends ParsedDirective {
  if: "if" | "else-if" | "else";
}

interface ParsedFor {
  for: ForParseResult;
}

interface ParsedAttribute {
  node: AttributeNode;
  parent: ElementNode;

  name: string;
  value: string | undefined;
}

export function parse(root: RootNode) {
  const context: ParseContext = {
    root,
    parent: undefined,
    prev: undefined,
    next: undefined,
  };

  const children = groupConditions(
    root.children.map((child) => parseNode(child, context))
  );

  return {
    node: root,
    children,
  };
}

export enum ParsedType {
  PartialElement = "partial-element",
  Element = "element",
  Attribute = "attribute",
  Directive = "directive",
  Template = "template",
  Text = "text",
  Comment = "comment",
  Interpolation = "interpolation",
  Condition = "condition",
  For = "for",
  RenderSlot = "render-slot",
  Slot = "slot",
}

export type ParsedNodeBase = {
  type: ParsedType;
  node?: TemplateChildNode;
  content?: any;
  children?: ParsedNodeBase[];
};

const ExtractTagRegex = /^\s*<(?<tag>[^\s]*)/g;

function parseNode(
  node: TemplateChildNode,
  context: ParseContext
): ParsedNodeBase {
  switch (node.type) {
    case NodeTypes.ELEMENT: {
      return parseElement(node, context);
    }
    case NodeTypes.TEXT: {
      if (node.content.trimStart().startsWith("<")) {
        return {
          type: ParsedType.PartialElement,
          content: node.content,
          children: [],
          node: node,
          tag: node.content.match(ExtractTagRegex).groups?.tag,
        };
      }

      return {
        type: ParsedType.Text,
        node,
        content: node.content,
      };
    }
    case NodeTypes.COMMENT: {
      return {
        type: ParsedType.Comment,
        node,
        content: node.content,
      };
    }
    case NodeTypes.INTERPOLATION: {
      return {
        type: ParsedType.Interpolation,
        node,
        content: node.content,
      };
    }
    case NodeTypes.COMPOUND_EXPRESSION: {
      break;
    }
    case NodeTypes.IF: {
      break;
    }
    case NodeTypes.IF_BRANCH: {
      break;
    }
    case NodeTypes.FOR: {
      break;
    }
    case NodeTypes.TEXT_CALL: {
      break;
    }
    default: {
      // @ts-expect-error unknown type
      throw new Error(`Unknown node type ${node.type}`);
    }
  }

  return {
    type: ParsedType.Comment,
    node,
    content: "NOT FOUND",
  };
}

export function parseElement(
  node: ElementNode,
  context: ParseContext
): ParsedElement {
  const children = groupConditions(
    node.children.map((child) => parseNode(child, context))
  );

  const { props, wrappers, childWrappers } = splitProps(
    node.props.map((prop) => parseProps(prop, node, context))
  );

  // todo if there's a ref, add it to the context to
  // be able to automatically infer if needed

  let content = children;
  for (const it of childWrappers) {
    content = [
      {
        ...it,
        children: content,
      },
    ];
  }

  const currentNode = {
    type:
      node.tagType === ElementTypes.SLOT ? ParsedType.Slot : ParsedType.Element,
    tag: node.tag,
    node,
    children: content,
    props,
  };

  if (wrappers.length) {
    const processed = wrappers.reduce((prev, cur) => {
      return {
        ...cur,
        children: [prev],
      };
    }, currentNode);

    return processed;
  }

  return currentNode;
}

function groupConditions(elements: ParsedNodeBase[]) {
  const children: ParsedNodeBase[] = [];

  const ifs: ParsedNodeBase[] = [];
  let lastIfIndex = -1;

  function addCondition() {
    children.push({
      type: ParsedType.Condition,
      conditions: [...ifs],
    });
    ifs.length = 0;
    lastIfIndex = -1;
  }

  for (let i = 0; i < elements.length; i++) {
    const it = elements[i];
    const rawName = it?.rawName;
    switch (rawName) {
      case "v-if": {
        if (lastIfIndex !== -1) {
          addCondition();
        }
        lastIfIndex = i;
        ifs.push(it);
        break;
      }
      case "v-else-if": {
        if (lastIfIndex === -1) {
          // TODO error
          throw new Error("invalid v-else-if or v-else");
        }
        ifs.push(it);
        break;
      }
      case "v-else": {
        if (lastIfIndex === -1) {
          // TODO error
          throw new Error("invalid v-else-if or v-else");
        }
        ifs.push(it);
      }
      // TODO ignore comments
      default: {
        // add the comment to ifs, that way it will be appended coorectly
        if (it?.type === "comment") {
          if (lastIfIndex !== -1) {
            ifs.push(it);
            continue;
          }
        }

        if (lastIfIndex !== -1) {
          addCondition();
        } else {
          children.push(it);
        }
        break;
      }
    }
  }
  if (lastIfIndex !== -1) {
    addCondition();
  }
  return children;
}

function splitProps(attrs: Array<ParsedAttribute | ParsedDirective>) {
  const props: Array<ParsedAttribute | ParsedDirective> = [];
  const wrappers: Array<ParsedDirective & { wrap: true }> = [];

  const childWrappers: Array<ParsedDirective> = [];

  for (const it of attrs) {
    if ("wrap" in it && it.wrap) {
      // push it to the end
      wrappers.push({
        type: it.for ? ParsedType.For : ParsedType.Condition,
        ...(it as ParsedDirective & { wrap: true }),
      });
    } else if ((it as ParsedDirective).rawName === "v-slot") {
      childWrappers.push({
        type: ParsedType.RenderSlot,
        ...(it as ParsedDirective),
      });
    } else {
      props.push(it);
    }
  }

  // just place the v-if before the rest
  wrappers.sort((a, b) => {
    if (a.rawName !== b.rawName) {
      if (
        a.rawName === "v-if" ||
        a.rawName === "v-else-if" ||
        a.rawName === "v-else"
      ) {
        return 1;
      }
      if (
        b.rawName === "v-if" ||
        b.rawName === "v-else-if" ||
        b.rawName === "v-else"
      ) {
        return -1;
      }
    }
    return 0;
  });

  return {
    props,
    wrappers,
    childWrappers,
  };
}

export function parseProps(
  prop: AttributeNode | DirectiveNode,
  node: ElementNode,
  context: ParseContext
) {
  switch (prop.type) {
    case NodeTypes.ATTRIBUTE: {
      return {
        node: prop,
        parent: node,

        name: prop.name,
        value: prop.value?.content ?? undefined,
      } satisfies ParsedAttribute;
    }
    case NodeTypes.DIRECTIVE: {
      return {
        node: prop,
        parent: node,

        directive: prop.name,
        rawName: prop.rawName,
        argument: prop.arg,
        expression: prop.exp,

        wrap: isWrapable(prop),
        for: prop.forParseResult,
        children: undefined,
      } satisfies ParsedDirective;
    }
    default: {
      // @ts-expect-error unknown type
      throw new Error(`Unknown attribute type ${prop.type}`);
    }
  }
}

const WrapableDirectives = new Set<string>([
  "v-if",
  "v-else-if",
  "v-else",
  "v-for",
] as NativeDirectives[]);
export function isWrapable(node: DirectiveNode) {
  if (node.rawName) {
    return WrapableDirectives.has(node.rawName);
  }
  return false;
}
