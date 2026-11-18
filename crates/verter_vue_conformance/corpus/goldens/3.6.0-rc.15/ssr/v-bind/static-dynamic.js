import { ref } from "vue"


const _sfc_main = {
  __name: 'static-dynamic',
  setup(__props, { expose: __expose }) {
  __expose();

const title = ref("Hello")
const disabled = ref(false)

const __returned__ = { title, disabled, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { mergeProps as _mergeProps } from "vue"
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<button${_ssrRenderAttrs(_mergeProps({
    type: "button",
    title: $setup.title,
    disabled: $setup.disabled
  }, _attrs))}>Go</button>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
