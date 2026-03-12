import { AvailableExports } from "@verter/types/string";
import { TemplateTypes, VerterASTNode } from "../../../../parser";
import { ProcessItemDefineModel, ProcessItemMacroBinding, ProcessItemType } from "../../../types";
import { createHelperImport } from "../../../utils";
import { definePlugin } from "../../types";
import { generateTypeString } from "../utils";
import { camelize, capitalize } from "vue";
import { isBuiltInDirective } from "@vue/shared";

export const TemplateBindingPlugin = definePlugin({
  name: "VerterTemplateBinding",
  enforce: "post",

  pre(s, ctx) {
    ctx.items.push(createHelperImport(["createMacroReturn", "shallowUnwrapRef"], ctx.prefix));
  },

  post(s, ctx) {
    const isTS = ctx.block.lang === "ts";
    const isAsync = ctx.isAsync;
    const tag = ctx.block.block.tag;
    const name = ctx.prefix("TemplateBinding");

    if (!ctx.isSetup) {
      const defaultOptionsName = ctx.prefix("default_Component");
      const declaration = `function ${name}FN(){return {} as InstanceType<typeof ${defaultOptionsName}>}`;

      const typeStr = generateTypeString(
        name,
        {
          from: `${name}FN`,
          isFunction: true,
        },
        ctx,
      );

      s.prependRight(tag.pos.close.end, [declaration, typeStr].join(";"));
      return;
    }

    const templateDirectivesItems =
      ctx.blocks
        .find((x) => x.type === "template")
        ?.result?.items.filter((x) => x.type === TemplateTypes.Directive) ?? [];

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
    // const unwrapRef = ctx.prefix("UnwrapRef");
    const unwrapRef = `import('vue').UnwrapRef`;
    const shallowUnwrapRef = ctx.prefix("shallowUnwrapRef");
    const createMacroReturn = ctx.prefix("createMacroReturn" as AvailableExports);

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
    // const usedBindings = ctx.templateBindings
    //   .map((x) => {
    //     if (!x.name) return;
    //     const b = bindings.get(x.name);
    //     if (!b) return;
    //     // rebind path
    //     return {
    //       name: x.name,
    //       start: b.start,
    //       end: b.end,
    //     };
    //   })
    //   // .map((x) => (x.name ? bindings.get(x.name) : undefined))
    //   .filter((x) => !!x);

    const namedDirectives = templateDirectivesItems.map((x) =>
      isBuiltInDirective(x.node.name) ? undefined : `v${capitalize(camelize(x.node.name))}`,
    );

    const usedBindings = Array.from(
      new Set([...ctx.templateBindings.map((x) => x.name), ...namedDirectives]).values(),
    )
      .map((x) => {
        if (!x) return;
        const b = bindings.get(x);
        if (!b) return;
        // rebind path
        return {
          name: x,
          start: b.start,
          end: b.end,
        };
      })
      .filter((x) => !!x);

    // .filter((x) => x.name && bindings.has(x.name));

    const macroReturn = ctx.items.find((x) => x.type === ProcessItemType.MacroReturn);

    const propsReturn = propsBindings
      .filter((x) => !!x.valueName)
      .map((x) => {
        const keyType = x.typeName ? `keyof ${x.typeName}` : `keyof typeof ${x.valueName}`;

        return `...({} as Pick<typeof ${x.valueName}, ${keyType}>)`;
      })
      .join(",");

    const modelReturns = Array.from(modelBindings.values()).map(
      (x) =>
        `${x.name}/*${x.node.start},${x.node.end}*/: {} as typeof ${x.valueName} extends import('vue').ModelRef<infer V> ? V extends boolean|undefined ? boolean : V & {b: 1} : ${unwrapRef}<typeof ${x.valueName}> & {a: 1}`,
    );

    const returnBindings = usedBindings.map(
      (x) =>
        `${x.name}/*${x.start},${x.end}*/: ${
          isTS ? `${x.name} as unknown as typeof ${x.name}` : `${unref}(${x.name})`
        }`,
    );

    // ..${
    //     macroReturn ? `${createMacroReturn}(${macroReturn.content})` : "{}"
    //   }}
    const macroReturnStr = macroReturn ? `...${createMacroReturn}(${macroReturn.content})` : "";
    s.prependRight(
      tag.pos.close.start,
      `;return {...${shallowUnwrapRef}({${[propsReturn, ...returnBindings, ...modelReturns]
        .filter((x) => x.length > 0)
        .join(",\n")}})
${macroReturnStr ? `,${macroReturnStr}` : ""}}`,
    );

    if (!isTS) {
      s.prependLeft(
        tag.pos.open.start,
        `/** @returns {{${usedBindings
          .map((x) => `${x.name}:${unwrapRef}<typeof ${x.name}>`)
          .join(",")}}} */`,
      );
    }

    const typeStr = generateTypeString(
      name,
      {
        from: `${name}FN`,
        isFunction: true,
      },
      ctx,
    );

    s.prependRight(tag.pos.close.end, typeStr);

    s.overwrite(tag.pos.open.start + 1, tag.pos.content.start, `${name}FN`);
  },
});
