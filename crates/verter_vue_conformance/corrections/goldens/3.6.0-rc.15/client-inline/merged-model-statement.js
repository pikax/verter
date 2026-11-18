import { unref as _unref, resolveComponent as _resolveComponent, isRef as _isRef, createVNode as _createVNode, withCtx as _withCtx, openBlock as _openBlock, createBlock as _createBlock } from "vue"


export default {
  __name: 'App',
  setup(__props) {
let value = ''; let failure = null;
return (_ctx, _cache) => {
  const _component_Widget = _resolveComponent("Widget")
  const _component_Shell = _resolveComponent("Shell")

  return (_openBlock(), _createBlock(_component_Shell, null, {
    default: _withCtx(() => [
      _createVNode(_component_Widget, {
        modelValue: _unref(value),
        "onUpdate:modelValue": [
          _cache[0] || (_cache[0] = $event => (_isRef(value) ? (value).value = $event : value = $event)),
          _cache[1] || (_cache[1] = $event => (_isRef(failure) ? failure.value = null : failure = null))
        ]
      }, null, 8 /* PROPS */, ["modelValue"])
    ]),
    _: 1 /* STABLE */
  }))
}
}

}
