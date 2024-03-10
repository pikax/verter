import {
  AttributeNode,
  ComponentNode,
  DirectiveNode,
  ElementNode,
  ElementTypes,
  ExpressionNode,
  ForParseResult,
  InterpolationNode,
  NodeTypes,
  PlainElementNode,
  TemplateChildNode,
  walkIdentifiers,
} from "@vue/compiler-core";
import { ParsedNodeBase, ParsedType, parse } from "./parse.js";
import { camelize, capitalize, isGloballyAllowed } from "@vue/shared";
import type * as _babel_types from "@babel/types";
import { LocationType, WalkResult } from "../types.js";

export function build(
  parsed: ReturnType<typeof parse>,
  ignoredIdentifiers: string[] = [],
  declarations: WalkResult[] = []
) {
  return parsed.children
    .map((x) => renderNode(x, ignoredIdentifiers, declarations))
    .join("\n");
}

const render = {
  [ParsedType.Element]: renderElement,
  [ParsedType.Attribute]: renderAttribute,
  [ParsedType.Directive]: renderDirective,
  [ParsedType.Template]: renderTemplate,
  [ParsedType.Text]: renderText,
  [ParsedType.For]: renderFor,
  [ParsedType.RenderSlot]: renderSlot,
  [ParsedType.Comment]: renderComment,
  [ParsedType.Interpolation]: renderInterpolation,
  [ParsedType.Condition]: renderCondition,
};

function renderNode(
  node: ParsedNodeBase,
  ignoredIdentifiers: string[] = [],
  declarations: WalkResult[] = []
) {
  return render[node.type](node, ignoredIdentifiers, declarations);
}

function renderChildren(
  children: ParsedNodeBase[],
  ignoredIdentifiers: string[]
): string;
function renderChildren(
  children: ParsedNodeBase[],
  ignoredIdentifiers: string[],
  join?: true
): string;
function renderChildren(
  children: ParsedNodeBase[],
  ignoredIdentifiers: string[],
  join?: false
): string[];
function renderChildren(
  children: ParsedNodeBase[],
  ignoredIdentifiers: string[],
  join = true
) {
  if (!children || children.length === 0) return "";
  const c = children.map((x) => renderNode(x, ignoredIdentifiers));
  return join ? c.join("\n") : c;
}

function renderElement(
  node: ParsedNodeBase & { node: ElementNode; props: ParsedNodeBase[] },
  ignoredIdentifiers: string[],
  declarations: WalkResult[] = []
): string {
  const content = renderChildren(node.children, ignoredIdentifiers);
  const tag = resolveComponentTag(node.node);

  const props = node.props
    .map((p) => renderAttribute(p, ignoredIdentifiers))
    .join(" ");

  // TODO add support for slots inferrence
  // if (tag === "slot") {
  //   declarations.push({
  //     type: LocationType.Slots,
  //     node: node.node,
  //     content: content,
  //     properties: retrieveProps(node.props, ignoredIdentifiers),
  //   });
  // }

  return `<${tag}${props ? " " + props : ""}>${content}</${tag}>`;
}

function retrieveProps(
  props: ParsedNodeBase[],
  ignoredIdentifiers: string[] = []
) {
  return props.flatMap((x) => {
    const n = x.node as unknown as AttributeNode | DirectiveNode;
    if (n.name === "name") return [];
    if (n.name === "bind") {
      if (!n.arg) {
        // full v-bind v-bind={}
        return [];
      }
    } else {
    }
    if (n.name === "bind") {
      const content = retriveStringExpressionNode(n.exp, ignoredIdentifiers);
      debugger;
      return [];
    }
  });
}

function resolveComponentTag(
  node: PlainElementNode | ComponentNode | ElementNode
) {
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

function renderAttribute(
  node: ParsedNodeBase,
  ignoredIdentifiers: string[]
): string {
  const n = node.node as unknown as AttributeNode | DirectiveNode;

  const name = retriveStringExpressionNode(n.arg, ignoredIdentifiers) || n.name;
  const content =
    retriveStringExpressionNode(n.exp, ignoredIdentifiers) ||
    n.value?.content ||
    n.value;

  if (n.name === "bind") {
    if (!!n.arg) {
      return `${name}={${content ?? name}}`;
    }
    return `{...${content}}`;
  }
  if (n.name === "on") {
    return `on${capitalize(name)}={${content}}`;
  }
  if (n.type === NodeTypes.DIRECTIVE) {
    return `${name}={${content}}`;
  }

  return `${name}={${JSON.stringify(content)}`;
}

function renderDirective(
  node: ParsedNodeBase,
  ignoredIdentifiers: string[]
): string {
  return `NOT_KNOWN_DIRECTIVE`;
}

function renderTemplate(
  node: ParsedNodeBase,
  ignoredIdentifiers: string[]
): string {
  return `NOT_KNOWN_TEMPLATE`;
}

function renderText(
  node: ParsedNodeBase,
  ignoredIdentifiers: string[]
): string {
  return `{ ${JSON.stringify(node.content)} }`;
}
function renderFor(node: ParsedNodeBase, ignoredIdentifiers: string[]): string {
  const forResult = node.for as ForParseResult;

  const { source, value, key, index } = forResult;

  const keyString = key
    ? retriveStringExpressionNode(key, ignoredIdentifiers, "")
    : "";
  const indexString = index
    ? retriveStringExpressionNode(index, ignoredIdentifiers, "")
    : "";

  const sourceString = source
    ? retriveStringExpressionNode(source, ignoredIdentifiers)
    : "()";

  const valueString = value
    ? retriveStringExpressionNode(value, ignoredIdentifiers, "")
    : "value";

  const content = renderChildren(
    node.children,
    [...ignoredIdentifiers, valueString, keyString, indexString].filter(Boolean)
  );

  const str = `renderList(${sourceString}, (${[
    valueString,
    keyString,
    indexString,
  ]
    .filter(Boolean)
    .join(", ")}) => { ${content} })`;

  return `{ ${str} }`;
}
function renderSlot(
  node: ParsedNodeBase,
  ignoredIdentifiers: string[],
  declarations: WalkResult[]
): string {
  return `NOT_KNOWN_SLOT`;
}

function renderComment(
  node: ParsedNodeBase,
  ignoredIdentifiers: string[]
): string {
  return `\n{ /*${node.content}*/ }\n`;
}
function renderInterpolation(
  node: ParsedNodeBase,
  ignoredIdentifiers: string[]
): string {
  if (true) {
    const g = node.node.content.ast
      ? generateNodeText(node.node.content.ast, ignoredIdentifiers)
      : appendCtx(node.node.content.content, ignoredIdentifiers);

    if (g !== `_ctx.${node.node.content.content}`) {
      const r = generateNodeText(node.node.content.ast, ignoredIdentifiers);
    }
    const n = node.node as InterpolationNode;

    return `{ ${g} }`;
  }
  return `{ ${node.content?.content} }`;
}

function appendCtx(
  content: string,
  ignoreCtx: string[] = [],
  ctx: string = "_ctx"
) {
  if (isGloballyAllowed(content) || ~ignoreCtx.indexOf(content) || !ctx)
    return content;
  return `${ctx}.${content}`;
}

function generateNodeText(
  node: _babel_types.Node,
  ignoreCtx: string[] = [],
  ctx: string = "_ctx"
) {
  console.log("ttytt", node.type);

  switch (node.type) {
    case "CallExpression": {
      const callee = generateNodeText(node.callee, ignoreCtx); // appendCtx(node.callee.name, ctx);
      const args = node.arguments
        .map((x) => generateNodeText(x, ignoreCtx, ctx))
        .join(", ");

      return `${callee}(${args})`;
    }
    case "Identifier": {
      return appendCtx(node.name, ignoreCtx, ctx);
    }
    case "MemberExpression": {
      const object = generateNodeText(node.object, ignoreCtx, ctx);
      const property = generateNodeText(
        node.property,
        ignoreCtx,
        node.property.type === "Identifier" ? "" : ctx
      );
      // if (node.extra.parenthesized) {
      //   return `${object}[${property}]`;
      // }
      if (node.property.type === "Identifier") {
        return `${object}.${property}`;
      } else {
        return `${object}[${property}]`;
      }
    }
    case "OptionalMemberExpression": {
      const object = generateNodeText(node.object, ignoreCtx, ctx);
      const property = generateNodeText(node.property, ignoreCtx, "");
      return `${object}?.${property}`;
    }
    case "LogicalExpression": {
      const left = generateNodeText(node.left, ignoreCtx, ctx);
      const right = generateNodeText(node.right, ignoreCtx, ctx);
      return `${left} ${node.operator} ${right}`;
    }
    case "ObjectExpression": {
      const keys = node.properties.map((x) => x.key.name).filter(Boolean);

      const ignore = [...ignoreCtx, ...keys];

      const properties = node.properties.map((x) =>
        generateNodeText(x, ignore, ctx)
      );
      return `{ ${properties.join(", ")} }`;
    }

    case "TSSatisfiesExpression": {
      const value = generateNodeText(node.expression, ignoreCtx, ctx);
      const type = generateNodeText(node.typeAnnotation, ignoreCtx, ctx);
      return `${value} satisfies ${type}`;
    }
    case "TSAsExpression": {
      const value = generateNodeText(node.expression, ignoreCtx, ctx);
      const type = generateNodeText(node.typeAnnotation, ignoreCtx, ctx);
      return `${value} as ${type}`;
    }
    case "TSTypeReference": {
      // TODO double check if this needs to be ctx or not
      return node.typeName.name;
    }
    case "BinaryExpression": {
      const left = generateNodeText(node.left, ignoreCtx, ctx);
      const right = generateNodeText(node.right, ignoreCtx, ctx);
      return `${left} ${node.operator} ${right}`;
    }
    case "ConditionalExpression": {
      const test = generateNodeText(node.test, ignoreCtx, ctx);
      const consequent = generateNodeText(node.consequent, ignoreCtx, ctx);
      const alternate = generateNodeText(node.alternate, ignoreCtx, ctx);
      return `${test} ? ${consequent} : ${alternate}`;
    }

    case "StringLiteral":
    case "NumericLiteral":
    case "BooleanLiteral": {
      return JSON.stringify(node.value);
    }
    case "TemplateLiteral": {
      const quasis = node.quasis.map((x) => x.value.raw);
      const expressions = node.expressions.map((x) =>
        generateNodeText(x, ignoreCtx, ctx)
      );
      const content = quasis.reduce((prev, cur, currentIndex) => {
        return (
          prev +
          cur +
          (expressions[currentIndex]
            ? `\$\{${expressions[currentIndex]}\}`
            : "")
        );
      }, "");

      return `\`${content}\``;
    }

    case "NullLiteral": {
      return "null";
    }
    case "UnaryExpression": {
      const argument = generateNodeText(node.argument, ignoreCtx, ctx);
      return `${node.operator}${argument}`;
    }
    case "ArrayExpression": {
      const elements = node.elements.map((x) =>
        generateNodeText(x, ignoreCtx, ctx)
      );
      return `[${elements.join(", ")}]`;
    }
    case "ArrowFunctionExpression": {
      const params = node.params.map((x) => generateNodeText(x, ignoreCtx, ""));
      const body = generateNodeText(node.body, [...ignoreCtx, ...params], ctx);
      return `(${params.join(", ")}) => ${body}`;
    }
    case "ObjectProperty": {
      const key = generateNodeText(node.key, ignoreCtx, ctx);
      const value = generateNodeText(node.value, ignoreCtx, ctx);
      return `${key}: ${value}`;
    }
    default: {
      console.log("-----", node.type);
      return node.content;
    }
    // case NodeTypes.SIMPLE_EXPRESSION: {
    //   return generateSimpleExpressionText(node);
    // }
    // case NodeTypes.COMPOUND_EXPRESSION: {
    //   return generateCompoundExpressionText(node);
    // }
    // default: {
    //   throw new Error(`Unknown node type ${node.type}`);
    // }
  }
}

function renderCondition(
  node: ParsedNodeBase,
  ignoredIdentifiers: string[]
): string {
  function getContent(children: ParsedNodeBase[]) {
    return renderChildren(children, ignoredIdentifiers, false).map((x) => {
      // check if wrapped, if it is unwrap
      if (x[0] === "{" && x[x.length - 1] === "}") {
        x = x.slice(1);
        x = x.slice(0, -1);
      }
      return x;
    });
  }

  const conditions = (node.conditions as []).map((x) => {
    const condition = retriveStringExpressionNode(
      x.expression,
      ignoredIdentifiers
    );
    const content = getContent(x.children ?? []);
    return {
      rawName: x.rawName,
      condition,
      content,
    };
  });
  const c = conditions.reduce((prev, cur, currentIndex, arr) => {
    const last = currentIndex === arr.length - 1;
    if (cur.rawName !== "v-else") {
      const r = `${prev ? prev + " : " : ""}(${cur.condition}) ? ${
        cur.content
      }`;
      if (last) return r + " : undefined";
      return r;
    } else {
      return `${prev} : ${cur.content} `;
    }
  }, "");

  return `{ ${c} }`;
}

function retriveStringExpressionNode(
  node: ExpressionNode,
  ignoredIdentifiers: string[],
  ctx = "_ctx"
) {
  if (!node) return undefined;
  switch (node.type) {
    case NodeTypes.SIMPLE_EXPRESSION: {
      if (node.isStatic) return node.content;
      const g = node.ast
        ? generateNodeText(node.ast, ignoredIdentifiers, ctx)
        : appendCtx(node.content, ignoredIdentifiers, ctx);
      return g;
      // return node.content;
      break;
    }
    case NodeTypes.COMPOUND_EXPRESSION: {
      return "NOT_KNOWN COMPOUND_EXPRESSION";
      break;
    }
    default: {
      // @ts-expect-error unknown type
      throw new Error(`Unknown expression type ${node.type}`);
    }
  }
}
