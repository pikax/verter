import { createElementVNode as _createElementVNode, Teleport as _Teleport, openBlock as _openBlock, createBlock as _createBlock } from "vue"
import { ref } from "vue"


const _sfc_main = {
  __name: 'teleport',
  setup(__props, { expose: __expose }) {
  __expose();

const open = ref(true)

const __returned__ = { open, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createBlock(_Teleport, {
    to: "body",
    disabled: !$setup.open
  }, [
    _cache[0] || (_cache[0] = _createElementVNode("p", { class: "overlay" }, "Overlay", -1 /* CACHED */))
  ], 8 /* PROPS */, ["disabled"]))
}
_sfc_main.render = render
export default _sfc_main
