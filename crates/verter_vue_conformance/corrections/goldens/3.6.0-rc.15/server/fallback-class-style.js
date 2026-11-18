import { resolveComponent as _resolveComponent, withCtx as _withCtx, createVNode as _createVNode } from "vue"
import { ssrRenderClass as _ssrRenderClass, ssrRenderStyle as _ssrRenderStyle, ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  _push(_ssrRenderComponent(_component_Shell, _attrs, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`<div class="${
          _ssrRenderClass($setup.cls)
        }" style="${
          _ssrRenderStyle($setup.style)
        }"${
          _scopeId
        }></div>`)
      } else {
        return [
          _createVNode("div", {
            class: $setup.cls,
            style: $setup.style
          })
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}
