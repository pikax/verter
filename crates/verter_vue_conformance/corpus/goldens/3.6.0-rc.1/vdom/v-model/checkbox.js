import { vModelCheckbox as _vModelCheckbox, createElementVNode as _createElementVNode, withDirectives as _withDirectives, createTextVNode as _createTextVNode, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'checkbox',
  setup(__props, { expose: __expose }) {
  __expose();

const agreed = ref(false)

const __returned__ = { agreed, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("label", null, [
    _withDirectives(_createElementVNode("input", {
      type: "checkbox",
      "onUpdate:modelValue": _cache[0] || (_cache[0] = $event => (($setup.agreed) = $event))
    }, null, 512 /* NEED_PATCH */), [
      [_vModelCheckbox, $setup.agreed]
    ]),
    _cache[1] || (_cache[1] = _createTextVNode(" Agree", -1 /* CACHED */))
  ]))
}
_sfc_main.render = render
export default _sfc_main
