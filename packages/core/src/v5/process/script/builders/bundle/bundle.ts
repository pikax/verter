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

          const imports = [];
          const defaultOptionsName = ctx.prefix("Component");
          ctx.blockNameResolver;
          imports.push({
            from: `./${ResolveOptionsFilename(ctx).split("/").pop() ?? ""}`,
            items: [{ name: defaultOptionsName }],
          });

          const importsStr = generateImport(imports);
          const compName = capitalize(
            camelize(
              ctx.filename.split("/").pop()?.split(".").shift() || "Component"
            )
          );

          const declaration = [
            `declare const ${compName}: typeof ${defaultOptionsName};`,
            `export default ${compName};`,
          ];

          s.prepend([importsStr, declaration.join("\n")].join("\n"));
        },
      },
    ],
    context
  );
}
