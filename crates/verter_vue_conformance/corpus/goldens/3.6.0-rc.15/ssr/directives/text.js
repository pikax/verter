import { ref } from "vue"


const _sfc_main = {
  __name: 'text',
  setup(__props, { expose: __expose }) {
  __expose();

const plain = ref("plain text")

const __returned__ = { plain, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<p${
    _ssrRenderAttrs(_attrs)
  }>${
    _ssrInterpolate($setup.plain)
  }</p>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
