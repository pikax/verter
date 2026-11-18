import { openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

const _hoisted_1 = ["title", "disabled"]

import { ref } from "vue"


const _sfc_main = {
  __name: 'static-dynamic',
  setup(__props) {

const title = ref("Hello")
const disabled = ref(false)

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("button", {
    type: "button",
    title: title.value,
    disabled: disabled.value
  }, "Go", 8 /* PROPS */, _hoisted_1))
}
}

}
export default _sfc_main
