import { toDisplayString as _toDisplayString, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

import { computed, ref } from "vue"


const _sfc_main = {
  __name: 'interpolation',
  setup(__props) {

const count = ref(1)
const doubled = computed(() => count.value * 2)

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("p", null, " Count: " + _toDisplayString(count.value) + " / Doubled: " + _toDisplayString(doubled.value) + " / Upper: " + _toDisplayString("hi".toUpperCase()) + " / Sign: " + _toDisplayString(count.value > 1 ? "many" : "one"), 1 /* TEXT */))
}
}

}
export default _sfc_main
