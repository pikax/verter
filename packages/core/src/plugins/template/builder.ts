import {
  AttributeNode,
  ComponentNode,
  DirectiveNode,
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

export function build(parsed: ReturnType<typeof parse>) {
  return parsed.children.map(renderNode).join("\n");
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

function renderNode(node: ParsedNodeBase) {
  return render[node.type](node);
}

function renderChildren(children?: ParsedNodeBase[]): string;
function renderChildren(children?: ParsedNodeBase[], join?: true): string;
function renderChildren(children?: ParsedNodeBase[], join?: false): string[];
function renderChildren(children?: ParsedNodeBase[], join = true) {
  if (!children || children.length === 0) return "";
  const c = children.map(renderNode);
  return join ? c.join("\n") : c;
}

function renderElement(node: ParsedNodeBase): string {
  const content = renderChildren(node.children);
  const tag = resolveComponentTag(node.node!);

  const props = node.props.map(renderAttribute).join(" ");
  return `<${tag}${props ? " " + props : ""}>${content}</${tag}>`;
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

function renderAttribute(node: ParsedNodeBase): string {
  const n = node.node as unknown as AttributeNode | DirectiveNode;

  const name = retriveStringExpressionNode(n.arg) || n.name;
  const content =
    retriveStringExpressionNode(n.exp) || n.value?.content || n.value;

  if (n.name === "bind") {
    if (!!n.arg) {
      return `${name}={${content ?? name}}`;
    }
    return `{...${content}}`;
  }
  return `${name}=${JSON.stringify(content)}`;
}

function renderDirective(node: ParsedNodeBase): string {
  return `NOT_KNOWN_DIRECTIVE`;
}

function renderTemplate(node: ParsedNodeBase): string {
  return `NOT_KNOWN_TEMPLATE`;
}

function renderText(node: ParsedNodeBase): string {
  return `{ ${JSON.stringify(node.content)} }`;
}
function renderFor(node: ParsedNodeBase): string {
  const forResult = node.for as ForParseResult;

  const { source, value, key, index } = forResult;

  const keyString = key ? retriveStringExpressionNode(key) : "";
  const indexString = index ? retriveStringExpressionNode(index) : "";

  const sourceString = source ? retriveStringExpressionNode(source) : "()";

  const valueString = value ? retriveStringExpressionNode(value) : "value";

  const content = renderChildren(node.children);

  const str = `renderList(${sourceString}, (${[
    valueString,
    keyString,
    indexString,
  ]
    .filter(Boolean)
    .join(", ")}) => { ${content} })`;

  return `{ ${str} }`;
}
function renderSlot(node: ParsedNodeBase): string {
  return `NOT_KNOWN_SLOT`;
}

function renderComment(node: ParsedNodeBase): string {
  return `\n/*${node.content}*/\n`;
}
function renderInterpolation(node: ParsedNodeBase): string {
  if (true) {
    const g = node.node.content.ast
      ? generateNodeText(node.node.content.ast)
      : appendCtx(node.node.content.content);

    if (g !== `_ctx.${node.node.content.content}`) {
      const r = generateNodeText(node.node.content.ast);
    }
    const n = node.node as InterpolationNode;

    return `{ ${g} }`;
  }
  return `{ ${node.content?.content} }`;
}

function appendCtx(content: string, ctx: string = "_ctx") {
  if (isGloballyAllowed(content)) return content;
  return ctx ? `${ctx}.${content}` : content;
}

function generateNodeText(node: _babel_types.Node, ctx: string = "_ctx") {
  console.log("ttytt", node.type);

  switch (node.type) {
    case "CallExpression": {
      // TODO append ctx?
      const callee = generateNodeText(node.callee); // appendCtx(node.callee.name, ctx);
      const args = node.arguments
        .map((x) => generateNodeText(x, ctx))
        .join(", ");

      return `${callee}(${args})`;
    }
    case "Identifier": {
      // TODO append ctx?
      return appendCtx(node.name, ctx);
    }
    case "MemberExpression": {
      const object = generateNodeText(node.object, ctx);
      const property = generateNodeText(
        node.property,
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
      const object = generateNodeText(node.object, ctx);
      const property = generateNodeText(node.property, "");
      return `${object}?.${property}`;
    }
    case "LogicalExpression": {
      const left = generateNodeText(node.left, ctx);
      const right = generateNodeText(node.right, ctx);
      return `${left} ${node.operator} ${right}`;
    }
    case "ObjectExpression": {
      const properties = node.properties.map(generateNodeText);
      return `{ ${properties.join(", ")} }`;
    }

    case "TSSatisfiesExpression": {
      const value = generateNodeText(node.expression, ctx);
      const type = generateNodeText(node.typeAnnotation, ctx);
      return `${value} satisfies ${type}`;
    }
    case "TSAsExpression": {
      const value = generateNodeText(node.expression, ctx);
      const type = generateNodeText(node.typeAnnotation, ctx);
      return `${value} as ${type}`;
    }
    case "TSTypeReference": {
      // TODO double check if this needs to be ctx or not
      return node.typeName.name;
    }
    case "BinaryExpression": {
      const left = generateNodeText(node.left, ctx);
      const right = generateNodeText(node.right, ctx);
      return `${left} ${node.operator} ${right}`;
    }
    case "ConditionalExpression": {
      const test = generateNodeText(node.test, ctx);
      const consequent = generateNodeText(node.consequent, ctx);
      const alternate = generateNodeText(node.alternate, ctx);
      return `${test} ? ${consequent} : ${alternate}`;
    }

    case "StringLiteral":
    case "NumericLiteral":
    case "BooleanLiteral": {
      return JSON.stringify(node.value);
    }
    case "NullLiteral": {
      return "null";
    }
    case "UnaryExpression": {
      const argument = generateNodeText(node.argument, ctx);
      return `${node.operator}${argument}`;
    }
    case "ArrayExpression": {
      const elements = node.elements.map((x) => generateNodeText(x, ctx));
      return `[${elements.join(", ")}]`;
    }
    case "ArrowFunctionExpression": {
      const params = node.params.map((x) => generateNodeText(x, ctx));
      const body = generateNodeText(node.body, ctx);
      return `(${params.join(", ")}) => ${body}`;
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

function renderCondition(node: ParsedNodeBase): string {
  function getContent(children: ParsedNodeBase[]) {
    return renderChildren(children, false).map((x) => {
      // check if wrapped, if it is unwrap
      if (x[0] === "{" && x[x.length - 1] === "}") {
        x = x.slice(1);
        x = x.slice(0, -1);
      }
      return x;
    });
  }

  const conditions = (node.conditions as []).map((x) => {
    const condition = retriveStringExpressionNode(x.expression);
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

function retriveStringExpressionNode(node?: ExpressionNode) {
  if (!node) return undefined;
  switch (node.type) {
    case NodeTypes.SIMPLE_EXPRESSION: {
      if (node.isStatic) return node.content;
      const g = node.ast ? generateNodeText(node.ast) : appendCtx(node.content);
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
