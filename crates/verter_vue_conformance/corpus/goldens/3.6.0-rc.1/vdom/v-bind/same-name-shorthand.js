import { openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'same-name-shorthand',
  setup(__props, { expose: __expose }) {
  __expose();

const id = ref("a1")
const title = ref("Hi")

const __returned__ = { id, title, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const _hoisted_1 = ["id", "title"]

function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("div", {
    id: $setup.id,
    title: $setup.title
  }, null, 8 /* PROPS */, _hoisted_1))
}
_sfc_main.render = render
export default _sfc_main
