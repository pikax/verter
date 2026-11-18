import { resolveComponent as _resolveComponent, withCtx as _withCtx, toDisplayString as _toDisplayString, createVNode as _createVNode, createTextVNode as _createTextVNode } from "vue"
import { ssrRenderComponent as _ssrRenderComponent, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  _push(_ssrRenderComponent(_component_Shell, _attrs, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`${
          _ssrInterpolate($setup.label)
        } <b${
          _scopeId
        }></b>`)
      } else {
        return [
          _createTextVNode(_toDisplayString($setup.label) + " "),
          _createVNode("b")
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}
