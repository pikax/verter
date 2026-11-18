import { ref } from "vue"


const _sfc_main = {
  __name: 'array',
  setup(__props, { expose: __expose }) {
  __expose();

const items = ref(["a", "b", "c"])

const __returned__ = { items, ref }
Object.defineProperty(__returned__, '__isScriptSetup', { enumerable: false, value: true })
return __returned__
}

}
import { ssrRenderAttrs as _ssrRenderAttrs, ssrInterpolate as _ssrInterpolate, ssrRenderList as _ssrRenderList } from "vue/server-renderer"
function ssrRender(_ctx, _push, _parent, _attrs, $props, $setup, $data, $options) {
  _push(`<ul${_ssrRenderAttrs(_attrs)}><!--[-->`)
  _ssrRenderList($setup.items, (item) => {
    _push(`<li>${_ssrInterpolate(item)}</li>`)
  })
  _push(`<!--]--></ul>`)
}
_sfc_main.ssrRender = ssrRender
export default _sfc_main
