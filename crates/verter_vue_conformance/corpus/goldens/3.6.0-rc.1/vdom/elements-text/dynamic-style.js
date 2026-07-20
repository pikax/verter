import { normalizeStyle as _normalizeStyle, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'dynamic-style',
  setup(__props, { expose: __expose }) {
  __expose();

const color = ref("red")
const top = ref(10)

const __returned__ = { color, top, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("div", {
    style: _normalizeStyle({ color: $setup.color, marginTop: $setup.top + 'px' })
  }, "Styled", 4 /* STYLE */))
}
_sfc_main.render = render
export default _sfc_main
