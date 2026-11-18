import { vModelText as _vModelText, createElementVNode as _createElementVNode, withDirectives as _withDirectives, toDisplayString as _toDisplayString, createTextVNode as _createTextVNode, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

export function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock(_Fragment, null, [
    _withDirectives(_createElementVNode("input", {
      "onUpdate:modelValue": _cache[0] || (_cache[0] = $event => (($setup.value) = $event))
    }, null, 512 /* NEED_PATCH */), [
      [_vModelText, $setup.value]
    ]),
    _createTextVNode(_toDisplayString($setup.value), 1 /* TEXT */)
  ], 64 /* STABLE_FRAGMENT */))
}
