import { vModelCheckbox as _vModelCheckbox, createElementVNode as _createElementVNode, withDirectives as _withDirectives, createTextVNode as _createTextVNode, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

import { ref } from "vue"


const _sfc_main = {
  __name: 'checkbox',
  setup(__props) {

const agreed = ref(false)

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("label", null, [
    _withDirectives(_createElementVNode("input", {
      type: "checkbox",
      "onUpdate:modelValue": _cache[0] || (_cache[0] = $event => ((agreed).value = $event))
    }, null, 512 /* NEED_PATCH */), [
      [_vModelCheckbox, agreed.value]
    ]),
    _cache[1] || (_cache[1] = _createTextVNode(" Agree", -1 /* CACHED */))
  ]))
}
}

}
export default _sfc_main
