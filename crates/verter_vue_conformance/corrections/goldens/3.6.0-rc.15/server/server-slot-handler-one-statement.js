import { withCtx as _withCtx, createVNode as _createVNode } from "vue"
import { ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(_ssrRenderComponent($setup["Child"], _attrs, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`<button${_scopeId}>x</button>`)
      } else {
        return [
          _createVNode("button", {
            onClick: $event => ($setup.a = '')
          }, "x", 8 /* PROPS */, ["onClick"])
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}