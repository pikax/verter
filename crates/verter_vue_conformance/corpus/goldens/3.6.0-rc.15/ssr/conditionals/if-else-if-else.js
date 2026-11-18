import { ref } from "vue"


const _sfc_main = {
  __name: 'if-else-if-else',
  setup(__props, { expose: __expose }) {
  __expose();

const status = ref("loading")

const __returned__ = { status, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  if ($setup.status === 'loading') {
    _push(`<p${_ssrRenderAttrs(_attrs)}>Loading</p>`)
  } else if ($setup.status === 'error') {
    _push(`<p${_ssrRenderAttrs(_attrs)}>Failed</p>`)
  } else {
    _push(`<p${
      _ssrRenderAttrs(_attrs)
    }>Done: ${
      _ssrInterpolate($setup.status)
    }</p>`)
  }
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
