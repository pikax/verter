import { vModelText as _vModelText, withDirectives as _withDirectives, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

import { ref } from "vue"


const _sfc_main = {
  __name: 'input',
  setup(__props) {

const text = ref("")

return (_ctx, _cache) => {
  return _withDirectives((_openBlock(), _createElementBlock("input", {
    "onUpdate:modelValue": _cache[0] || (_cache[0] = $event => ((text).value = $event)),
    placeholder: "Type here"
  }, null, 512 /* NEED_PATCH */)), [
    [_vModelText, text.value]
  ])
}
}

}
export default _sfc_main
