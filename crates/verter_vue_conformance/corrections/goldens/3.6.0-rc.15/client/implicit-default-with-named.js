import { createElementVNode as _createElementVNode, toDisplayString as _toDisplayString, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    extra: _withCtx(() => [...(_cache[0] || (_cache[0] = [
      _createElementVNode("b", null, null, -1 /* CACHED */)
    ]))]),
    default: _withCtx(() => [
      _createElementVNode("i", null, _toDisplayString($setup.label))
    ]),
    _: 1 /* STABLE */
  }))
}
