import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, toDisplayString as _toDisplayString } from "vue"

import { ref } from "vue"


export default {
  __name: 'index-destructure',
  setup(__props) {

const users = ref([
  { id: 1, name: "Ada" },
  { id: 2, name: "Bo" },
])

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("ul", null, [
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList(users.value, ({ id, name }, index) => {
      return (_openBlock(), _createElementBlock("li", { key: id }, _toDisplayString(index) + " — " + _toDisplayString(name), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */))
  ]))
}
}

}
