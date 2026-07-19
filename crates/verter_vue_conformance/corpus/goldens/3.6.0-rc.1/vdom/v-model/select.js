import { createElementVNode as _createElementVNode, vModelSelect as _vModelSelect, withDirectives as _withDirectives, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

import { ref } from "vue"


export default {
  __name: 'select',
  setup(__props) {

const picked = ref("a")

return (_ctx, _cache) => {
  return _withDirectives((_openBlock(), _createElementBlock("select", {
    "onUpdate:modelValue": _cache[0] || (_cache[0] = $event => ((picked).value = $event))
  }, [...(_cache[1] || (_cache[1] = [
    _createElementVNode("option", { value: "a" }, "A", -1 /* CACHED */),
    _createElementVNode("option", { value: "b" }, "B", -1 /* CACHED */)
  ]))], 512 /* NEED_PATCH */)), [
    [_vModelSelect, picked.value]
  ])
}
}

}
