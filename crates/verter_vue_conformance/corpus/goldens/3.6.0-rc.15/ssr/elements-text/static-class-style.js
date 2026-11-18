const _sfc_main = {}
import { mergeProps as _mergeProps } from "vue"
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs) {
  _push(`<div${_ssrRenderAttrs(_mergeProps({
    class: "card elevated",
    style: {"color":"red","margin-top":"4px"}
  }, _attrs))}><span class="label">Label</span></div>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
