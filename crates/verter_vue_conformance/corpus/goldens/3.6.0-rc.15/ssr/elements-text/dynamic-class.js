import { ref } from "vue"


const _sfc_main = {
  __name: 'dynamic-class',
  setup(__props, { expose: __expose }) {
  __expose();

const active = ref(true)
const size = ref("lg")

const __returned__ = { active, size, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { mergeProps as _mergeProps } from "vue"
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${_ssrRenderAttrs(_mergeProps({
    class: ['card', { active: $setup.active }, 'size-' + $setup.size]
  }, _attrs))}>Classy</div>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
