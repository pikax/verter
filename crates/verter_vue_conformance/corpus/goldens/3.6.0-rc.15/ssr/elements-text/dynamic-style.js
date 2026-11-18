import { ref } from "vue"


const _sfc_main = {
  __name: 'dynamic-style',
  setup(__props, { expose: __expose }) {
  __expose();

const color = ref("red")
const top = ref(10)

const __returned__ = { color, top, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { mergeProps as _mergeProps } from "vue"
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${_ssrRenderAttrs(_mergeProps({
    style: { color: $setup.color, marginTop: $setup.top + 'px' }
  }, _attrs))}>Styled</div>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
