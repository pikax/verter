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
          const imports = [...bundler.imports];

          imports.push({
            from: relative(ctx.filename, ResolveOptionsFilename(ctx.filename)),
            items: [
              {
                name: ctx.prefix("resolveProps"),
              },
              {
                name: ctx.prefix("default"),
              },
              {
                name: ctx.prefix("resolveExtraProps"),
              },
              {
                name: ctx.prefix("resolveSlots"),
              },
            ],
          });

          const importsStr = generateImport(imports);
          s.prepend([importsStr, bundler.content].join("\n"));
        },
      },
    ],
    context
  );
}
