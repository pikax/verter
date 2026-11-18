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

import { createElementVNode as _createElementVNode, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createBlock($setup["ChildComp"], null, {
    default: _withCtx(() => [
      _createElementVNode("button", {
        onClick: _cache[0] || (_cache[0] = $event => {$setup.a = ''; $setup.b = ''})
      }, "reset")
    ]),
    _: 1 /* STABLE */
  }))
}
_sfc_main.render = render
export default _sfc_main
