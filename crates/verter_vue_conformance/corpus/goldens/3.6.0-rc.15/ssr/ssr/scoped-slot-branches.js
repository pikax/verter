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

import { withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock, createCommentVNode as _createCommentVNode, toDisplayString as _toDisplayString, createTextVNode as _createTextVNode, Fragment as _Fragment } from "vue"
import { ssrRenderComponent as _ssrRenderComponent, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(_ssrRenderComponent($setup["ChildComp"], _attrs, {
    cell: _withCtx(({ row }, _push, _parent, _scopeId) => {
      if (_push) {
        if (row.a) {
          _push(`<span${_scopeId}>a</span>`)
        } else {
          _push(`<!--[-->${_ssrInterpolate(row.b)}<!--]-->`)
        }
      } else {
        return [
          (row.a)
            ? (_openBlock(), _createBlock("span", { key: 0 }, "a"))
            : (_openBlock(), _createBlock(_Fragment, { key: 1 }, [
                _createTextVNode(_toDisplayString(row.b), 1 /* TEXT */)
              ], 64 /* STABLE_FRAGMENT */))
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
