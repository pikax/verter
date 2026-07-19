import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, toDisplayString as _toDisplayString } from "vue"

import { ref } from "vue"


export default {
  __name: 'object',
  setup(__props) {

const scores = ref({ alice: 3, bob: 5 })

return (_ctx, _cache) => {
  return (_openBlock(), _createElementBlock("ul", null, [
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList(scores.value, (score, name) => {
      return (_openBlock(), _createElementBlock("li", { key: name }, _toDisplayString(name) + ": " + _toDisplayString(score), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */))
  ]))
}
}

}
