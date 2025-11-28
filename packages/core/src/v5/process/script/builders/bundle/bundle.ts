import { camelize, capitalize } from "vue";
import { VerterAST } from "../../../../parser/ast";
import { ScriptItem } from "../../../../parser/script/types";
import { BundlerHelper } from "../../../template/helpers/bundler";
import { ProcessContext } from "../../../types";
import { generateImport } from "../../../utils";
import { processScript } from "../../script";
import { ScriptContext } from "../../types";

import { ResolveOptionsFilename } from "../main";

export function ResolveBundleFilename(
  ctx: Required<Pick<ProcessContext, "blockNameResolver">>
) {
  return ctx.blockNameResolver(`bundle.ts`);
}

export function buildBundle(
  items: ScriptItem[],
  context: Partial<ScriptContext> &
    Pick<
      ProcessContext,
      "filename" | "s" | "blocks" | "block" | "blockNameResolver"
    >
) {
  return processScript(
    items,
    [
      {
        post: (s, ctx) => {
          // remove everything, we dont need it
          s.remove(0, s.original.length);

          const prefix = ctx.prefix("");
          // s.prepend(`${prefix}export default `);
          const bundler = BundlerHelper.withPrefix(prefix);

          // bundler names
          const ProcessPropsName = ctx.prefix("ProcessProps");

          const imports = [...bundler.imports];

          const defaultOptionsName = ctx.prefix("default_Component");
          const resolvePropsName = ctx.prefix("resolveProps");
          const resolveSlotsName = ctx.prefix("defineSlots");
          ctx.blockNameResolver;
          imports.push({
            from: `./${ResolveOptionsFilename(ctx).split("/").pop() ?? ""}`,
            items: [
              { name: defaultOptionsName },
              { name: resolvePropsName },
              { name: resolveSlotsName },
            ],
          });

          const importsStr = generateImport(imports);
          const compName = capitalize(
            camelize(
              ctx.filename.split("/").pop()?.split(".").shift() ?? "Comp"
            )
          );

          const genericDeclaration = ctx.generic
            ? `<${ctx.generic.declaration}>`
            : "";

          const sanitisedNames = ctx.generic
            ? `<${ctx.generic.sanitisedNames.join(",")}>`
            : "";

          // const vSlotProp = `'v-slot': (cb: (i: InstanceType<typeof ${compName}${sanitisedNames}>) => any) => any`;
          // const vSlotProp = [
          //   `'v-slot': (cb: (instance: ${resolveSlotsName}${sanitisedNames}`,
          //   ` extends ${ctx.isAsync ? "Promise<" : ""}infer P${
          //     ctx.isAsync ? ">" : ""
          //   }`,
          //   `? P extends P & 1 ? {} : P & {} : never) => void | any | any[])=>void | any;`,
          // ].join("");

          const vSlotProp = `'${ctx.prefix(
            "v-slot"
          )}'?: (i: InstanceType<typeof ${compName}${sanitisedNames}>)  => any`;

          const props = [
            `$props: (${ProcessPropsName}<${resolvePropsName}${sanitisedNames}`,
            `${ctx.isAsync ? " extends Promise<infer P> ? P : {}" : ""}>)`,
            // ` & { supa?: (i: InstanceType<typeof ${compName}${sanitisedNames}>)  => any}`,
            // ` & { ___verter___slot?: (i: InstanceType<typeof ${compName}${sanitisedNames}>)  => any}`,
            ` & {${vSlotProp}};`,
          ];

          const slots = [
            `$slots: ${resolveSlotsName}${sanitisedNames}`,
            ` extends ${ctx.isAsync ? "Promise<" : ""}infer P${
              ctx.isAsync ? ">" : ""
            }`,
            `? P extends P & 1 ? {} : P & {} : never;`,
          ];

          const declaration = [
            ...(ctx.isSetup
              ? [
                  `declare const ${compName}: typeof ${defaultOptionsName} & {new${genericDeclaration}():{`,
                  ...props,
                  ...slots,
                  `}};`,
                ]
              : [`declare const ${compName}: typeof ${defaultOptionsName};`]),
            // `declare const ${compName}: { a: string}`,
            `export default ${compName};`,
          ];
          // const declaration = `declare const ${compName}: typeof ${defaultOptionsName};`;

          s.prepend(
            [importsStr, bundler.content, declaration.join("")].join("\n")
          );
        },
      },
    ],
    context
  );
}
