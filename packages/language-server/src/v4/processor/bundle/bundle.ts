import { ParseScriptContext, PrefixSTR, VerterSFCBlock } from "@verter/core";
import { MagicString } from "vue/compiler-sfc";

import {
  DefaultOptions,
  BindingContextExportName,
  ComponentExport,
} from "./../options/index.js";
import { getBlockFilename } from "../utils";

export function processBundle(context: ParseScriptContext) {
  const filename = context.filename + ".bundle.ts";

  const optionsFile = getBlockFilename("options", context, true);
  const s = new MagicString("");

  const ctx = {
    DefineComponent: PrefixSTR("DefineComponent"),
  };

  s.append(
    `import { DefineComponent as ${ctx.DefineComponent} } from "vue";
import { ${ComponentExport}, ${BindingContextExportName} } from "./${optionsFile}";
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
