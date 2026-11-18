import { resolveComponent as _resolveComponent, openBlock as _openBlock, createBlock as _createBlock } from "vue"

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  const _component_Widget = _resolveComponent("Widget")

  return (_openBlock(), _createBlock(_component_Widget, {
    modelValue: $setup.value,
    "onUpdate:modelValue": [
      _cache[0] || (_cache[0] = $event => (($setup.value) = $event)),
      $setup.changed
    ]
  }, null, 8 /* PROPS */, ["modelValue"]))
}
