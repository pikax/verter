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

import { mergeProps as _mergeProps } from "vue"
import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<p${
    _ssrRenderAttrs(_mergeProps({ class: "a" }, _attrs))
  } data-v-7d1ce19b>${
    _ssrInterpolate($setup.n)
  }</p>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
