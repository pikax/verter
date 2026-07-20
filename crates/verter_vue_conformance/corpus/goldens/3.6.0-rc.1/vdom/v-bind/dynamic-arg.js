import { normalizeProps as _normalizeProps, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'dynamic-arg',
  setup(__props, { expose: __expose }) {
  __expose();

const attrName = ref("title")
const value = ref("Tooltip")

const __returned__ = { attrName, value, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("p", _normalizeProps({ [$setup.attrName || ""]: $setup.value }), "Dynamic attribute", 16 /* FULL_PROPS */))
}
_sfc_main.render = render
export default _sfc_main
