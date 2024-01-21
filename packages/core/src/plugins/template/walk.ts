import {
  AttributeNode,
  ComponentNode,
  DirectiveNode,
  ElementNode,
  ElementTypes,
  ExpressionNode,
  ForParseResult,
  NodeTypes,
  PlainElementNode,
  RootNode,
  TemplateChildNode,
} from "@vue/compiler-core";
import { MagicString } from "@vue/compiler-sfc";

import { camelize, capitalize } from "@vue/shared";

interface WalkContext {
  root: RootNode;

  parent: TemplateChildNode | RootNode;
  prev: TemplateChildNode | undefined;
  next: TemplateChildNode | undefined;
}

export function walkRoot(root: RootNode, magicString: MagicString) {
  return root.children
    .map((x, i, arr) =>
      walk(x, magicString, {
        root,
        parent: root,
        prev: i > 0 ? arr[i - 1] : undefined,
        next: i < arr.length - 1 ? arr[i + 1] : undefined,
      } satisfies WalkContext)
    )
    .join("\n");
}

export function walk(
  node: TemplateChildNode,
  magicString: MagicString,
  ctx: WalkContext
) {
  switch (node.type) {
    case NodeTypes.ELEMENT: {
      return walkElement(node, magicString, ctx);
    }
    case NodeTypes.TEXT: {
      return node.content;
    }
    case NodeTypes.COMMENT: {
      break;
    }
    case NodeTypes.INTERPOLATION: {
      break;
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
  return "";
}

export function walkElement(
  node: ElementNode,
  magicString: MagicString,
  ctx: WalkContext
): string {
  const childrenContent = node.children.map((x, i, arr) =>
    walk(x, magicString, {
      root: ctx.root,
      parent: node,
      prev: i > 0 ? arr[i - 1] : undefined,
      next: i < arr.length - 1 ? arr[i + 1] : undefined,
    } satisfies WalkContext)
  );
  const attrs = node.props.map((x) =>
    // I don't see where the attribute context would be necessary :thinking:
    walkAttribute(x, magicString, ctx)
  );

  const vfor = attrs.find((x) => !!x.for) as WalkAttributeFor | undefined;
  const vIf = attrs.find((x) => !!x.if) as WalkAttributeIf | undefined;
  const attributes = attrs.filter(
    (x) => x.attribute === true || x.binding === true
  ) as WalkAttributeAttr[];
  const directives = attrs.filter(
    (x) => x.directive === true && x.binding !== true
  ) as WalkAttributeDirective[];

  const content =
    childrenContent.length === 0
      ? ""
      : childrenContent.length > 1
      ? `[${childrenContent.join(",\n")}]`
      : childrenContent[0];

  switch (node.tagType) {
    case ElementTypes.ELEMENT:
    case ElementTypes.COMPONENT: {
      const tag = resolveComponentTag(node);

      const elementStr =
        tag === "template"
          ? content
          : `<${tag} ${attributes
              .map((x) => attributeToString(x, magicString))
              .join(" ")}>${content}</${tag}>`;

      // v-if has higher priority than v-for
      return wrapWithIf(
        wrapWithFor(elementStr, vfor, magicString, !!vIf, ctx),
        vIf,
        magicString,
        ctx
      );
    }
    case ElementTypes.SLOT: {
      break;
    }
    case ElementTypes.TEMPLATE: {
      const elementStr = childrenContent.length > 1 ? `${content}` : content;

      return wrapWithIf(
        wrapWithFor(elementStr, vfor, magicString, !!vIf, ctx),
        vIf,
        magicString,
        ctx
      );
    }
    default: {
      // @ts-expect-error unknown type
      throw new Error(`Unknown element type ${node.tagType}`);
    }
  }
  return "";
}

function resolveComponentTag(node: PlainElementNode | ComponentNode) {
  if (node.tagType === ElementTypes.COMPONENT) {
    const camel = camelize(node.tag);
    // if not camel just return
    if (camel === node.tag) return node.tag;
    // NOTE probably this is not 100% correct, maybe we could check if the component exists
    // by passing in the context
    return capitalize(camel);
  }
  return node.tag;
}

const IfMap = {
  if: "if",
  else: "else",
  "else-if": "else if",
};

function wrapWithIf(
  content: string,
  ifNode: WalkAttributeIf | undefined,
  magicString: MagicString,
  ctx: WalkContext
): string {
  if (!ifNode) return content;

  const isStart = ifNode.if === "if";
  const isEnd =
    (ctx.next as ElementNode)?.props?.find(
      (x) =>
        (x as DirectiveNode).rawName === "v-else" ||
        (x as DirectiveNode).rawName === "v-else-if"
    ) === undefined;

  if (ifNode.if === "else" || ifNode.if === "else-if") {
    const hasPreviousIf =
      (ctx.prev as ElementNode)?.props?.find(
        (x) =>
          (x as DirectiveNode).rawName === "v-if" ||
          (x as DirectiveNode).rawName === "v-else-if"
        // (x as DirectiveNode).rawName === "v-else" ||
      ) !== undefined;
    if (!hasPreviousIf) {
      throw new Error("v-else or v-else-if must be preceded by v-if");
    }
    if (ifNode.if === "else-if") {
      const hasPreviousElse =
        (ctx.prev as ElementNode)?.props?.find(
          (x) =>
            // (x as DirectiveNode).rawName === "v-if" ||
            (x as DirectiveNode).rawName === "v-else" //||
          // (x as DirectiveNode).rawName !== "v-else-if"
        ) !== undefined;
      if (hasPreviousElse) {
        throw new Error("v-else-if must be preceded by v-if, not v-else");
      }
    }
  }

  // regular if / else / else-if
  // return `${isStart ? "{" : ""} ${IfMap[ifNode.if]} ${
  //   ifNode.content ? `(${ifNode.content})` : ""
  // }{ ${content} } ${isEnd ? "}" : ""}`;

  switch (ifNode.if) {
    case "if": {
      return `${isStart ? "{" : ""} ${
        ifNode.content ? `${ifNode.content} ?` : ""
      } ${content} ${isEnd ? ": undefined }" : ""}`;
    }
    case "else-if": {
      return `${isStart ? "{" : ":"} ${
        ifNode.content ? `${ifNode.content} ?` : ""
      } ${content} ${isEnd ? ": undefined }" : ""}`;
    }
    case "else": {
      return `: ${content} ${isEnd ? "}" : ""}`;
    }
  }

  // ternany
  return `${isStart ? "{" : ""} ${ifNode.content ? `${ifNode.content}` : ""} ${
    ifNode.if === "else" ? "" : "?"
  } ${content} ${ifNode.if === "else" && isEnd ? "" : ":"}  ${
    isEnd && ifNode.if !== "else" ? "undefined }" : ""
  }`;
}

function wrapWithFor(
  content: string,
  forNode: WalkAttributeFor | undefined,
  magicString: MagicString,
  isWrapped: boolean,
  ctx: WalkContext
): string {
  if (!forNode) return content;
  const { source, value, key, index } = forNode.for;

  const keyString = key
    ? walkExpressionNode(key, forNode.node, magicString)
    : "";
  const indexString = index
    ? walkExpressionNode(index, forNode.node, magicString)
    : "";

  const sourceString = source
    ? walkExpressionNode(source, forNode.node, magicString)
    : "()";

  const valueString = value
    ? walkExpressionNode(value, forNode.node, magicString)
    : "value";

  const str = `renderList(${sourceString}, (${[
    valueString,
    keyString,
    indexString,
  ]
    .filter(Boolean)
    .join(", ")}) => { ${content} })`;

  // magicString.prependLeft(forParseResult.node.loc.start.offset, str);
  if (isWrapped) {
    return str;
  }
  return `{ ${str} }`;
}

function attributeToString(
  attribute: WalkAttributeAttr | WalkAttributeDirectiveBinding,
  magicString: MagicString
) {
  const { name, content } = attribute;
  if (attribute.binding) {
    if (name) {
      return `${name}={${content ?? name}}`;
    }
    return `{...${content}}`;
  }
  return `${name}=${JSON.stringify(content)}`;
}

interface WalkAttributeBase {
  node: AttributeNode | DirectiveNode;

  content?: string | undefined;
  attribute?: boolean;
  directive?: boolean;
  binding?: boolean;
  for?: undefined | ForParseResult;
  if?: "if" | "else" | "else-if";
}

interface WalkAttributeAttr extends WalkAttributeBase {
  attribute: true;
  node: AttributeNode;
  name: string;
  content: string | undefined;
}

interface WalkAttributeDirective extends WalkAttributeBase {
  directive: true;
  node: DirectiveNode;
  name?: string;
  content: string | undefined;
}
interface WalkAttributeDirectiveBinding extends WalkAttributeBase {
  directive: true;
  binding: true;
  node: DirectiveNode;
  name: string;
  content: string | undefined;
}
interface WalkAttributeFor extends WalkAttributeBase {
  for: ForParseResult;
  node: DirectiveNode;
}
interface WalkAttributeIf extends WalkAttributeBase {
  if: "if" | "else" | "else-if";
  node: DirectiveNode;
}

type WalkAttributeResult =
  | WalkAttributeAttr
  | WalkAttributeDirective
  | WalkAttributeFor
  | WalkAttributeIf
  | WalkAttributeDirectiveBinding;

export function walkAttribute(
  node: AttributeNode | DirectiveNode,
  magicString: MagicString,
  ctx: WalkContext
): WalkAttributeResult {
  switch (node.type) {
    case NodeTypes.ATTRIBUTE: {
      return {
        attribute: true,
        node,
        name: node.name,
        content: node.value ? walk(node.value, magicString, ctx) : undefined,
      };
    }
    case NodeTypes.DIRECTIVE: {
      if (node.forParseResult) {
        return {
          for: node.forParseResult,
          node: node,
        };
      }
      if (node.rawName === "v-if" || node.rawName === "v-else-if") {
        return {
          if: node.name as "if" | "else-if",
          node: node,
          content: node.exp
            ? walkExpressionNode(node.exp!, node, magicString)
            : undefined,
        };
      }
      if (node.rawName === "v-else") {
        return {
          if: "else",
          node: node,
        };
      }

      return {
        directive: true,
        binding: node.name === "bind",
        node,
        name: node.arg
          ? walkExpressionNode(node.arg, node, magicString)
          : undefined,
        content: node.exp
          ? walkExpressionNode(node.exp, node, magicString)
          : undefined,
      };
    }
    default: {
      // @ts-expect-error unknown type
      throw new Error(`Unknown attribute type ${node.type}`);
    }
  }
}

export function walkExpressionNode(
  node: ExpressionNode,
  parent: AttributeNode | DirectiveNode,
  magicString: MagicString
) {
  switch (node.type) {
    case NodeTypes.SIMPLE_EXPRESSION: {
      // magicString.update(
      //   node.loc.start.offset,
      //   node.loc.end.offset,
      //   node.content
      // );
      // // TODO handle hoisted
      return node.content;
      break;
    }
    case NodeTypes.COMPOUND_EXPRESSION: {
      break;
    }
    default: {
      // @ts-expect-error unknown type
      throw new Error(`Unknown expression type ${node.type}`);
    }
  }
}

// export function returnWrapAttributes(props: )
