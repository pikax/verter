import { ProcessItemType } from "../../../types";
import { generateImport } from "../../../utils";
import { declareTemplatePlugin } from "../../template";

export const ImportsPlugin = declareTemplatePlugin({
  name: "VerterImports",
  enforce: "post",

  post(s, ctx) {
    const imports = ctx.items.filter((x) => x.type === ProcessItemType.Import);
    if (imports.length === 0) return;

    const importStr = generateImport(imports);
    s.prepend(importStr);
  },
});
