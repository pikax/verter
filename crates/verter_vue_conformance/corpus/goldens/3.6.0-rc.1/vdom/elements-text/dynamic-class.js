import { normalizeClass as _normalizeClass, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'dynamic-class',
  setup(__props, { expose: __expose }) {
  __expose();

const active = ref(true)
const size = ref("lg")

const __returned__ = { active, size, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("div", {
    class: _normalizeClass(['card', { active: $setup.active }, 'size-' + $setup.size])
  }, "Classy", 2 /* CLASS */))
}
_sfc_main.render = render
export default _sfc_main
