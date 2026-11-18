import { resolveComponent as _resolveComponent, mergeProps as _mergeProps } from "vue"
import { ssrRenderComponent as _ssrRenderComponent } from "vue/server-renderer"

export function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  const _component_Widget = _resolveComponent("Widget")

  _push(_ssrRenderComponent(_component_Widget, _mergeProps({
    modelValue: $setup.value,
    "onUpdate:modelValue": [$event => (($setup.value) = $event), $setup.changed]
  }, _attrs), null, _parent))
}
