import { resolveComponent as _resolveComponent, withCtx as _withCtx, createVNode as _createVNode, toDisplayString as _toDisplayString } from "vue"
import { ssrRenderComponent as _ssrRenderComponent, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  _push(_ssrRenderComponent(_component_Shell, _attrs, {
    extra: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`<b${_scopeId}></b>`)
      } else {
        return [
          _createVNode("b")
        ]
      }
    }),
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`<i${
          _scopeId
        }>${
          _ssrInterpolate($setup.label)
        }</i>`)
      } else {
        return [
          _createVNode("i", null, _toDisplayString($setup.label))
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}
