import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, toDisplayString as _toDisplayString } from "vue"

import { ref } from "vue"


const _sfc_main = {
  __name: 'array',
  setup(__props) {

const items = ref(["a", "b", "c"])

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("ul", null, [
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList(items.value, (item) => {
      return (_openBlock(), _createElementBlock("li", { key: item }, _toDisplayString(item), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */))
  ]))
}
}

}
export default _sfc_main
