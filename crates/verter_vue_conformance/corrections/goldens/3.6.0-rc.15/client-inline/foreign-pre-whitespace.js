import { toDisplayString as _toDisplayString, createElementVNode as _createElementVNode, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

const n = 1;
export default {
  __name: 'App',
  setup(__props) {

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock(_Fragment, null, [
    _createElementVNode("svg", null, [
      _createElementVNode("pre", null, _toDisplayString(n) + "a b")
    ]),
    _createElementVNode("math", null, [
      _createElementVNode("mtext", null, [
        _createElementVNode("pre", null, _toDisplayString(n) + "c  d")
      ]),
      _createElementVNode("mi", null, [
        _createElementVNode("mglyph", null, [
          _createElementVNode("pre", null, _toDisplayString(n) + "e f")
        ])
      ])
    ])
  ], 64 /* STABLE_FRAGMENT */))
}
}

}
