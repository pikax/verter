import { normalizeClass as _normalizeClass, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

import { ref } from "vue"


export default {
  __name: 'dynamic-class',
  setup(__props) {

const active = ref(true)
const size = ref("lg")

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("div", {
    class: _normalizeClass(['card', { active: active.value }, 'size-' + size.value])
  }, "Classy", 2 /* CLASS */))
}
}

}
