import { mergeProps as _mergeProps, createElementVNode as _createElementVNode, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"

const _hoisted_1 = ["onClick", "title"]

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(({ item }) => [
      _createElementVNode("button", _mergeProps($setup.attrs, {
        onClick: $event => ($setup.pick(item)),
        title: $setup.label
      }), "x", 16 /* FULL_PROPS */, _hoisted_1),
      _createElementVNode("p", _mergeProps($setup.attrs, { ref: $setup.setBox }), "y", 16 /* FULL_PROPS */)
    ]),
    _: 1 /* STABLE */
  }))
}
