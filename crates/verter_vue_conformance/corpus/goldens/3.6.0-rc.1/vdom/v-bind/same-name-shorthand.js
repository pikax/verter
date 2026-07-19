import { openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

const _hoisted_1 = ["id", "title"]

import { ref } from "vue"


export default {
  __name: 'same-name-shorthand',
  setup(__props) {

const id = ref("a1")
const title = ref("Hi")

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("div", {
    id: id.value,
    title: title.value
  }, null, 8 /* PROPS */, _hoisted_1))
}
}

}
