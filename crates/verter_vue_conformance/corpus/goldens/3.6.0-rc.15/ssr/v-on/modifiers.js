
const _sfc_main = {
  __name: 'modifiers',
  setup(__props, { expose: __expose }) {
  __expose();

function save() {}
function del() {}
function open() {}

const __returned__ = { save, del, open }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<div${_ssrRenderAttrs(_attrs)}><button>Delete</button><a href="/x">Open</a></div>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
