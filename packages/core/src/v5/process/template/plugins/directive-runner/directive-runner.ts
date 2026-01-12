import { DirectiveNode, ElementNode } from "@vue/compiler-core";
import { MagicString } from "@vue/compiler-sfc";
import { TemplateDirective } from "../../../../parser";
import { declareTemplatePlugin } from "../../template";
import { ProcessItemType } from "../../../types";

const capitalize = (value: string) =>
  value.length > 0 ? value[0].toUpperCase() + value.slice(1) : value;

function buildDirectiveModifiers(node: DirectiveNode) {
  if (!node.modifiers || node.modifiers.length === 0) return "{}";

  const names = node.modifiers.map((m) => {
    if (typeof m === "string") return m;
    const candidate = (m as any).name ?? (m as any).value ?? (m as any).content;
    if (typeof candidate === "string") return candidate;
    return String(m);
  });

  return `{ ${names
    .map((name) => `${/^[A-Za-z_$][\w$]*$/.test(name) ? name : JSON.stringify(name)}: true`)
    .join(", ")} }`;
}

function buildExpression(
  source: MagicString,
  exp: DirectiveNode["exp"] | DirectiveNode["arg"],
  fallback: string,
  wrapStatic = false
) {
  if (!exp) return fallback;
  if (wrapStatic && "isStatic" in exp && exp.isStatic) {
    return `"${exp.content}"`;
  }
  return source.slice(exp.loc.start.offset, exp.loc.end.offset);
}

export const DirectiveRunnerPlugin = declareTemplatePlugin({
  name: "VerterDirectiveRunner",

  directivesByElement: new Map<ElementNode, TemplateDirective[]>(),

  pre() {
    this.directivesByElement.clear();
  },

  transformDirective(directive, _s, _ctx) {
    const list = this.directivesByElement.get(directive.element) ?? [];
    list.push(directive);
    this.directivesByElement.set(directive.element, list);
  },

  post(s, ctx) {
    if (this.directivesByElement.size === 0) return;

    const slotInstance = ctx.retrieveAccessor("slotInstance");
    const directiveAccessor = ctx.retrieveAccessor("directiveAccessor");
    const directiveElement = ctx.prefix("directiveElement");
    const runCustomDirective = ctx.prefix("runCustomDirective");
    const extractLeafElement = ctx.prefix("ExtractLeafElement");

    ctx.items.push({
      type: ProcessItemType.Import,
      from: "$verter/types$",
      items: [
        { name: "runCustomDirective", alias: runCustomDirective },
        { name: "ExtractLeafElement", alias: extractLeafElement, type: true },
      ],
    });

    for (const [element, directives] of this.directivesByElement.entries()) {
      if (!directives.length) continue;

      const insertPos = element.loc.start.offset + element.tag.length + 1;

      const calls = directives
        .map((directive) => {
          const node = directive.node;
          const value = buildExpression(s, node.exp, "true");
          const arg = buildExpression(s, node.arg, "undefined", true);
          const modifiers = buildDirectiveModifiers(node);

          const directiveRef = `${directiveAccessor}.v${capitalize(
            directive.name
          )}`;

          return (
            `${runCustomDirective}(${directiveElement}, ${directiveRef})(` +
            `${directiveElement}, ${value}, ${arg}, ${modifiers});`
          );
        })
        .join("");

      const block =
        ` v-directive={(${slotInstance})=>{` +
        `declare const ${directiveElement}: ${extractLeafElement}<typeof ${slotInstance}>;` +
        calls +
        `}}`;

      s.appendLeft(insertPos, block);
    }
  },
});
