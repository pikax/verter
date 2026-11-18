const _sfc_main = {}
import { resolveDynamicComponent as _resolveDynamicComponent, withCtx as _withCtx, renderSlot as _renderSlot, createVNode as _createVNode } from "vue"
import { ssrRenderSlot as _ssrRenderSlot, ssrRenderVNode as _ssrRenderVNode } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs) {
  _push(`<!--[-->`)
  _ssrRenderVNode(_push, _createVNode(_resolveDynamicComponent(_ctx.current), { class: "control" }, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _ssrRenderSlot(_ctx.$slots, "default", {}, null, _push, _parent, _scopeId)
      } else {
        return [
          _renderSlot(_ctx.$slots, "default")
        ]
      }
    }),
    _: 3 /* FORWARDED */
  }), _parent)
  _push(`<input type="checkbox"><!--]-->`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
