import { ParseContext, PrefixSTR } from "@verter/core";
import { MagicString } from "vue/compiler-sfc";

import {
  ResolveProps,
  ComponentExport,
  DefaultOptions,
} from "./../options/index.js";
import { FunctionExportName } from "./../render/index.js";
import { getBlockFilename } from "../utils";

export function processBundle(context: ParseContext) {
  const filename = context.filename + ".bundle.ts";

  const optionsFile = getBlockFilename("options", context, true);
  const renderFile = getBlockFilename("render", context, true);
  const s = new MagicString("");

  const ctx = {
    DefineComponent: PrefixSTR("DefineComponent"),
  };
  const generic = context.generic;

  s.append(
    `import { DefineComponent as ${ctx.DefineComponent} } from "vue";
import { ${DefaultOptions}, ${ResolveProps} } from "${optionsFile}";
// import { ${FunctionExportName} } from "${renderFile}";

declare const Comp : typeof ${DefaultOptions} & { new${
      generic ? `<${generic.declaration}>` : ""
    }(): { 
  $props: ReturnType<typeof ${ResolveProps}${
      generic ? `<${generic.sanitisedNames.join(",")}>` : ""
    }>;
} };
export default Comp;
`
  );

  return {
    languageId: "typescript",
    filename,

    loc: {
      source: s.original,
    },

    s,
    content: s.toString(),
  };
}
