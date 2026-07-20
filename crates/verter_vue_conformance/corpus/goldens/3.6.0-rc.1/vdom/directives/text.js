import { toDisplayString as _toDisplayString, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'text',
  setup(__props, { expose: __expose }) {
  __expose();

const plain = ref("plain text")

const __returned__ = { plain, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const _hoisted_1 = ["textContent"]

function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("p", {
    textContent: _toDisplayString($setup.plain)
  }, null, 8 /* PROPS */, _hoisted_1))
}
_sfc_main.render = render
export default _sfc_main
