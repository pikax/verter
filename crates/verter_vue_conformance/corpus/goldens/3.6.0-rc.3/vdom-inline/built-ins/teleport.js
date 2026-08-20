import { createElementVNode as _createElementVNode, Teleport as _Teleport, openBlock as _openBlock, createBlock as _createBlock } from "vue"

import { ref } from "vue"


const _sfc_main = {
  __name: 'teleport',
  setup(__props) {

const open = ref(true)

return (_ctx, _cache) => {
  return (_openBlock(), _createBlock(_Teleport, {
    to: "body",
    disabled: !open.value
  }, [
    _cache[0] || (_cache[0] = _createElementVNode("p", { class: "overlay" }, "Overlay", -1 /* CACHED */))
  ], 8 /* PROPS */, ["disabled"]))
}
}

}
export default _sfc_main
