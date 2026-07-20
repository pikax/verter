import { openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'prop-attr-modifiers',
  setup(__props, { expose: __expose }) {
  __expose();

const text = ref("inner")
const dataX = ref("x")

const __returned__ = { text, dataX, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const _hoisted_1 = [".text-content", "^data-x"]

function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("div", {
    ".text-content": $setup.text,
    "^data-x": $setup.dataX
  }, null, 40 /* PROPS, NEED_HYDRATION */, _hoisted_1))
}
_sfc_main.render = render
export default _sfc_main
