import { toDisplayString as _toDisplayString, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

const _hoisted_1 = ["textContent"]

import { ref } from "vue"


const _sfc_main = {
  __name: 'text',
  setup(__props) {

const plain = ref("plain text")

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("p", {
    textContent: _toDisplayString(plain.value)
  }, null, 8 /* PROPS */, _hoisted_1))
}
}

}
export default _sfc_main
