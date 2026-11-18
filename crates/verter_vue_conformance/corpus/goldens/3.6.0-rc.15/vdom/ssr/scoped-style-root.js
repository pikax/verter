import { defineComponent as _defineComponent } from "vue";
import { ref } from "vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "scoped-style-root",
  setup(__props, { expose: __expose }) {
    __expose();
    const n = ref(1);
    const __returned__ = { n };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

import { toDisplayString as _toDisplayString, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
const _hoisted_1 = { class: "a" }

function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("p", _hoisted_1, _toDisplayString($setup.n), 1 /* TEXT */))
}
_sfc_main.render = render
export default _sfc_main
