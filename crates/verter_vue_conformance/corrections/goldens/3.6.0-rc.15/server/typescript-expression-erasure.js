import { resolveComponent as _resolveComponent, withCtx as _withCtx, toDisplayString as _toDisplayString, createVNode as _createVNode } from "vue"
import { ssrRenderComponent as _ssrRenderComponent, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  _push(_ssrRenderComponent(_component_Shell, _attrs, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`<span${
          _scopeId
        }>${
          _ssrInterpolate(($setup.value.label as string)!)
        }</span>`)
      } else {
        return [
          _createVNode("span", null, _toDisplayString(($setup.value.label as string)!), 1 /* TEXT */)
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}
