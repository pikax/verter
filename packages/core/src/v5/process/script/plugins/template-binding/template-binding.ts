import { AvailableExports } from "@verter/types/string";
import { VerterASTNode } from "../../../../parser";
import {
  ProcessItemDefineModel,
  ProcessItemMacroBinding,
  ProcessItemType,
} from "../../../types";
import { createHelperImport } from "../../../utils";
import { definePlugin } from "../../types";
import { generateTypeString } from "../utils";

export const TemplateBindingPlugin = definePlugin({
  name: "VerterTemplateBinding",
  enforce: "post",

  pre(s, ctx) {
    ctx.items.push(createHelperImport(["createMacroReturn"], ctx.prefix));
  },

  post(s, ctx) {
    const isTS = ctx.block.lang === "ts";
    const isAsync = ctx.isAsync;
    const tag = ctx.block.block.tag;
    const name = ctx.prefix("TemplateBinding");

    if (!ctx.isSetup) {
      const declaration = `function ${name}FN(){return {}}`;

      const typeStr = generateTypeString(
        name,
        {
          from: `${name}FN`,
          isFunction: true,
        },
        ctx
      );

      s.prependRight(tag.pos.close.end, [declaration, typeStr].join(";"));
      return;
    }

    // const bindings = new Set(
    //   ctx.items
    //     .filter(
    //       (x) =>
    //         x.type === ProcessItemType.Binding ||
    //         x.type === ProcessItemType.Import
    //     )
    //     .flatMap((x) =>
    //       x.type === ProcessItemType.Binding
    //         ? x.name
    //         : x.items.map((x) => x.alias ?? x.name)
    //     )
    // );
    // const bindings = new Map(
    //   ctx.items
    //     .filter(
    //       (x) =>
    //         x.type === ProcessItemType.Binding ||
    //         x.type === ProcessItemType.Import
    //     )
    //     .flatMap((x) => (x.type === ProcessItemType.Binding ? x : x.items))
    //     .map((x) => '  [ x.name, x])
    // );

    const bindings = new Map<string, VerterASTNode>();
    const propsBindings = [] as ProcessItemMacroBinding[];
    const modelBindings = new Map<string, ProcessItemDefineModel>();
    for (const item of ctx.items) {
      switch (item.type) {
        case ProcessItemType.Binding: {
          bindings.set(item.name, item.node);
          break;
        }
        case ProcessItemType.DefineModel: {
          modelBindings.set(item.name, item);
          break;
        }
        case ProcessItemType.MacroBinding: {
          if (item.macro === "defineProps") {
            propsBindings.push(item);
          }
          break;
        }
        // case ProcessItemType.Import: {
        //   for (const importItem of item.items) {
        //     bindings.set(importItem.alias ?? importItem.name, importItem.node);
        //   }
        //   break;
        // }
      }
    }

    const unref = ctx.prefix("unref");
    const unwrapRef = ctx.prefix("UnwrapRef");
    const createMacroReturn = ctx.prefix(
      "createMacroReturn" as AvailableExports
    );

    // const macroBindings = ctx.items
    //   .filter((x) => x.type === ProcessItemType.MacroBinding)
    //   .reduce((acc, x) => {
    //     const n = ctx.prefix(
    //       x.macro === "withDefaults" ? "defineProps" : x.macro
    //     );
    //     acc[x.macro] = x.name;
    //     return acc;
    //   }, {} as Record<string, string>);
    // const defineModels = ctx.items.filter(
    //   (x) => x.type === ProcessItemType.DefineModel
    // );
    const usedBindings = ctx.templateBindings
      .map((x) => {
        if (!x.name) return;
        const b = bindings.get(x.name);
        if (!b) return;
        // rebind path
        return {
          name: x.name,
          start: b.start,
          end: b.end,
        };
      })
      // .map((x) => (x.name ? bindings.get(x.name) : undefined))
      .filter((x) => !!x);

    // .filter((x) => x.name && bindings.has(x.name));

    const macroReturn = ctx.items.find(
      (x) => x.type === ProcessItemType.MacroReturn
    );

    const propsReturn = propsBindings
      .map((x) => x.valueName)
      .filter((x) => !!x)
      .map((x) => `...({} as typeof ${x})`)
      .join(",");

    const modelReturns = Array.from(modelBindings.values())
      .map(
        (x) =>
          `${x.name}/*${x.node.start},${x.node.end}*/: {} as typeof ${x.valueName} extends import('vue').ModelRef<infer V> ? V : ${unwrapRef}<typeof ${x.valueName}>`
      )
      .join(",");

    s.prependRight(
      tag.pos.close.start,
      `;return{
${propsReturn}${propsReturn ? "," : ""}
${modelReturns}${modelReturns ? "," : ""}
${usedBindings
  .map(
    (x) =>
      `${x.name}/*${x.start},${x.end}*/: ${
        isTS
          ? `${x.name} as unknown as ${unwrapRef}<typeof ${x.name}>`
          : `${unref}(${x.name})`
      }`
  )
  // .concat(
  //   Object.entries(macroBindings).map(
  //     ([k, x]) => `${k}:${`${isTS ? `${x} as typeof ${x}` : ""}`}`
  //   )
  //   // Object.entries(macroBindings).map(([k, v]) =>
  //   //   v.map((x) => `${k}:${k}`)
  //   // )
  // )
  // .concat([
  //   // defineModel regular props
  //   `${ctx.prefix("defineModel")}:{${defineModels
  //     .map(
  //       (x) =>
  //         // TODO this should be either pointing to the variable or to the function itself
  //         `${x.name}/*${x.node.start},${x.node.end}*/: ${
  //           isTS ? `${x.varName} as typeof ${x.varName}` : x.varName
  //         }`
  //     )
  //     .join(",")}}`,
  // ])
  .join(",")}${usedBindings.length > 0 ? "," : ""}...${
        macroReturn ? `${createMacroReturn}(${macroReturn.content})` : "{}"
      }}`
    );

    if (!isTS) {
      s.prependLeft(
        tag.pos.open.start,
        `/** @returns {{${usedBindings
          .map((x) => `${x.name}:${unwrapRef}<typeof ${x.name}>`)
          .join(",")}}} */`
      );
    }

    const typeStr = generateTypeString(
      name,
      {
        from: `${name}FN`,
        isFunction: true,
      },
      ctx
    );

    s.prependRight(tag.pos.close.end, typeStr);

    s.overwrite(tag.pos.open.start + 1, tag.pos.content.start, `${name}FN`);
  },
});
