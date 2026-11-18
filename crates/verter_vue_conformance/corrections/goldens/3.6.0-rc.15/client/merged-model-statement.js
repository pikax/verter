import { resolveComponent as _resolveComponent, createVNode as _createVNode, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  const _component_Widget = _resolveComponent("Widget")
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      _createVNode(_component_Widget, {
        modelValue: $setup.value,
        "onUpdate:modelValue": [
          _cache[0] || (_cache[0] = $event => (($setup.value) = $event)),
          _cache[1] || (_cache[1] = $event => ($setup.failure = null))
        ]
      }, null, 8 /* PROPS */, ["modelValue"])
    ]),
    _: 1 /* STABLE */
  }))
}
