import { ParsedBlockScript } from "../../../../parser/types";
import { definePlugin } from "../../types";

const Macros = new Set([
  "defineProps",
  "defineEmits",
  "defineExpose",
  "defineOptions",
  "defineModel",
  "defineSlots",
  "withDefaults",

  // useSlots()/useAttrs()
]);

export const MacrosPlugin = definePlugin({
  name: "VerterMacro",

  transformDeclaration(item, s, ctx) {
    console.log("sss", item);
  },
  transformBinding(item, s, ctx) {
    console.log("sss", item);
  },

  transformFunctionCall(item, s, ctx) {
    if (!Macros.has(item.name)) {
      return;
    }
    console.log("sss", item);
  },
});
