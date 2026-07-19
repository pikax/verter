import { vModelText as _vModelText, withDirectives as _withDirectives, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'input',
  setup(__props, { expose: __expose }) {
  __expose();

const text = ref("")

const __returned__ = { text, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return _withDirectives((_openBlock(), _createElementBlock("input", {
    "onUpdate:modelValue": _cache[0] || (_cache[0] = $event => (($setup.text) = $event)),
    placeholder: "Type here"
  }, null, 512 /* NEED_PATCH */)), [
    [_vModelText, $setup.text]
  ])
}
_sfc_main.render = render
export default _sfc_main
