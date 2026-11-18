import { ref } from "vue"


const _sfc_main = {
  __name: 'input',
  setup(__props, { expose: __expose }) {
  __expose();

const text = ref("")

const __returned__ = { text, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { mergeProps as _mergeProps } from "vue"
import { ssrRenderAttrs as _ssrRenderAttrs, ssrGetDynamicModelProps as _ssrGetDynamicModelProps } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  let _temp0

  _push(`<input${_ssrRenderAttrs((_temp0 = _mergeProps({ placeholder: "Type here" }, _attrs), _mergeProps(_temp0, _ssrGetDynamicModelProps(_temp0, $setup.text))))}>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
