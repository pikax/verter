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

import { createTextVNode as _createTextVNode, withCtx as _withCtx, createSlots as _createSlots, openBlock as _openBlock, createBlock as _createBlock } from "vue"
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createBlock($setup["ChildComp"], null, _createSlots({ _: 2 /* DYNAMIC */ }, [
    true
      ? {
          name: "actions",
          fn: _withCtx(() => [
            _cache[0] || (_cache[0] = _createTextVNode("x", -1 /* CACHED */))
          ]),
          key: "0"
        }
      : undefined
  ]), 1024 /* DYNAMIC_SLOTS */))
}
_sfc_main.render = render
export default _sfc_main
