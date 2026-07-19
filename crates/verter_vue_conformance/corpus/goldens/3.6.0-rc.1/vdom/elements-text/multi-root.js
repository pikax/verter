import { createElementVNode as _createElementVNode, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

export function render(_ctx, _cache) {
  return (_openBlock(), _createElementBlock(_Fragment, null, [
    _cache[0] || (_cache[0] = _createElementVNode("header", null, "Header", -1 /* CACHED */)),
    _cache[1] || (_cache[1] = _createElementVNode("main", null, "Body", -1 /* CACHED */)),
    _cache[2] || (_cache[2] = _createElementVNode("footer", null, "Footer", -1 /* CACHED */))
  ], 64 /* STABLE_FRAGMENT */))
}
