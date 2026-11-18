import { renderSlot as _renderSlot, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"

const label = 'x';
export default {
  __name: 'App',
  setup(__props) {

return (_ctx, _cache) => {
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      _renderSlot(_ctx.$slots, "default", { label: label })
    ]),
    _: 3 /* FORWARDED */
  }))
}
}

}
