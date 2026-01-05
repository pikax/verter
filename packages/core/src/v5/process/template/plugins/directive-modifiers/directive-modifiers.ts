import { DirectiveNode, ElementNode } from "@vue/compiler-core";
import { declareTemplatePlugin } from "../../template";

type DirectiveModifierMapEntry = {
  name: string;
  type: "event" | "custom";
  modifiers: DirectiveNode["modifiers"];
  node: DirectiveNode;
  parent: ElementNode;
};

function directiveKey(node: DirectiveNode, original: string) {
  const raw = node.rawName ? node.rawName.split(".")[0] : null;
  if (raw) return raw;

  if (node.name === "on") {
    const prefix = original[node.loc.start.offset] === "@" ? "@" : "v-on:";
    const arg = node.arg
      ? original.slice(node.arg.loc.start.offset, node.arg.loc.end.offset)
      : "";
    return `${prefix}${arg}`;
  }

  return node.name ? `v-${node.name}` : "";
}

export const DirectiveModifiersPlugin = declareTemplatePlugin({
  name: "VerterDirectiveModifiers",

  directives: new Map<string, DirectiveModifierMapEntry>(),
  pre() {
    this.directives.clear();
  },

  transformProp(prop, s, ctx) {
    if (!prop.event) return;
    const node = prop.node;
    if (!node.modifiers || node.modifiers.length === 0) return;

    const key = directiveKey(node, s.original);
    if (!key) return;

    this.directives.set(key, {
      name: key,
      type: "event",
      modifiers: node.modifiers,
      node,
      parent: prop.element,
    });
  },

  transformDirective(directive, s, ctx) {
    const node = directive.node;
    if (!node.modifiers || node.modifiers.length === 0) return;

    const key = directiveKey(node, s.original);
    if (!key) return;

    this.directives.set(key, {
      name: key,
      type: "custom",
      modifiers: node.modifiers,
      node,
      parent: directive.element,
    });
  },
});
