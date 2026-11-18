import { ref } from "vue"


const _sfc_main = {
  __name: 'same-name-shorthand',
  setup(__props, { expose: __expose }) {
  __expose();

const id = ref("a1")
const title = ref("Hi")

const __returned__ = { id, title, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { mergeProps as _mergeProps } from "vue"
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${_ssrRenderAttrs(_mergeProps({
    id: $setup.id,
    title: $setup.title
  }, _attrs))}></div>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
