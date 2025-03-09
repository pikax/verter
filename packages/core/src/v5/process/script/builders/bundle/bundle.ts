import { camelize, capitalize } from "vue";
import { VerterAST } from "../../../../parser/ast";
import { ScriptItem } from "../../../../parser/script/types";
import { BundlerHelper } from "../../../template/helpers/bundler";
import { ProcessContext } from "../../../types";
import { generateImport } from "../../../utils";
import { processScript } from "../../script";
import { ScriptContext } from "../../types";

import { relative } from "node:path/posix";
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

          const defaultOptionsName = ctx.prefix("default");
          const resolvePropsName = ctx.prefix("resolveProps");
          const resolveSlotsName = ctx.prefix("resolveSlots");
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

          const sanitisedNames = ctx.generic
            ? `<${ctx.generic.sanitisedNames.join(",")}>`
            : "";

          const props = [
            `$props: (${ProcessPropsName}<ReturnType<typeof ${resolvePropsName}${sanitisedNames}>`,
            `${ctx.isAsync ? " extends Promise<infer P> ? P : {}" : ""}>);`,
          ];

          const slots = [
            `$slots: ReturnType<typeof ${resolveSlotsName}${sanitisedNames}>`,
            `extends ${ctx.isAsync ? "Promise<" : ""}infer P${
              ctx.isAsync ? ">" : ""
            }`,
            `? P extends P & 1 ? {} : P : never;`,
          ];

          const declaration = [
            // `declare const ${compName}: typeof ${defaultOptionsName} & {new():{`,
            // ...props,
            // ...slots,
            // `}};`,
            `declare const ${compName}: typeof ${defaultOptionsName};`,
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
