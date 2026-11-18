import { defineComponent as _defineComponent } from "vue";
import { ref } from "vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "css-vars-root",
  setup(__props, { expose: __expose }) {
    __expose();
    const color = ref("red");
    const __returned__ = { color };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

import { mergeProps as _mergeProps } from "vue"
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _cssVars = { style: {
  ":--89d5a0b5-color": ($setup.color)
}}
  _push(`<p${_ssrRenderAttrs(_mergeProps({ class: "a" }, _attrs, _cssVars))}>text</p>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
