import { defineComponent as _defineComponent } from "vue";
import ChildComp from "../components/child-comp.vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "conditional-slot-static",
  setup(__props, { expose: __expose }) {
    __expose();
    const __returned__ = { ChildComp };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

import { withCtx as _withCtx, createTextVNode as _createTextVNode, createSlots as _createSlots } from "vue"
import { ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(_ssrRenderComponent($setup["ChildComp"], _attrs, _createSlots({ _: 2 /* DYNAMIC */ }, [
    true
      ? {
          name: "actions",
          fn: _withCtx((_, _push, _parent, _scopeId) => {
            if (_push) {
              _push(`x`)
            } else {
              return [
                _createTextVNode("x")
              ]
            }
          }),
          key: "0"
        }
      : undefined
  ]), _parent))
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
