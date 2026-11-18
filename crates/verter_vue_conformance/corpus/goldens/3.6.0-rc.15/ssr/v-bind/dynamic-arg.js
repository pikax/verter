import { ref } from "vue"


const _sfc_main = {
  __name: 'dynamic-arg',
  setup(__props, { expose: __expose }) {
  __expose();

const attrName = ref("title")
const value = ref("Tooltip")

const __returned__ = { attrName, value, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { mergeProps as _mergeProps } from "vue"
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<p${_ssrRenderAttrs(_mergeProps({ [$setup.attrName || ""]: $setup.value }, _attrs))}>Dynamic attribute</p>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
