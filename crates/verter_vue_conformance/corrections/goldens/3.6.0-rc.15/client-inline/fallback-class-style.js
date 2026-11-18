import { normalizeClass as _normalizeClass, createElementVNode as _createElementVNode, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"


export default {
  __name: 'App',
  setup(__props) {
const cls = ['x']; const style = { color: 'red' };
return (_ctx, _cache) => {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      _createElementVNode("div", {
        class: _normalizeClass(cls),
        style: style
      })
    ]),
    _: 1 /* STABLE */
  }))
}
}

}
