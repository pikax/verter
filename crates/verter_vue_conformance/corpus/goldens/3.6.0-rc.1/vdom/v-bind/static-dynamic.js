import { openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'static-dynamic',
  setup(__props, { expose: __expose }) {
  __expose();

const title = ref("Hello")
const disabled = ref(false)

const __returned__ = { title, disabled, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
const _hoisted_1 = ["title", "disabled"]

function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("button", {
    type: "button",
    title: $setup.title,
    disabled: $setup.disabled
  }, "Go", 8 /* PROPS */, _hoisted_1))
}
_sfc_main.render = render
export default _sfc_main
