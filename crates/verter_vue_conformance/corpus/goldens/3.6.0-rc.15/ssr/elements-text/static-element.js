const _sfc_main = {}
import { mergeProps as _mergeProps } from "vue"
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs) {
  _push(`<div${_ssrRenderAttrs(_mergeProps({
    id: "app",
    title: "Hello",
    "data-role": "root"
  }, _attrs))}><p>Static text</p></div>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
