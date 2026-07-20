import { renderList as _renderList, Fragment as _Fragment, openBlock as _openBlock, createElementBlock as _createElementBlock, toDisplayString as _toDisplayString } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'index-destructure',
  setup(__props, { expose: __expose }) {
  __expose();

const users = ref([
  { id: 1, name: "Ada" },
  { id: 2, name: "Bo" },
])

const __returned__ = { users, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("ul", null, [
    (_openBlock(true), _createElementBlock(_Fragment, null, _renderList($setup.users, ({ id, name }, index) => {
      return (_openBlock(), _createElementBlock("li", { key: id }, _toDisplayString(index) + " — " + _toDisplayString(name), 1 /* TEXT */))
    }), 128 /* KEYED_FRAGMENT */))
  ]))
}
_sfc_main.render = render
export default _sfc_main
