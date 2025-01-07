import { NodeTypes } from "@vue/compiler-core";
import type { AttributeNode, DirectiveNode } from "@vue/compiler-core";
import { VerterNode } from "../../../walk";
import {
  TemplateBinding,
  TemplateDirective,
  TemplateFunction,
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

          items.push(...propToTemplateProp(prop, context));
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
        items.push(...propToTemplateProp(prop, context));
    }
  }

  if (toNormalise.classes.length > 0) {
    const props = [];
    const bindings = [];

    for (let i = 0; i < toNormalise.classes.length; i++) {
      const [p, ...b] = propToTemplateProp(toNormalise.classes[i], context);

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
      const [p, ...b] = propToTemplateProp(toNormalise.styles[i], context);

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

  return items;
}

export function propToTemplateProp<T extends AttributeNode | DirectiveNode>(
  prop: T,
  context: PropsContext
): [
  TemplateProp | TemplateDirective,
  ...Array<TemplateBinding | TemplateFunction>
] {
  if (prop.type === NodeTypes.ATTRIBUTE) {
    return [
      {
        type: TemplateTypes.Prop,
        node: prop,
        name: prop.name,
        value: prop.value?.content ?? null,
        static: true,
      },
    ];
  } else if (prop.name === "bind" || prop.name === "on") {
    const nameBinding = prop.arg
      ? retrieveBindings(prop.arg, context, prop)
      : [];
    const valueBinding = prop.exp ? retrieveBindings(prop.exp, context) : [];

    return [
      {
        type: TemplateTypes.Prop,
        node: prop,
        name: prop.arg
          ? nameBinding.filter((x) => x.type === TemplateTypes.Binding)
          : null,
        value: prop.exp
          ? valueBinding.filter((x) => x.type === TemplateTypes.Binding)
          : null,
        static: false,

        event: prop.name === "on",
        context,
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
      },
      ...nameBinding,
      ...valueBinding,
    ];
  }
}
