import { toDisplayString as _toDisplayString, createElementVNode as _createElementVNode, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock(_Fragment, null, [
    _createElementVNode("svg", null, [
      _createElementVNode("pre", null, _toDisplayString($setup.n) + "a b")
    ]),
    _createElementVNode("math", null, [
      _createElementVNode("mtext", null, [
        _createElementVNode("pre", null, _toDisplayString($setup.n) + "c  d")
      ]),
      _createElementVNode("mi", null, [
        _createElementVNode("mglyph", null, [
          _createElementVNode("pre", null, _toDisplayString($setup.n) + "e f")
        ])
      ])
    ])
  ], 64 /* STABLE_FRAGMENT */))
}
