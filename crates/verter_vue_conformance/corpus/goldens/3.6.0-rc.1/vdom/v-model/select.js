import { createElementVNode as _createElementVNode, vModelSelect as _vModelSelect, withDirectives as _withDirectives, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'select',
  setup(__props, { expose: __expose }) {
  __expose();

const picked = ref("a")

const __returned__ = { picked, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return _withDirectives((_openBlock(), _createElementBlock("select", {
    "onUpdate:modelValue": _cache[0] || (_cache[0] = $event => (($setup.picked) = $event))
  }, [...(_cache[1] || (_cache[1] = [
    _createElementVNode("option", { value: "a" }, "A", -1 /* CACHED */),
    _createElementVNode("option", { value: "b" }, "B", -1 /* CACHED */)
  ]))], 512 /* NEED_PATCH */)), [
    [_vModelSelect, $setup.picked]
  ])
}
_sfc_main.render = render
export default _sfc_main
