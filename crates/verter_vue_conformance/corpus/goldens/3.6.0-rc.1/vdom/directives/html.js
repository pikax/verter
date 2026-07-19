import { openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'html',
  setup(__props, { expose: __expose }) {
  __expose();

const raw = ref("<b>bold</b>")

const __returned__ = { raw, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const _hoisted_1 = ["innerHTML"]

function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("div", { innerHTML: $setup.raw }, null, 8 /* PROPS */, _hoisted_1))
}
_sfc_main.render = render
export default _sfc_main
