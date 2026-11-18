import { unref as _unref, isRef as _isRef, vModelText as _vModelText, createElementVNode as _createElementVNode, withDirectives as _withDirectives, toDisplayString as _toDisplayString, createTextVNode as _createTextVNode, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

import { read } from './state'; 
export default {
  __name: 'App',
  setup(__props) {
let value = read();
return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock(_Fragment, null, [
    _withDirectives(_createElementVNode("input", {
      "onUpdate:modelValue": _cache[0] || (_cache[0] = $event => (_isRef(value) ? (value).value = $event : value = $event))
    }, null, 512 /* NEED_PATCH */), [
      [_vModelText, _unref(value)]
    ]),
    _createTextVNode(_toDisplayString(_unref(value)), 1 /* TEXT */)
  ], 64 /* STABLE_FRAGMENT */))
}
}

}
