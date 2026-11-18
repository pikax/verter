import { toDisplayString as _toDisplayString, createElementVNode as _createElementVNode, createTextVNode as _createTextVNode, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"

const label = 'x';
export default {
  __name: 'App',
  setup(__props) {

return (_ctx, _cache) => {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      _createTextVNode(_toDisplayString(label) + " "),
      _cache[0] || (_cache[0] = _createElementVNode("b", null, null, -1 /* CACHED */))
    ]),
    _: 1 /* STABLE */
  }))
}
}

}
