import { createElementVNode as _createElementVNode, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"

const _hoisted_1 = ["onClick"]


export default {
  __name: 'App',
  setup(__props) {
function pick(item) {}
return (_ctx, _cache) => {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(({ item }) => [
      _createElementVNode("button", {
        onClick: $event => (pick(item))
      }, "x", 8 /* PROPS */, _hoisted_1),
      _createElementVNode("button", {
        onClick: _cache[0] || (_cache[0] = $event => (pick(0)))
      }, "y")
    ]),
    _: 1 /* STABLE */
  }))
}
}

}
