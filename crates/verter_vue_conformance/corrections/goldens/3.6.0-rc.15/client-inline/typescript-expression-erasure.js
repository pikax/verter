import { defineComponent as _defineComponent } from 'vue'
import { toDisplayString as _toDisplayString, createElementVNode as _createElementVNode, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"


export default /*@__PURE__*/_defineComponent({
  __name: 'App',
  setup(__props) {
const value = {} as { label?: string };
return (_ctx: any,_cache: any) => {
  const _component_Shell = _resolveComponent("Shell")!

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      _createElementVNode("span", null, _toDisplayString((value.label as string)!), 1 /* TEXT */)
    ]),
    _: 1 /* STABLE */
  }))
}
}

})
