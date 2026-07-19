import { normalizeProps as _normalizeProps, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

import { ref } from "vue"


export default {
  __name: 'dynamic-arg',
  setup(__props) {

const attrName = ref("title")
const value = ref("Tooltip")

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("p", _normalizeProps({ [attrName.value || ""]: value.value }), "Dynamic attribute", 16 /* FULL_PROPS */))
}
}

}
