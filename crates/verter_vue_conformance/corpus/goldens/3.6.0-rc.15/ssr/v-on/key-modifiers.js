
const _sfc_main = {
  __name: 'key-modifiers',
  setup(__props, { expose: __expose }) {
  __expose();

function onEnter() {}
function onEsc() {}

const __returned__ = { onEnter, onEsc }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { ssrRenderAttrs as _ssrRenderAttrs } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<input${_ssrRenderAttrs(_attrs)}>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
