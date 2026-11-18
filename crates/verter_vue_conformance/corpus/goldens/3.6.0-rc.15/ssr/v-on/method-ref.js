
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
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<form${_ssrRenderAttrs(_attrs)}><button>Send</button></form>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
