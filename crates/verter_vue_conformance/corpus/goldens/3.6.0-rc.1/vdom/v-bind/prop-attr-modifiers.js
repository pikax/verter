import { openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

const _hoisted_1 = [".text-content", "^data-x"]

import { ref } from "vue"


export default {
  __name: 'prop-attr-modifiers',
  setup(__props) {

const text = ref("inner")
const dataX = ref("x")

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("div", {
    ".text-content": text.value,
    "^data-x": dataX.value
  }, null, 40 /* PROPS, NEED_HYDRATION */, _hoisted_1))
}
}

}
