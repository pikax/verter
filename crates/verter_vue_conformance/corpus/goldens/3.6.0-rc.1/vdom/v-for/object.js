import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, toDisplayString as _toDisplayString } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'object',
  setup(__props, { expose: __expose }) {
  __expose();

const scores = ref({ alice: 3, bob: 5 })

const __returned__ = { scores, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("ul", null, [
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList($setup.scores, (score, name) => {
      return (_openBlock(), _createElementBlock("li", { key: name }, _toDisplayString(name) + ": " + _toDisplayString(score), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */))
  ]))
}
_sfc_main.render = render
export default _sfc_main
