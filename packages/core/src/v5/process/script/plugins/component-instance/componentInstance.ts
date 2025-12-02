import { definePlugin } from "../../types";
import { BundlerHelper } from "../../../template/helpers/bundler";
import { createHelperImport } from "../../../utils";
import { type AvailableExports } from "@verter/types/string";

export const ComponentInstancePlugin = definePlugin({
  name: "VerterComponentInstance",
  enforce: "post",

  pre(s, ctx) {
    // const prefix = ctx.prefix("");
    // const bundler = BundlerHelper.withPrefix(prefix);
    // const imports = [...bundler.imports];
    // const str = generateImport(imports);
    // s.prepend(`${str}\n`);

    if (ctx.isSetup) {
      ctx.items.push(
        createHelperImport(["PublicInstanceFromMacro"], ctx.prefix)
      );
    }
  },

  post(s, ctx) {
    if (ctx.isSetup) {
      const macroToInstance = ctx.prefix(
        "PublicInstanceFromMacro" as AvailableExports
      );
      const attributes = ctx.prefix("attributes");
      // TODO resolve the first element in template and use its type
      // NOTE that if inheritedAttrs is false, then it should be {}
      // if allowDev is true it will export a type to be imported in test components
      const allowDev = true;

      const componentName = ctx.prefix("Component");
      const templateBinding = ctx.prefix("TemplateBinding");
      const defaultOptionsName = ctx.prefix("default_Component");
      const getRootComponentName = ctx.prefix("getRootComponent");

      const genericDeclaration = ctx.generic
        ? `<${ctx.generic.declaration}>`
        : "";
      const sanitisedNames = ctx.generic
        ? `<${ctx.generic.sanitisedNames.join(",")}>`
        : "";

      const instanceName = ctx.prefix("Instance");

      const element = `ReturnType<typeof ${getRootComponentName}${sanitisedNames}>`
      const rootElementStr = `(${element} extends infer R ? R extends {$props:infer P} ? P : R : {})`;

      const declaration = [
        `export type ${instanceName}${genericDeclaration} = InstanceType<typeof ${defaultOptionsName}> & ${macroToInstance}<${templateBinding}${sanitisedNames},{}&${attributes}&${rootElementStr}, ${element}, false,false>;`,
        allowDev &&
          `export type ${instanceName}_TEST${genericDeclaration} = InstanceType<typeof ${defaultOptionsName}> & ${macroToInstance}<${templateBinding}${sanitisedNames},{}&${attributes}&${rootElementStr}, ${element}, true,true>;`,
        `export const ${componentName}={} as typeof ${defaultOptionsName} & { new${genericDeclaration}(props?: ${instanceName}${sanitisedNames}['$props']):${instanceName}${sanitisedNames} };`,
      ];

      s.append(declaration.filter(Boolean).join("\n"));
    } else {
      console.warn("Setup is not supported yet for ComponentInstancePlugin");
    }
  },

  postOld(s: any, ctx: any) {
    const prefix = ctx.prefix("");
    const bundler = BundlerHelper.withPrefix(prefix);

    const ProcessPropsName = ctx.prefix("ProcessProps");

    const defaultOptionsName = ctx.prefix("default_Component");
    const resolvePropsName = ctx.prefix("resolveProps");
    const resolveSlotsName = ctx.prefix("defineSlots");

    // imports.push({
    //   from: `./${ResolveOptionsFilename(ctx).split("/").pop() ?? ""}`,
    //   items: [
    //     { name: defaultOptionsName },
    //     { name: resolvePropsName },
    //     { name: resolveSlotsName },
    //   ],
    // });

    // const importsStr = generateImport(imports);
    // const compName = capitalize(
    //   camelize(ctx.filename.split("/").pop()?.split(".").shift() ?? "Comp")
    // );
    const componentName = ctx.prefix("Component");

    const genericDeclaration = ctx.generic
      ? `<${ctx.generic.declaration}>`
      : "";
    const sanitisedNames = ctx.generic
      ? `<${ctx.generic.sanitisedNames.join(",")}>`
      : "";

    const propsType = [
      `(${ProcessPropsName}<${resolvePropsName}${sanitisedNames}`,
      `${ctx.isAsync ? " extends Promise<infer P> ? P : {}" : ""}>)`,
    ].join("");

    const props = [`$props: ${propsType} & {};`];

    const slots = [
      `$slots: ${resolveSlotsName}${sanitisedNames}`,
      ` extends ${ctx.isAsync ? "Promise<" : ""}infer P${
        ctx.isAsync ? ">" : ""
      }`,
      `? P extends P & 1 ? {} : P & {} : never;`,
    ];

    const instanceName = ctx.prefix("instance");

    const declaration = [
      ...(ctx.isSetup
        ? [
            `export declare const ${componentName}: typeof ${defaultOptionsName} & {new${genericDeclaration}():{`,
            ...props,
            ...slots,
            `}`,
            `& ${propsType}`,
            `};`,
          ]
        : [
            `export declare const ${componentName}: typeof ${defaultOptionsName};`,
          ]),
      // `declare const ${compName}: { a: string}`,
      // `export const ${componentName};`,
      `export type ${instanceName}${genericDeclaration} = InstanceType<typeof ${
        componentName + sanitisedNames
      }>;`,
    ];

    s.append([bundler.content, declaration.join("")].join("\n"));
  },
});
