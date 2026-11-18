import { defineComponent as _defineComponent } from "vue";
import { ref } from "vue";
import ChildComp from "../components/child-comp.vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "slot-handler-statements",
  setup(__props, { expose: __expose }) {
    __expose();
    const a = ref("");
    const b = ref("");
    const __returned__ = { a, b, ChildComp };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

import { withCtx as _withCtx, createVNode as _createVNode } from "vue"
import { ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(_ssrRenderComponent($setup["ChildComp"], _attrs, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`<button${_scopeId}>reset</button>`)
      } else {
        return [
          _createVNode("button", {
            onClick: $event => {$setup.a = ''; $setup.b = ''}
          }, "reset", 8 /* PROPS */, ["onClick"])
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
