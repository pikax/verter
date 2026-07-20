import { toDisplayString as _toDisplayString, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'inline',
  setup(__props, { expose: __expose }) {
  __expose();

const count = ref(0)

const __returned__ = { count, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("button", {
    onClick: _cache[0] || (_cache[0] = $event => ($setup.count++))
  }, "Count: " + _toDisplayString($setup.count), 1 /* TEXT */))
}
_sfc_main.render = render
export default _sfc_main
