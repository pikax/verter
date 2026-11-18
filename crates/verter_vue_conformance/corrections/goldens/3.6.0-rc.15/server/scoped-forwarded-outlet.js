import { resolveComponent as _resolveComponent, withCtx as _withCtx, renderSlot as _renderSlot } from "vue"
import { ssrRenderSlot as _ssrRenderSlot, ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  _push(_ssrRenderComponent(_component_Shell, _attrs, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _ssrRenderSlot(_ctx.$slots, "default", { label: $setup.label }, null, _push, _parent, "data-v-0d04bfeb-s" + _scopeId)
      } else {
        return [
          _renderSlot(_ctx.$slots, "default", { label: $setup.label })
        ]
      }
    }),
    _: 3 /* FORWARDED */
  }, _parent))
}
