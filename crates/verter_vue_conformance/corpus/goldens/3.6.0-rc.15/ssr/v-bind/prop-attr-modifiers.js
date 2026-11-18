import { ref } from "vue"


const _sfc_main = {
  __name: 'prop-attr-modifiers',
  setup(__props, { expose: __expose }) {
  __expose();

const text = ref("inner")
const dataX = ref("x")

const __returned__ = { text, dataX, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { mergeProps as _mergeProps } from "vue"
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${_ssrRenderAttrs(_mergeProps({
    "text-content": $setup.text,
    "data-x": $setup.dataX
  }, _attrs))}></div>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
