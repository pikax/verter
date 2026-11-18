import { ref } from "vue"


const _sfc_main = {
  __name: 'html',
  setup(__props, { expose: __expose }) {
  __expose();

const raw = ref("<b>bold</b>")

const __returned__ = { raw, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${
    _ssrRenderAttrs(_attrs)
  }>${
    ($setup.raw) ?? ''
  }</div>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
