import { toDisplayString as _toDisplayString, createElementVNode as _createElementVNode, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      _createElementVNode("span", null, _toDisplayString(($setup.value.label as string)!), 1 /* TEXT */)
    ]),
    _: 1 /* STABLE */
  }))
}
