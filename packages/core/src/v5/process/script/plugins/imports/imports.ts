import { definePlugin } from "../../types";

// move imports to the top of the file
export const ImportsPlugin = definePlugin({
  name: "VerterImports",
  transformImport(item, s) {
    s.move(item.node.start, item.node.end, 0);
  },
});
