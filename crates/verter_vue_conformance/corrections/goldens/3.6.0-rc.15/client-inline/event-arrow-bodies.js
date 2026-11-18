import { isRef as _isRef, createElementVNode as _createElementVNode, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"


export default {
  __name: 'App',
  setup(__props) {
let count = 0;
return (_ctx, _cache) => {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      _createElementVNode("button", {
        onClick: _cache[0] || (_cache[0] = $event => (_isRef(count) ? count.value++ : count++))
      }),
      _createElementVNode("button", {
        onClick: _cache[1] || (_cache[1] = $event => {_isRef(count) ? count.value++ : count++; _isRef(count) ? count.value++ : count++})
      })
    ]),
    _: 1 /* STABLE */
  }))
}
}

}
