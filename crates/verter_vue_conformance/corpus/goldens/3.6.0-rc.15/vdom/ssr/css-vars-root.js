import { useCssVars as _useCssVars, defineComponent as _defineComponent } from "vue";
import { ref } from "vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "css-vars-root",
  setup(__props, { expose: __expose }) {
    __expose();
    _useCssVars((_ctx) => ({
      "89d5a0b5-color": color.value
    }));
    const color = ref("red");
    const __returned__ = { color };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

import { openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
const _hoisted_1 = { class: "a" }

function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("p", _hoisted_1, "text"))
}
_sfc_main.render = render
export default _sfc_main
