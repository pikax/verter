import ChildComp from "./child-comp.vue"

const _sfc_main = {
  __name: 'slots',
  setup(__props, { expose: __expose }) {
  __expose();


const __returned__ = { ChildComp }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { withCtx as _withCtx, createVNode as _createVNode, toDisplayString as _toDisplayString, mergeProps as _mergeProps } from "vue"
import { ssrRenderComponent as _ssrRenderComponent, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(_ssrRenderComponent($setup["ChildComp"], _mergeProps({ label: "Slotted" }, _attrs), {
    header: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`<h1${_scopeId}>Title</h1>`)
      } else {
        return [
          _createVNode("h1", null, "Title")
        ]
      }
    }),
    default: _withCtx(({ rows }, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`<p${
          _scopeId
        }>${
          _ssrInterpolate(rows.length)
        } rows</p>`)
      } else {
        return [
          _createVNode("p", null, _toDisplayString(rows.length) + " rows", 1 /* TEXT */)
        ]
      }
    }),
    footer: _withCtx(({ total }, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`<span${
          _scopeId
        }>Total ${
          _ssrInterpolate(total)
        }</span>`)
      } else {
        return [
          _createVNode("span", null, "Total " + _toDisplayString(total), 1 /* TEXT */)
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
