import { unref as _unref, resolveComponent as _resolveComponent, isRef as _isRef, openBlock as _openBlock, createBlock as _createBlock } from "vue"


export default {
  __name: 'App',
  setup(__props) {
let value = ''; function changed() {}
return (_ctx, _cache) => {
  const _component_Widget = _resolveComponent("Widget")

  return (_openBlock(), _createBlock(_component_Widget, {
    modelValue: _unref(value),
    "onUpdate:modelValue": [
      _cache[0] || (_cache[0] = $event => (_isRef(value) ? (value).value = $event : value = $event)),
      changed
    ]
  }, null, 8 /* PROPS */, ["modelValue"]))
}
}

}
