import { resolveComponent as _resolveComponent, withCtx as _withCtx, createVNode as _createVNode, createSlots as _createSlots } from "vue"
import { ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  _push(_ssrRenderComponent(_component_Shell, _attrs, _createSlots({ _: 2 /* DYNAMIC */ }, [
    ($setup.visible)
      ? {
          name: "extra",
          fn: _withCtx((_, _push, _parent, _scopeId) => {
            if (_push) {
              _push(`<b${_scopeId}></b>`)
            } else {
              return [
                _createVNode("b")
              ]
            }
          }),
          key: "0"
        }
      : undefined
  ]), _parent))
}
