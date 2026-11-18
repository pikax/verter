import { resolveComponent as _resolveComponent, withCtx as _withCtx, vModelText as _vModelText, createVNode as _createVNode, withDirectives as _withDirectives } from "vue"
import { ssrRenderAttr as _ssrRenderAttr, ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  _push(_ssrRenderComponent(_component_Shell, _attrs, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(`<input${
          _ssrRenderAttr("value", $setup.value!)
        }${
          _scopeId
        }>`)
      } else {
        return [
          _withDirectives(_createVNode("input", {
            "onUpdate:modelValue": $event => (($setup.value!) = $event)
          }, null, 8 /* PROPS */, ["onUpdate:modelValue"]), [
            [_vModelText, $setup.value!]
          ])
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}
