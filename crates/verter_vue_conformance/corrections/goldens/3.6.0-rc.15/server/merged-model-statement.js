import { resolveComponent as _resolveComponent, withCtx as _withCtx, createVNode as _createVNode } from "vue"
import { ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")
  const _component_Widget = _resolveComponent("Widget")

  _push(_ssrRenderComponent(_component_Shell, _attrs, {
    default: _withCtx((_, _push, _parent, _scopeId) => {
      if (_push) {
        _push(_ssrRenderComponent(_component_Widget, {
          modelValue: $setup.value,
          "onUpdate:modelValue": [$event => (($setup.value) = $event), $event => ($setup.failure = null)]
        }, null, _parent, _scopeId))
      } else {
        return [
          _createVNode(_component_Widget, {
            modelValue: $setup.value,
            "onUpdate:modelValue": [$event => (($setup.value) = $event), $event => ($setup.failure = null)]
          }, null, 8 /* PROPS */, ["modelValue", "onUpdate:modelValue"])
        ]
      }
    }),
    _: 1 /* STABLE */
  }, _parent))
}
