import { defineComponent as _defineComponent } from "vue";
import ChildComp from "../components/child-comp.vue";
const _sfc_main = /* @__PURE__ */ _defineComponent({
  __name: "scoped-slot-branches",
  setup(__props, { expose: __expose }) {
    __expose();
    const __returned__ = { ChildComp };
    Object.defineProperty(__returned__, "__isScriptSetup", { enumerable: false, value: true });
    return __returned__;
  }
});

import { openBlock as _openBlock, createElementBlock as _createElementBlock, createCommentVNode as _createCommentVNode, toDisplayString as _toDisplayString, createTextVNode as _createTextVNode, Fragment as _Fragment, withCtx as _withCtx, createBlock as _createBlock } from "vue"
const _hoisted_1 = { key: 0 }

function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createBlock($setup["ChildComp"], null, {
    cell: _withCtx(({ row }) => [
      (row.a)
        ? (_openBlock(), _createElementBlock("span", _hoisted_1, "a"))
        : (_openBlock(), _createElementBlock(_Fragment, { key: 1 }, [
            _createTextVNode(_toDisplayString(row.b), 1 /* TEXT */)
          ], 64 /* STABLE_FRAGMENT */))
    ]),
    _: 1 /* STABLE */
  }))
}
_sfc_main.render = render
export default _sfc_main
