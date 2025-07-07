import { NodeTypes } from "@vue/compiler-core";
import type {
  AttributeNode,
  DirectiveNode,
  ElementNode,
} from "@vue/compiler-core";
import { VerterNode } from "../../../walk";
import {
  TemplateBinding,
  TemplateDirective,
  TemplateFunction,
  TemplateLiteral,
  TemplateProp,
  TemplateTypes,
} from "../../types";
import { retrieveBindings } from "../../utils";

export type PropsContext = {
  ignoredIdentifiers: string[];
};

export function handleProps(node: VerterNode, context: PropsContext) {
  if (node.type !== NodeTypes.ELEMENT) {
    return null;
  }

  const items = [];
  const toNormalise = {
    classes: [] as Array<DirectiveNode | AttributeNode>,
    styles: [] as Array<DirectiveNode | AttributeNode>,
  };

  for (const prop of node.props) {
    switch (prop.name) {
      case "style":
        toNormalise.styles.push(prop);
        break;
      case "class":
        toNormalise.classes.push(prop);
        break;
      case "on":
      case "bind": {
        if (prop.type === NodeTypes.DIRECTIVE) {
          // @ts-expect-error
          if (prop.arg?.content === "class") {
            toNormalise.classes.push(prop);
            break;
          }
          // @ts-expect-error
          if (prop.arg?.content === "style") {
            toNormalise.styles.push(prop);
            break;
          }

          items.push(...propToTemplateProp(prop, node, context));
        }
        break;
      }
      case "if":
      case "else-if":
      case "else":
      case "for":
      case "slot": {
        // slots the others are handled by the parent
        if (prop.type === NodeTypes.DIRECTIVE) {
          break;
        }
      }
      default:
        if (prop.name === "is" && node.tag === "component") {
          break;
        }

        items.push(...propToTemplateProp(prop, node, context));
    }
  }

  if (toNormalise.classes.length > 0) {
    const props = [];
    const bindings = [];

    for (let i = 0; i < toNormalise.classes.length; i++) {
      const [p, ...b] = propToTemplateProp(
        toNormalise.classes[i],
        node,
        context
      );

      props.push(p);
      bindings.push(...b);
    }

    items.push({
      type: TemplateTypes.Prop,
      node: null,
      name: "class",
      props: props,
      context,
    });
    items.push(...bindings);
  }

  if (toNormalise.styles.length > 0) {
    const props = [];
    const bindings = [];

    for (let i = 0; i < toNormalise.styles.length; i++) {
      const [p, ...b] = propToTemplateProp(
        toNormalise.styles[i],
        node,
        context
      );

      props.push(p);
      bindings.push(...b);
    }

    items.push({
      type: TemplateTypes.Prop,
      node: null,
      name: "style",
      props: props,
      context,
    });
    items.push(...bindings);
  }

  const tagEnd =
    (node.isSelfClosing
      ? node.loc.end.offset
      : node.children.length > 0
      ? node.children[node.children.length - 1].loc.end.offset
      : node.loc.end.offset) - node.loc.start.offset;

  const hasPre = node.loc.source.slice(0, tagEnd).indexOf("v-pre");
  if (hasPre !== -1) {
    items.push({
      type: TemplateTypes.Prop,
      name: "pre",
      arg: null,
      exp: null,
      static: true,
      node: {
        loc: {
          start: { offset: hasPre + node.loc.start.offset },
          end: { offset: hasPre + 5 + node.loc.start.offset },
        },
      },
      context,
    });
  }

  return items;
}

const BuiltInDirectivesAsProps = new Set([
  "bind",
  "on",
  "text",
  "html",
  "show",
  "pre",
  "once",
  "memo",
  "cloak",
]);

export function propToTemplateProp<T extends AttributeNode | DirectiveNode>(
  prop: T,
  element: ElementNode,
  context: PropsContext
): [
  TemplateProp | TemplateDirective,
  ...Array<TemplateBinding | TemplateFunction | TemplateLiteral>
] {
  if (prop.type === NodeTypes.ATTRIBUTE) {
    return [
      {
        type: TemplateTypes.Prop,
        node: prop,
        name: prop.name,
        value: prop.value?.content ?? null,
        static: true,
        event: false,
        element,
        items: [],
      },
    ];
  } else if (BuiltInDirectivesAsProps.has(prop.name)) {
    const nameBinding = prop.arg
      ? retrieveBindings(prop.arg, context, prop)
      : [];
    const valueBinding = prop.exp ? retrieveBindings(prop.exp, context) : [];

    if (!prop.exp) {
      nameBinding
        .filter((x) => x.type === TemplateTypes.Binding)
        .forEach((x) => {
          x.ignore = false;
          x.skip = true;
        });
    }

    return [
      {
        type: TemplateTypes.Prop,
        node: prop,
        arg: prop.arg
          ? nameBinding.filter((x) => x.type === TemplateTypes.Binding)
          : null,
        exp: prop.exp
          ? valueBinding.filter((x) => x.type === TemplateTypes.Binding)
          : null,
        static: false,

        event: prop.name === "on",
        name: prop.name,
        context,
        element,
        items: [...nameBinding, ...valueBinding],
      },
      ...nameBinding,
      ...valueBinding,
    ];
  } else {
    const nameBinding = prop.arg ? retrieveBindings(prop.arg, context) : [];
    const valueBinding = prop.exp ? retrieveBindings(prop.exp, context) : [];

    return [
      {
        type: TemplateTypes.Directive,
        node: prop,
        name: prop.name,
        arg: prop.arg
          ? nameBinding.filter((x) => x.type === TemplateTypes.Binding)
          : null,
        exp: prop.exp
          ? valueBinding.filter((x) => x.type === TemplateTypes.Binding)
          : null,
        static: false,
        context,
        element,

        items: [...nameBinding, ...valueBinding],
      },
      ...nameBinding,
      ...valueBinding,
    ];
  }
}
