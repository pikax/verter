import { DirectiveNode, ElementNode, NodeTypes } from "@vue/compiler-core";
import { TemplateDirective } from "../../../../parser";
import { declareTemplatePlugin, TemplateContext } from "../../template";
import { ProcessItemType } from "../../../types";
import { createHelperImport } from "../../../utils";

const BUILTIN_DIRECTIVE_NAMES = new Set([
  "text",
  "html",
  "show",
  "if",
  "else-if",
  "else",
  "for",
  "slot",
  "pre",
  "once",
  "memo",
  "cloak",
  "model",
  "bind",
  "on",
]);

function pushWarning(
  node: DirectiveNode,
  ctx: import("../../template").TemplateContext,
  message:
    | "UNSUPPORTED_BUILTIN_DIRECTIVE_MODIFIER"
    | "UNSUPPORTED_BUILTIN_DIRECTIVE_ARGUMENT"
    | "UNSUPPORTED_BUILTIN_DIRECTIVE_VALUE"
) {
  ctx.items.push({
    type: ProcessItemType.Warning,
    message,
    start: node.loc.start.offset,
    end: node.loc.end.offset,
    node,
  });
}

function validateBuiltInDirective(
  name: string,
  node: DirectiveNode,
  ctx: TemplateContext
) {
  if (!BUILTIN_DIRECTIVE_NAMES.has(name)) {
    return;
  }

  const meta = BUILTIN_DIRECTIVE_META[name];
  if (!meta) return;

  const hasModifiers = node.modifiers && node.modifiers.length > 0;

  if (!meta.allowModifiers && hasModifiers) {
    pushWarning(node, ctx, "UNSUPPORTED_BUILTIN_DIRECTIVE_MODIFIER");
  }

  if (!meta.allowArg && node.arg) {
    pushWarning(node, ctx, "UNSUPPORTED_BUILTIN_DIRECTIVE_ARGUMENT");
  }

  if (!meta.allowValue && node.exp) {
    pushWarning(node, ctx, "UNSUPPORTED_BUILTIN_DIRECTIVE_VALUE");
  }
}

const BUILTIN_DIRECTIVE_META = {
  text: { allowArg: false, allowValue: true, allowModifiers: false },
  html: { allowArg: false, allowValue: true, allowModifiers: false },
  show: { allowArg: false, allowValue: true, allowModifiers: false },
  if: { allowArg: false, allowValue: true, allowModifiers: false },
  "else-if": { allowArg: false, allowValue: true, allowModifiers: false },
  else: { allowArg: false, allowValue: false, allowModifiers: false },
  for: { allowArg: false, allowValue: true, allowModifiers: false },
  slot: { allowArg: true, allowValue: true, allowModifiers: false },
  pre: { allowArg: false, allowValue: false, allowModifiers: false },
  once: { allowArg: false, allowValue: false, allowModifiers: false },
  memo: { allowArg: false, allowValue: true, allowModifiers: false },
  cloak: { allowArg: false, allowValue: false, allowModifiers: false },
  model: { allowArg: true, allowValue: true, allowModifiers: true },
  bind: { allowArg: true, allowValue: true, allowModifiers: true },
  on: { allowArg: true, allowValue: true, allowModifiers: true },
} as Record<
  string,
  { allowArg: boolean; allowValue: boolean; allowModifiers: boolean }
>;

export const DirectiveRunnerPlugin = declareTemplatePlugin({
  name: "VerterDirectiveRunner",

  directivesByElement: new Map<ElementNode, TemplateDirective[]>(),

  pre() {
    this.directivesByElement.clear();
  },

  transformDirective(directive, _s, ctx) {
    validateBuiltInDirective(directive.name, directive.node, ctx);
    if (BUILTIN_DIRECTIVE_NAMES.has(directive.name as any)) return;

    const list = this.directivesByElement.get(directive.element) ?? [];
    list.push(directive);
    this.directivesByElement.set(directive.element, list);
  },

  transformProp(prop, _s, ctx) {
    const node = prop.node;
    if (!node || node.type !== NodeTypes.DIRECTIVE) return;
    validateBuiltInDirective(prop.name, node, ctx);
  },

  post(s, ctx) {
    if (this.directivesByElement.size === 0) return;

    const slotInstance = ctx.retrieveAccessor("slotInstance");
    const directiveAccessor = ctx.retrieveAccessor("directiveAccessor");
    const directiveElement = ctx.prefix("directiveElement");
    const runCustomDirective = ctx.prefix("runCustomDirective");
    const extractLeafElement = ctx.prefix("ExtractLeafElement");

    ctx.items.push(
      createHelperImport(
        ["runCustomDirective", "ExtractLeafElement"],
        ctx.prefix
      )
    );

    for (const [element, directives] of this.directivesByElement.entries()) {
      if (!directives.length) continue;

      const insertPos = element.loc.start.offset + element.tag.length + 1;

      s.appendLeft(
        insertPos,
        ` v-directive={(${slotInstance})=>{declare const ${directiveElement}:${extractLeafElement}<typeof ${slotInstance}>;`
      );

      let lastPos = insertPos;
      let prepend = "";

      // for (const dir of directives) {
      for (let i = 0; i < directives.length; i++) {
        const dir = directives[i];
        const node = dir.node;
        const arg = node.arg;
        const exp = node.exp;

        if (i === 0) {
          const context = dir.context;
          if (ctx.doNarrow && context.conditions.length > 0) {
            ctx.doNarrow(
              {
                index: insertPos,
                inBlock: false,
                conditions: context.conditions,
                type: "append",
              },
              s
            );
          }
        }

        const startName = node.loc.start.offset;
        const endName = Math.min(
          startName + 2 /* "v-".length */ + dir.name.length,
          node.loc.end.offset
        );
        lastPos = node.loc.end.offset;

        // remove - & capitalise first letter
        if (dir.name) {
          s.overwrite(
            node.loc.start.offset + 1,
            node.loc.start.offset + 3,
            dir.name.charAt(0).toUpperCase()
          );

          // if contains hyphen, remove and capitalise next letter
          dir.name.replace(/-([a-zA-Z])/g, (_, char, index) => {
            s.overwrite(
              node.loc.start.offset + 2 + index,
              node.loc.start.offset + 4 + index,
              char.toUpperCase()
            );
            return "";
          });
        } else {
          s.overwrite(node.loc.start.offset + 1, node.loc.start.offset + 2, "");
        }

        s.appendRight(
          startName,
          `${prepend}${runCustomDirective}(${directiveElement},${directiveAccessor}["`
        );
        s.appendLeft(endName, `"])(${directiveElement}`);

        prepend = "";

        const moves = [] as Array<() => void>;

        moves.push(() => s.move(insertPos, startName, endName));

        if (exp) {
          s.remove(exp.loc.start.offset - 2, exp.loc.start.offset); // remove ="
          // s.remove(exp.loc.end.offset, exp.loc.end.offset + 1); // remove ending "
          s.overwrite(exp.loc.end.offset, exp.loc.end.offset + 1, "");
          s.prependRight(exp.loc.start.offset, ",");
          moves.push(() =>
            s.move(insertPos, exp.loc.start.offset, exp.loc.end.offset)
          );
        } else {
          prepend += ",true";
        }

        if (arg) {
          s.remove(arg.loc.start.offset - 1, arg.loc.start.offset); // remove ":"
          s.appendRight(arg.loc.start.offset, `${prepend},`);
          prepend = "";
          if (arg.type === NodeTypes.SIMPLE_EXPRESSION && arg.isStatic) {
            s.appendRight(arg.loc.start.offset, `"`);
            s.appendLeft(arg.loc.end.offset, `"`);
          }
          moves.push(() =>
            s.move(insertPos, arg.loc.start.offset, arg.loc.end.offset)
          );
        } else {
          prepend += `,undefined`;
        }

        if (node.modifiers && node.modifiers.length > 0) {
          s.appendRight(node.modifiers[0].loc.start.offset, `${prepend},{`);
          prepend = "";

          for (let i = 0; i < node.modifiers.length; i++) {
            const modifier = node.modifiers[i];
            const isLast = i === node.modifiers.length - 1;

            // remove dot "."
            s.remove(modifier.loc.start.offset - 1, modifier.loc.start.offset);

            s.appendRight(modifier.loc.start.offset, '"');
            s.appendLeft(modifier.loc.end.offset, `":true${isLast ? "" : ","}`);

            if (isLast) {
              s.appendLeft(modifier.loc.end.offset, "});");
            }

            moves.push(() =>
              s.move(
                insertPos,
                modifier.loc.start.offset,
                modifier.loc.end.offset
              )
            );
          }
        } else {
          prepend += `,{});`;
        }

        for (let i = 0; i < moves.length; i++) {
          const move = moves[i];

          move();
        }
      }

      prepend += "}}";
      if (prepend) {
        s.appendRight(lastPos, prepend);
      }
    }
  },
});
