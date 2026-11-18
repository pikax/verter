import { resolveComponent as _resolveComponent, createVNode as _createVNode, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"

const value = 'x';
export default {
  __name: 'App',
  setup(__props) {

return (_ctx, _cache) => {
  const _component_Widget = _resolveComponent("Widget")
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      _createVNode(_component_Widget, { "page-key": value })
    ]),
    _: 1 /* STABLE */
  }))
}
}

}
