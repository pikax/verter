import { ProcessItemType } from "../../../types";
import { generateImport } from "../../../utils";
import { definePlugin } from "../../types";

// move imports to the top of the file
export const ImportsPlugin = definePlugin({
  name: "VerterImports",
  enforce: "post",

  post(s, ctx) {
    const imports = ctx.items.filter((x) => x.type === ProcessItemType.Import);
    if (imports.length === 0) return;

    const importStr = generateImport(imports);
    s.prepend(importStr);
  },
  transformImport(item, s) {
    s.move(item.node.start, item.node.end, 0);
  },
});
