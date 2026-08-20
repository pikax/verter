import { openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

const _hoisted_1 = ["innerHTML"]

import { ref } from "vue"


const _sfc_main = {
  __name: 'html',
  setup(__props) {

const raw = ref("<b>bold</b>")

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("div", { innerHTML: raw.value }, null, 8 /* PROPS */, _hoisted_1))
}
}

}
export default _sfc_main
