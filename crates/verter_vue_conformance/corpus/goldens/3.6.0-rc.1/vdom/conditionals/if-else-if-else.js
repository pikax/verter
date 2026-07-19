import { openBlock as _openBlock, createElementBlock as _createElementBlock, createCommentVNode as _createCommentVNode, toDisplayString as _toDisplayString } from "vue"

const _hoisted_1 = { key: 0 }
const _hoisted_2 = { key: 1 }
const _hoisted_3 = { key: 2 }

import { ref } from "vue"


export default {
  __name: 'if-else-if-else',
  setup(__props) {

const status = ref("loading")

return (_ctx, _cache) => {
  return (status.value === 'loading')
    ? (_openBlock(), _createElementBlock("p", _hoisted_1, "Loading"))
    : (status.value === 'error')
      ? (_openBlock(), _createElementBlock("p", _hoisted_2, "Failed"))
      : (_openBlock(), _createElementBlock("p", _hoisted_3, "Done: " + _toDisplayString(status.value), 1 /* TEXT */))
}
}

}
