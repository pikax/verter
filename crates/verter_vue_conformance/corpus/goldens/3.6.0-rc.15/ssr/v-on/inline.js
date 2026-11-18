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
import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<button${
    _ssrRenderAttrs(_attrs)
  }>Count: ${
    _ssrInterpolate($setup.count)
  }</button>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
