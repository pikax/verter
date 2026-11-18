import { createElementVNode as _createElementVNode, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"


export default {
  __name: 'App',
  setup(__props) {
function act() {}
return (_ctx, _cache) => {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      _createElementVNode("button", {
        onClick: _cache[0] || (_cache[0] = $event => (act()))
      })
    ]),
    _: 1 /* STABLE */
  }))
}
}

}
