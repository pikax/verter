import { createElementVNode as _createElementVNode, openBlock as _openBlock, createElementBlock as _createElementBlock } from "vue"

const _sfc_main = {
  __name: 'method-ref',
  setup(__props, { expose: __expose }) {
  __expose();

function onSubmit() {}

const __returned__ = { onSubmit }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
function render(_ctx, _cache, $props, $setup, $data, $options) {
  return (_openBlock(), _createElementBlock("form", { onSubmit: $setup.onSubmit }, [...(_cache[0] || (_cache[0] = [
    _createElementVNode("button", null, "Send", -1 /* CACHED */)
  ]))], 32 /* NEED_HYDRATION */))
}
_sfc_main.render = render
export default _sfc_main
