import { defineComponent as _defineComponent } from 'vue'
import { unref as _unref, vModelText as _vModelText, createElementVNode as _createElementVNode, withDirectives as _withDirectives, resolveComponent as _resolveComponent, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"


export default /*@__PURE__*/_defineComponent({
  __name: 'App',
  setup(__props) {
let value: string | undefined = '';
return (_ctx: any,_cache: any) => {
  const _component_Shell = _resolveComponent("Shell")!

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      _withDirectives(_createElementVNode("input", {
        "onUpdate:modelValue": _cache[0] || (_cache[0] = ($event: any) => ((_unref(value)!) = $event))
      }, null, 512 /* NEED_PATCH */), [
        [_vModelText, _unref(value)!]
      ])
    ]),
    _: 1 /* STABLE */
  }))
}
}

})
