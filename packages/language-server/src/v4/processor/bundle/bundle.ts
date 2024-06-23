import { ParseContext, PrefixSTR } from "@verter/core";
import { MagicString } from "vue/compiler-sfc";

import {
  BindingContextExportName,
  ComponentExport,
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

  s.append(
    `import { DefineComponent as ${ctx.DefineComponent} } from "vue";
import { ${ComponentExport}, ${BindingContextExportName} } from "./${optionsFile}";
import { ${FunctionExportName} } from "./${renderFile}";

export default {}
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
