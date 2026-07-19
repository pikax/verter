import { toDisplayString as _toDisplayString, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

import { ref } from "vue"


export default {
  __name: 'inline',
  setup(__props) {

const count = ref(0)

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("button", {
    onClick: _cache[0] || (_cache[0] = $event => (count.value++))
  }, "Count: " + _toDisplayString(count.value), 1 /* TEXT */))
}
}

}
