import { normalizeStyle as _normalizeStyle, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

import { ref } from "vue"


export default {
  __name: 'dynamic-style',
  setup(__props) {

const color = ref("red")
const top = ref(10)

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("div", {
    style: _normalizeStyle({ color: color.value, marginTop: top.value + 'px' })
  }, "Styled", 4 /* STYLE */))
}
}

}
