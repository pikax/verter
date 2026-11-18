import { toDisplayString as _toDisplayString, createElementVNode as _createElementVNode, createTextVNode as _createTextVNode, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

const label = 'x';
export default {
  __name: 'App',
  setup(__props) {

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("div", null, [
    _createElementVNode("span", null, _toDisplayString(label)),
    _cache[0] || (_cache[0] = _createTextVNode()),
    _cache[1] || (_cache[1] = _createElementVNode("i", null, null, -1 /* CACHED */))
  ]))
}
}

}
