import { resolveComponent as _resolveComponent, withCtx as _withCtx, createVNode as _createVNode } from "vue"
import { ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  _push(_ssrRenderComponent(_component_Shell, _attrs, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`<button${
          _scopeId
        }></button><button${
          _scopeId
        }></button>`)
      } else {
        return [
          _createVNode("button", {
            onClick: $event => ($setup.count++)
          }, null, 8 /* PROPS */, ["onClick"]),
          _createVNode("button", {
            onClick: $event => {$setup.count++; $setup.count++}
          }, null, 8 /* PROPS */, ["onClick"])
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}
