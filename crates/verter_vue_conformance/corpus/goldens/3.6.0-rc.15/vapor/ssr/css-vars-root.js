import { useVaporCssVars as _useVaporCssVars, defineVaporComponent as _defineVaporComponent } from "vue";
import { ref } from "vue";
const _sfc_main = /* @__PURE__ */ _defineVaporComponent({
  __name: "css-vars-root",
  setup(__props, { expose: __expose }) {
    __expose();
    _useVaporCssVars((_ctx) => ({
      "89d5a0b5-color": color.value
    }));
    const color = ref("red");
    const __returned__ = { color };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

import { template as _template } from 'vue';
const t0 = _template("<p class=a>text", 3)

function render(_ctx, $props, $emit, $attrs, $slots) {
  const n0 = t0()
  return n0
}
_sfc_main.render = render
export default _sfc_main
