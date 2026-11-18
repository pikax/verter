import { vModelText as _vModelText, createElementVNode as _createElementVNode, withDirectives as _withDirectives, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      _withDirectives(_createElementVNode("input", {
        "onUpdate:modelValue": _cache[0] || (_cache[0] = $event => (($setup.value) = $event))
      }, null, 512 /* NEED_PATCH */), [
        [_vModelText, $setup.value]
      ])
    ]),
    _: 1 /* STABLE */
  }))
}
