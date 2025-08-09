import { VerterASTNode } from "../../../../parser";
import {
  ProcessItem,
  ProcessItemBinding,
  ProcessItemMacroBinding,
  ProcessItemType,
} from "../../../types";
import { definePlugin, ScriptContext } from "../../types";
import { generateTypeString } from "../utils";

export const TemplateBindingPlugin = definePlugin({
  name: "VerterTemplateBinding",
  enforce: "post",

  post(s, ctx) {
    const tag = ctx.block.block.tag;
    const name = ctx.prefix("TemplateBinding");

    // if (!ctx.isSetup) {
    //   const declaration = `function ${name}FN(){return {}}`;

    //   const typeStr = generateTypeString(
    //     name,
    //     {
    //       from: `${name}FN`,
    //       isFunction: true,
    //     },
    //     ctx
    //   );

    //   s.prependRight(tag.pos.close.end, ["", declaration, typeStr].join(";"));
    //   return;
    // }

    // SETUP MODE

    const templateBindings = new Set(ctx.templateBindings.map((x) => x.name));
    const usedBindings = ctx.items
      .filter((x) => isUsedBinding(x, templateBindings))
      .map((x) => bindingToReturnStatement(x.name, x.name, x.node, ctx));

    const macros = ctx.items
      .filter((x) => x.type === ProcessItemType.MacroBinding)
      .map((x) => macroBindingToReturnStatement(x, ctx));
    const models = ctx.items.filter(
      (x) => x.type === ProcessItemType.DefineModel
    );
    const options = ctx.items.find((x) => x.type === ProcessItemType.Options);

    // known bindings
  },
});

export function bindingToReturnStatement(
  name: string,
  varName: string,
  node: {
    start: number;
    end: number;
  },
  ctx: ScriptContext
) {
  const unref = ctx.prefix(ctx.isTS ? "unref" : "UnwrapRef");

  const value = ctx.isTS
    ? `${name} as unknown as ${unref}<typeof ${name}>`
    : `${unref}(${varName})`;

  return `${name}/*${node.start},${node.end}*/:${value}`;
}

function macroBindingToReturnStatement(
  item: ProcessItemMacroBinding,
  ctx: ScriptContext
) {
  const unref = ctx.prefix(ctx.isTS ? "UnwrapRef" : "unref");

  const value = ctx.isTS
    ? `value/*${item.node.start},${item.node.end}*/:${unref}<typeof ${item.name}>`
    : `value/*${item.node.start},${item.node.end}*/:${unref}(${item.name})`;

  const items = [value];

  if (item.declarationName) {
    const declaration = ctx.isTS
      ? `${item.declarationType}:${unref}<${item.declarationType === 'object' ? 'typeof ' : ''}${item.declarationName}>`
      : `object:${unref}(${item.declarationName})`;

    items.push(declaration);
  }

  return `${item.macro}/*${item.node.start},${item.node.end}*/:{${items.join(
    ","
  )}}`;
}

function isUsedBinding(
  item: ProcessItem,
  usedBinding: Set<string | undefined>
): item is ProcessItemBinding {
  return item.type === ProcessItemType.Binding && usedBinding.has(item.name);
}
