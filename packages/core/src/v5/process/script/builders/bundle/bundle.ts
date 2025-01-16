import { camelize, capitalize } from "vue";
import { VerterAST } from "../../../../parser/ast";
import { ScriptItem } from "../../../../parser/script/types";
import { BundlerHelper } from "../../../template/helpers/bundler";
import { ProcessContext } from "../../../types";
import { generateImport } from "../../../utils";
import { processScript } from "../../script";
import { ScriptContext } from "../../types";
import { ResolveOptionsFilename } from "../options/index.js";

import { relative } from "node:path/posix";

// import

export function ResolveBundleFilename(filename: string) {
  return `${filename}.bundle.ts`;
}

export function buildBundle(
  items: ScriptItem[],
  context: Partial<ScriptContext> &
    Pick<ProcessContext, "filename" | "s" | "blocks">
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

          const defaultOptionsName = ctx.prefix("default");
          const resolvePropsName = ctx.prefix("resolveProps");
          const resolveExtraPropsName = ctx.prefix("resolveExtraProps");
          const resolveSlotsName = ctx.prefix("resolveSlots");

          imports.push({
            from: relative(ctx.filename, ResolveOptionsFilename(ctx.filename)),
            items: [
              { name: defaultOptionsName },
              { name: resolvePropsName },
              { name: resolveExtraPropsName },
              { name: resolveSlotsName },
            ],
          });

          const importsStr = generateImport(imports);
          const compName = capitalize(
            camelize(ctx.filename.split("/").pop()?.slice(0, -4) ?? "Comp")
          );

          const sanitisedNames = ctx.generic
            ? `<${ctx.generic.sanitisedNames.join(",")}>`
            : "";

          const props = [
            `$props: (${ProcessPropsName}<ReturnType<typeof ${resolvePropsName}${sanitisedNames}>`,
            `${ctx.isAsync ? " extends Promise<infer P> ? P : {}" : ""}>)`,
            `& ReturnType<typeof ${resolveExtraPropsName}${sanitisedNames}>;`,
          ];

          const slots = [
            `$slots: ReturnType<typeof ${resolveSlotsName}${sanitisedNames}>`,
            `extends ${ctx.isAsync ? "Promise<" : ""}infer P${
              ctx.isAsync ? ">" : ""
            }`,
            `? P extends P & 1 ? {} : P : never;`,
          ];

          const declaration = [
            `declare const ${compName}: typeof ${defaultOptionsName} & {new():{`,
            ...props,
            ...slots,
            `}};`,
            `export default ${compName};`,
          ];

          s.prepend(
            [importsStr, bundler.content, declaration.join("")].join("\n")
          );
        },
      },
    ],
    context
  );
}
